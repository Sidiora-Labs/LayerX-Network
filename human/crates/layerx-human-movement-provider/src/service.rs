use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use layerx_human_service::custody::RemoteKmsProvider;
use layerx_human_service::journeys::{
    ExitWalletOutcome, MovementExecutionIdentity, PaxeerActionOutcome, SettlementConfig,
    WalletCustodyOutcome, WalletCustodyRequest,
};
use layerx_human_service::server::movement_provider::{
    MovementProviderCodec, MovementProviderRequest as Request,
    MovementProviderResponse as Response, MovementProviderService, NativeMovementCodec,
};
use layerx_paxeer_client::{
    raw_call, DepositFailure, DepositProof, DepositProofVerifier, DepositRootRegistration,
    FinalityReport, FinalityTracker, Json, ProofFault, PublishedDepositProof, TrackerConfig,
    TransactionHash,
};
use layerx_types::intent::EvmAddress;
use sha2::{Digest, Sha256};
use sha3::Keccak256;

use crate::config::{hex, hex_string, Config, MAX_FRAME};
use crate::journal::{private_directory, read_private, Journal};
use crate::Error;

pub(crate) struct EvidenceService {
    journal: Journal,
    codec: NativeMovementCodec,
    tracker_config: TrackerConfig,
    trackers: BTreeMap<[u8; 32], FinalityTracker>,
    verifier: DepositProofVerifier,
    evidence_root: PathBuf,
    vault: EvmAddress,
    chain_id: u64,
    checkpoint_registry: EvmAddress,
    claims_contract: EvmAddress,
    exit_contract: EvmAddress,
    executor: Option<Arc<RemoteKmsProvider>>,
    policy: layerx_paxeer_client::DepositProofConfig,
    settlement: SettlementConfig,
    reminder: u64,
    exit: layerx_paxeer_client::EmergencyExit,
}

impl EvidenceService {
    pub fn new(config: &Config, journal: Journal) -> Result<Self, Error> {
        private_directory(&config.evidence_root)?;
        FinalityTracker::new(config.tracker.clone(), TransactionHash::new([1; 32]))
            .map_err(|_| Error::Configuration)?;
        let verifier =
            DepositProofVerifier::new(config.proof.clone()).map_err(|_| Error::Configuration)?;
        if config.proof.endpoints != config.tracker.endpoints
            || config.proof.minimum_endpoint_agreement != config.tracker.minimum_endpoint_agreement
            || config.proof.required_confirmations != config.tracker.required_confirmations
            || config.proof.layerx_protocol_version != config.listener.protocol
            || config.vault.bytes() == [0; 20]
        {
            return Err(Error::Configuration);
        }
        Ok(Self {
            journal,
            codec: NativeMovementCodec::for_protocol(config.listener.protocol)
                .map_err(|_| Error::Configuration)?,
            tracker_config: config.tracker.clone(),
            trackers: BTreeMap::new(),
            verifier,
            evidence_root: config.evidence_root.clone(),
            vault: config.vault,
            chain_id: config.proof.paxeer_chain_id,
            checkpoint_registry: config.checkpoint_registry,
            claims_contract: config.claims_contract,
            exit_contract: config.exit_contract,
            executor: config.executor.clone(),
            policy: config.proof.clone(),
            settlement: SettlementConfig {
                checkpoint_interval_seconds: config.checkpoint_interval_seconds,
                paxeer_block_seconds: config.paxeer_block_seconds,
                required_confirmations: config.tracker.required_confirmations,
            },
            reminder: config.reminder_interval_seconds,
            exit: layerx_paxeer_client::EmergencyExit::new(layerx_paxeer_client::ExitConfig {
                endpoints: config.tracker.endpoints.clone(),
                minimum_endpoint_agreement: config.tracker.minimum_endpoint_agreement,
                exit_contract: config.exit_contract,
                required_confirmations: config.tracker.required_confirmations,
                poll_cadence: config.tracker.poll_cadence,
                delayed_after_polls: config.tracker.delayed_after_polls,
            })
            .map_err(|_| Error::Configuration)?,
        })
    }

    fn poll(&mut self, transaction: TransactionHash) -> Result<FinalityReport, Error> {
        if transaction.bytes() == [0; 32] {
            return Err(Error::Integrity);
        }
        if !self.trackers.contains_key(&transaction.bytes()) {
            if self.trackers.len() >= 1024 {
                return Err(Error::Capacity);
            }
            self.trackers.insert(
                transaction.bytes(),
                FinalityTracker::new(self.tracker_config.clone(), transaction)
                    .map_err(|_| Error::Configuration)?,
            );
        }
        Ok(self
            .trackers
            .get_mut(&transaction.bytes())
            .ok_or(Error::Integrity)?
            .poll())
    }

    fn obtain(&mut self, transaction: TransactionHash) -> Result<DepositProof, DepositFailure> {
        let file = self
            .evidence_root
            .join(format!("deposit-{}.bin", hex_string(&transaction.bytes())));
        let bytes = read_private(&file, MAX_FRAME).map_err(|error| match error {
            Error::Io(io) if io.kind() == std::io::ErrorKind::NotFound => {
                proof_error(ProofFault::ProducerUnavailable)
            }
            _ => proof_error(ProofFault::EvidenceSourceMismatch),
        })?;
        let Response::DepositProof(Ok(candidate)) = self
            .codec
            .decode_response(&bytes)
            .map_err(|_| proof_error(ProofFault::EvidenceSourceMismatch))?
        else {
            return Err(proof_error(ProofFault::EvidenceSourceMismatch));
        };
        if candidate.transaction() != transaction || candidate.vault() != self.vault {
            return Err(proof_error(ProofFault::EvidenceSourceMismatch));
        }
        let published = PublishedDepositProof {
            registration: DepositRootRegistration {
                checkpoint_id: candidate.checkpoint_id().bytes(),
                checkpoint_state_root: candidate.checkpoint_state_root(),
                deposit_root: candidate.deposit_root(),
                custody_reference: candidate.custody_reference(),
                network_id: candidate.network_id(),
                protocol_version: candidate.protocol_version(),
                signature: candidate.registration_signature(),
            },
            inclusion_proof: candidate.inclusion_proof().clone(),
        };
        let report = self
            .poll(transaction)
            .map_err(|_| proof_error(ProofFault::MissingQuorumEvidence))?;
        let verified = self.verifier.obtain(&report, self.vault, published)?;
        if candidate.custody() != verified.custody()
            || candidate.inclusion() != verified.inclusion()
            || candidate.nullifier() != verified.nullifier()
        {
            return Err(proof_error(ProofFault::EvidenceSourceMismatch));
        }
        Ok(verified)
    }

    fn verify_external(
        &mut self,
        request: &WalletCustodyRequest,
        transaction: TransactionHash,
    ) -> Response {
        if request.chain_id != self.chain_id || request.vault != self.vault {
            return Response::ContractViolation;
        }
        let Ok(proof) = self.obtain(transaction) else {
            return Response::Unavailable;
        };
        let custody = proof.custody();
        if custody.payer != request.wallet
            || custody.asset != request.asset
            || custody.beneficiary != request.beneficiary
            || custody.amount != request.amount
        {
            return Response::ContractViolation;
        }
        let expected_input = deposit_calldata(request);
        let agreement = self
            .tracker_config
            .endpoints
            .iter()
            .filter(|endpoint| {
                let Ok(value) = raw_call(
                    endpoint,
                    "eth_getTransactionByHash",
                    &[Json::Text(transaction.to_hex())],
                ) else {
                    return false;
                };
                transaction_matches(&value, request, &proof, &expected_input)
            })
            .count();
        if agreement < self.tracker_config.minimum_endpoint_agreement {
            return Response::Unavailable;
        }
        Response::VerifiedDeposit(transaction)
    }

    fn bind_debit(
        &self,
        identity: &layerx_human_service::journeys::MovementExecutionIdentity,
        debit: &layerx_paxeer_client::DebitExpectation,
    ) -> Response {
        let Ok(plan) = self.journal.authorized_plan(identity) else {
            return Response::ContractViolation;
        };
        let context = plan.context;
        if plan.operation != "withdraw.start"
            || debit.activity_id != debit.withdrawal_id
            || debit.network_id != context.network.value()
            || debit.account != identity.account
            || debit.recipient != context.wallet
            || debit.asset_id != context.asset.bytes()
            || debit.amount != context.amount.value()
            || layerx_paxeer_client::account_address_for_protocol(
                &context.withdrawals_account,
                context.protocol_version,
            )
            .ok()
                != Some(debit.withdrawals_account)
        {
            return Response::ContractViolation;
        }
        Response::Ready
    }

    fn execute(&mut self, request: &Request) -> Response {
        match request {
            Request::CheckpointProof(debit) => self.checkpoint(debit),
            Request::BindWithdrawalDebit {
                identity, debit, ..
            } => self.bind_debit(identity, debit),
            Request::PollDepositFinality(transaction) => self
                .poll(*transaction)
                .map_or(Response::Unavailable, Response::DepositFinality),
            Request::ObtainDepositProof(transaction) => {
                Response::DepositProof(self.obtain(*transaction))
            }
            Request::VerifyExternalDeposit {
                request,
                transaction,
            } => self.verify_external(request, *transaction),
            Request::PlanMove(plan)
            | Request::PlanDeposit(plan)
            | Request::PlanWithdrawal(plan)
            | Request::PlanExit(plan) => {
                if self.executor.is_none() {
                    return Response::Unavailable;
                }
                if crate::planning::validate(plan, &self.policy).is_err() {
                    return Response::ContractViolation;
                }
                let result = match request {
                    Request::PlanMove(_) => {
                        crate::planning::move_plan(plan).map(Response::MovePlan)
                    }
                    Request::PlanDeposit(_) => {
                        crate::planning::deposit_plan(plan, self.vault).map(Response::DepositPlan)
                    }
                    Request::PlanWithdrawal(_) => {
                        crate::planning::withdrawal_plan(plan, self.settlement, self.reminder)
                            .map(Response::WithdrawalPlan)
                    }
                    Request::PlanExit(_) => self.exit_plan(plan),
                    _ => Err(Error::Integrity),
                };
                result.unwrap_or(Response::Unavailable)
            }
            Request::PrepareEvmTransaction {
                identity,
                action_key,
                target,
                calldata,
            } => self
                .prepare(identity, *action_key, *target, calldata)
                .unwrap_or(Response::Unavailable),
            Request::SubmitDepositCustody(value) => {
                let calldata = deposit_calldata(value);
                let execution = crate::execution::Execution {
                    identity: &value.identity,
                    action_key: value.action_key,
                    target: value.vault,
                    calldata: &calldata,
                    signed_transaction: None,
                };
                match self.submit(&execution) {
                    Ok(Some(hash)) => {
                        Response::DepositCustody(WalletCustodyOutcome::Submitted(hash))
                    }
                    _ => Response::Unavailable,
                }
            }
            Request::SubmitWithdrawal(value) => match self.submit(&withdrawal_execution(value)) {
                Ok(Some(hash)) => Response::Withdrawal(PaxeerActionOutcome::Submitted(hash)),
                Ok(None) => Response::Withdrawal(PaxeerActionOutcome::Unknown),
                Err(_) => Response::Unavailable,
            },
            Request::SubmitExit(value) => {
                let execution = crate::execution::Execution {
                    identity: &value.identity,
                    action_key: value.action_key,
                    target: value.contract,
                    calldata: &value.calldata,
                    signed_transaction: None,
                };
                match self.submit(&execution) {
                    Ok(Some(hash)) => Response::Exit(ExitWalletOutcome::Submitted(hash)),
                    _ => Response::Unavailable,
                }
            }
            Request::LookupWithdrawal(key) => self.lookup(*key).unwrap_or(Response::Unavailable),
            Request::VerifyClaimSignature { request, signature } => self
                .verify_signature(request, signature)
                .unwrap_or(Response::Unavailable),
            Request::Readiness => Response::Unavailable,
        }
    }

    fn authorized_execution(
        &self,
        execution: &crate::execution::Execution<'_>,
    ) -> Result<layerx_human_service::server::movement_provider::PlanningRequest, Error> {
        let plan = self.journal.authorized_plan(execution.identity)?;
        let expected_target = match plan.operation.as_str() {
            "deposit.start" => self.vault,
            "withdraw.start" => self.claims_contract,
            "exit.start" => self.exit_contract,
            _ => return Err(Error::Integrity),
        };
        if execution.target != expected_target {
            return Err(Error::Integrity);
        }
        Ok(plan)
    }

    fn prepare(
        &self,
        identity: &MovementExecutionIdentity,
        action_key: [u8; 32],
        target: EvmAddress,
        calldata: &[u8],
    ) -> Result<Response, Error> {
        if self.executor.is_none() {
            return Err(Error::Configuration);
        }
        let execution = crate::execution::Execution {
            identity,
            action_key,
            target,
            calldata,
            signed_transaction: None,
        };
        let plan = self.authorized_execution(&execution)?;
        let observed = crate::execution::pending_nonce(&self.tracker_config, identity.wallet)?;
        let nonce =
            self.journal
                .next_nonce(identity.wallet, plan.context.paxeer_chain_id, observed)?;
        crate::execution::prepare(&plan, &execution, nonce).map(Response::PreparedEvmTransaction)
    }

    fn submit(
        &self,
        execution: &crate::execution::Execution<'_>,
    ) -> Result<Option<TransactionHash>, Error> {
        let kms = self.executor.as_ref().ok_or(Error::Configuration)?;
        let plan = self.authorized_execution(execution)?;
        crate::execution::submit(kms, &self.tracker_config, &plan, execution)
    }

    fn lookup(&self, key: [u8; 32]) -> Result<Response, Error> {
        let Some(Request::SubmitWithdrawal(request)) = self.journal.withdrawal_request(key)? else {
            return Ok(Response::WithdrawalLookup(None));
        };
        let execution = withdrawal_execution(&request);
        let plan = self.authorized_execution(&execution)?;
        let kms = self.executor.as_ref().ok_or(Error::Configuration)?;
        crate::execution::lookup(kms, &self.tracker_config, &plan, &execution)
            .map(Response::WithdrawalLookup)
    }

    fn verify_signature(
        &self,
        request: &layerx_human_service::journeys::WithdrawalTransactionRequest,
        signature: &[u8],
    ) -> Result<Response, Error> {
        let execution = withdrawal_execution(request);
        let plan = self.authorized_execution(&execution)?;
        let kms = self.executor.as_ref().ok_or(Error::Configuration)?;
        crate::execution::external_signature(kms, &plan, &execution, signature)
            .map(Response::ClaimTransaction)
    }

    fn exit_plan(
        &self,
        request: &layerx_human_service::server::movement_provider::PlanningRequest,
    ) -> Result<Response, Error> {
        let context = &request.context;
        let account = layerx_paxeer_client::account_address_for_protocol(
            &context.account,
            context.protocol_version,
        )
        .map_err(|_| Error::Integrity)?;
        let file = self.evidence_root.join(format!(
            "exit-{}-{}.bin",
            hex_string(&account),
            hex_string(&context.asset.bytes())
        ));
        let bytes = read_private(&file, MAX_FRAME)?;
        let Response::ExitPlan(mut plan) = self
            .codec
            .decode_response(&bytes)
            .map_err(|_| Error::Integrity)?
        else {
            return Err(Error::Integrity);
        };
        if plan.evidence.account != account
            || plan.evidence.asset_id != context.asset.bytes()
            || plan.evidence.recipient != context.wallet
            || plan.evidence.finalised_balance != context.amount.value()
        {
            return Err(Error::Integrity);
        }
        self.exit
            .construct_claim(&plan.evidence)
            .map_err(|_| Error::Integrity)?;
        plan.journey_id = layerx_human_service::notify::JourneyId::new(format!(
            "exit-{}",
            hex_string(&crate::planning::identity(request, b"exit")?)
        ))
        .map_err(|_| Error::Integrity)?;
        plan.idempotency_key = request.idempotency_key;
        Ok(Response::ExitPlan(plan))
    }

    fn checkpoint(&self, debit: &layerx_paxeer_client::DebitExpectation) -> Response {
        if debit.validated().is_err() || !self.journal.has_withdrawal_debit(debit) {
            return Response::ContractViolation;
        }
        let file = self.evidence_root.join(format!(
            "withdrawal-{}.bin",
            hex_string(&debit.withdrawal_id)
        ));
        let Ok(bytes) = read_private(&file, MAX_FRAME) else {
            return Response::Unavailable;
        };
        let Ok(Response::CheckpointProof(Some(proof))) = self.codec.decode_response(&bytes) else {
            return Response::ContractViolation;
        };
        if crate::producer::verify_checkpoint_registration(
            &self.tracker_config,
            self.checkpoint_registry,
            &proof,
        )
        .is_err()
        {
            return Response::Unavailable;
        }
        Response::CheckpointProof(Some(proof))
    }
}

impl MovementProviderService for EvidenceService {
    fn dispatch(&mut self, request: Request) -> Response {
        let Ok(bytes) = self.codec.encode_request(&request) else {
            return Response::ContractViolation;
        };
        let key = match &request {
            Request::PlanMove(plan)
            | Request::PlanDeposit(plan)
            | Request::PlanWithdrawal(plan)
            | Request::PlanExit(plan) => {
                let Ok(key) = crate::planning::identity(plan, plan.operation.as_bytes()) else {
                    return Response::ContractViolation;
                };
                hex_string(&key)
            }
            Request::BindWithdrawalDebit { identity, .. } => {
                let mut hash = Sha256::new();
                hash.update(b"lxmp-withdrawal-receipt/v1\0");
                for value in [
                    identity.principal.as_str().as_bytes(),
                    identity.tenant.as_str().as_bytes(),
                    &identity.plan_id,
                ] {
                    hash.update((value.len() as u64).to_be_bytes());
                    hash.update(value);
                }
                hex_string(&hash.finalize())
            }
            Request::PrepareEvmTransaction { action_key, .. } => {
                action_key_hash(b"prepare", action_key)
            }
            Request::SubmitDepositCustody(value) => action_key_hash(b"deposit", &value.action_key),
            Request::SubmitWithdrawal(value) => action_key_hash(b"withdrawal", &value.action_key),
            Request::SubmitExit(value) => action_key_hash(b"exit", &value.action_key),
            Request::VerifyClaimSignature { request, .. } => {
                action_key_hash(b"signature", &request.action_key)
            }
            Request::VerifyExternalDeposit { request, .. } => hex_string(&Sha256::digest(
                [b"lxmp-external-action/v1\0".as_slice(), &request.action_key].concat(),
            )),
            _ => hex_string(&Sha256::digest(&bytes)),
        };
        if let Err(error) = self.journal.begin(&key, &bytes) {
            return match error {
                Error::Conflict => Response::ContractViolation,
                _ => Response::Unavailable,
            };
        }
        if matches!(
            request,
            Request::PlanMove(_)
                | Request::PlanDeposit(_)
                | Request::PlanWithdrawal(_)
                | Request::PlanExit(_)
                | Request::PrepareEvmTransaction { .. }
        ) {
            if let Some(encoded) = self
                .journal
                .record(&key)
                .and_then(|record| record.response.as_ref())
            {
                if let Ok(response) = self.codec.decode_response(encoded) {
                    if !matches!(
                        response,
                        Response::Unavailable | Response::ContractViolation
                    ) {
                        return response;
                    }
                }
            }
        }
        let response = self.execute(&request);
        let Ok(encoded) = self.codec.encode_response(&response) else {
            return Response::ContractViolation;
        };
        if self.journal.complete(&key, &encoded).is_err() {
            return Response::Unavailable;
        }
        response
    }
}

fn proof_error(fault: ProofFault) -> DepositFailure {
    DepositFailure::ProofUnavailable(fault)
}

fn action_key_hash(purpose: &[u8], key: &[u8; 32]) -> String {
    let mut digest = Sha256::new();
    digest.update(b"layerx-human-movement-provider/request/v2\0");
    digest.update(purpose);
    digest.update([0]);
    digest.update(key);
    hex_string(&digest.finalize())
}

fn withdrawal_execution(
    request: &layerx_human_service::journeys::WithdrawalTransactionRequest,
) -> crate::execution::Execution<'_> {
    crate::execution::Execution {
        identity: &request.identity,
        action_key: request.action_key,
        target: request.target,
        calldata: &request.calldata,
        signed_transaction: request.signed_transaction.as_deref(),
    }
}

fn deposit_calldata(request: &WalletCustodyRequest) -> Vec<u8> {
    let selector = Keccak256::digest(b"deposit(bytes32,uint256,bytes32)");
    let mut bytes = selector[..4].to_vec();
    bytes.extend(request.asset.bytes());
    bytes.extend([0; 16]);
    bytes.extend(request.amount.to_be_bytes());
    bytes.extend(request.beneficiary);
    bytes
}

fn transaction_matches(
    value: &Json,
    request: &WalletCustodyRequest,
    proof: &DepositProof,
    input: &[u8],
) -> bool {
    let text = |key| value.member(key).and_then(Json::as_text);
    let bytes32 = |key| text(key).and_then(|v| hex::<32>(v).ok());
    let address = |key| text(key).and_then(|v| hex::<20>(v).ok());
    let quantity = |key| {
        text(key)
            .and_then(|v| v.strip_prefix("0x"))
            .and_then(|v| u64::from_str_radix(v, 16).ok())
    };
    bytes32("hash") == Some(proof.transaction().bytes())
        && bytes32("blockHash") == Some(proof.inclusion().block.hash)
        && quantity("blockNumber") == Some(proof.inclusion().block.number)
        && quantity("chainId") == Some(request.chain_id)
        && quantity("value") == Some(0)
        && address("from") == Some(request.wallet.bytes())
        && address("to") == Some(request.vault.bytes())
        && text("input") == Some(format!("0x{}", hex_string(input)).as_str())
}
