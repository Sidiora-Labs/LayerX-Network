use layerx_agent_api::error::RequestId;
use layerx_agent_api::idempotency::{BodyDigest, IdempotentMutation, Key};
use layerx_agent_api::identity::{AgentDid, AuthorityRef, ContractError};
use layerx_agent_api::prepare::{
    IdempotencyRef, PayloadBytes, PrepareRequest as AgentPrepareRequest,
    TimestampBound as AgentTimestampBound,
};
use layerx_agent_api::{Amount as AgentAmount, Sequence};
use layerx_intents::{
    compile, BridgeDepositCredit, CompileError, CompiledIntent, Intent, IntentError, IntentKind,
};
use layerx_proof::checkpoint::{verify_certificate, Certificate, CheckpointError, GuarantorKey};
use layerx_proof::receipt::{
    verify_outcome, AuthorizedBatch, ReceiptCheck, VerificationFailure, VerifiedReceipt,
};
use layerx_sdk::{Call, Client as AgentClient};
use layerx_types::account::{AccountId, AccountNamespace};
use layerx_types::amount::Amount;
use layerx_types::ids::{AssetId, CheckpointId, IdempotencyKey};
use layerx_types::intent::{DepositProofId, EvmAddress};
use layerx_types::payload::ModuleRegistry;
use sha2::{Digest as _, Sha256};

use crate::client::{
    BlockRef, ClientConfigError, EndpointError, ExecutionOutcome, LogRecord, PaxeerClient,
    QuorumBinding, TransactionHash, TransactionInclusion,
};
use crate::finality::{FinalityReport, FinalityStage, TrackerConfigError};
use crate::rpc::EndpointConfig;

const CUSTODY_DEPOSIT_TOPIC: [u8; 32] = [
    0x7e, 0xdb, 0x71, 0xc9, 0x10, 0x0c, 0x65, 0x68, 0x47, 0x89, 0x6d, 0x0b, 0x5b, 0x19, 0x4f, 0x69,
    0xf7, 0xda, 0x28, 0x7e, 0xb5, 0x79, 0x64, 0xa8, 0x1e, 0x7f, 0x80, 0x7a, 0x6a, 0x94, 0x40, 0x28,
];

const CUSTODY_DEPOSIT_DOMAIN: &[u8] = b"LXP/Paxeer/custody-deposit/v1";
const ACCOUNT_ID_DOMAIN: &[u8] = b"LXP/v1/account-id\0";
const CREDIT_KEY_DOMAIN: &[u8] = b"LXP/human/deposit-credit-key/v1\0";
const PROOF_COMMITMENT_DOMAIN: &[u8] = b"LX:DEPOSIT:PROOF:v1";

/// Derives the exact 32-byte account address custody names as beneficiary.
#[must_use]
pub fn account_address(account: &AccountId) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(ACCOUNT_ID_DOMAIN);
    hasher.update(account.canonical().as_bytes());
    hasher.finalize().into()
}

/// One `CustodyDeposit` event exactly as the vault emitted it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CustodyDeposit {
    pub deposit_id: [u8; 32],
    pub asset: AssetId,
    pub payer: EvmAddress,
    pub beneficiary: [u8; 32],
    pub amount: Amount,
    pub nonce: u64,
}

/// A checkpoint admitted only after its canonical header, bonded signatures,
/// threshold, and optional settlement registration have verified.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FinalizedCheckpoint {
    id: CheckpointId,
    state_root: [u8; 32],
    network_id: u32,
    protocol_version: u16,
}

impl FinalizedCheckpoint {
    /// Verifies core-produced checkpoint evidence and extracts the exact bridge
    /// proof fields from the signed canonical batch header.
    ///
    /// # Errors
    ///
    /// Returns the proof verifier's exact certificate or canonical-header
    /// failure. A value cannot be constructed from an unverified identifier.
    pub fn verify(
        certificate: &Certificate,
        bonded_set: &[GuarantorKey],
        registered_checkpoint_id: CheckpointId,
        registered_settlement_reference: Option<&[u8]>,
    ) -> Result<Self, CheckpointError> {
        let report = verify_certificate(
            certificate,
            bonded_set,
            &registered_checkpoint_id.bytes(),
            registered_settlement_reference,
        )?;
        Ok(Self {
            id: registered_checkpoint_id,
            state_root: report.resulting_state_root(),
            network_id: report.network_id(),
            protocol_version: report.protocol_version(),
        })
    }

    #[must_use]
    pub const fn id(&self) -> CheckpointId {
        self.id
    }

    #[must_use]
    pub const fn state_root(&self) -> [u8; 32] {
        self.state_root
    }

    #[must_use]
    pub const fn network_id(&self) -> u32 {
        self.network_id
    }

    #[must_use]
    pub const fn protocol_version(&self) -> u16 {
        self.protocol_version
    }
}

/// Why the custody transaction itself failed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CustodyFault {
    Reverted {
        inclusion: TransactionInclusion,
    },
    Displaced {
        lost: TransactionInclusion,
        head: u64,
        requeued: bool,
    },
}

/// Why the finalised custody proof is not available right now.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProofFault {
    NotFinal {
        stage: FinalityStage,
    },
    Unreadable {
        error: EndpointError,
    },
    InclusionChanged {
        tracked: BlockRef,
        observed: Option<BlockRef>,
    },
    MissingQuorumEvidence,
    EvidenceSourceMismatch,
    ConfirmationPolicyMismatch {
        expected: u64,
        reported: u64,
    },
    ConfirmationEvidenceMismatch {
        reported: u64,
        observed: u64,
    },
    MissingCustodyEvent {
        vault: EvmAddress,
    },
    MalformedCustodyEvent {
        detail: String,
    },
    AmountOverflow {
        beneficiary: [u8; 32],
    },
    UnboundDeposit {
        emitted: [u8; 32],
        derived: [u8; 32],
    },
    Checkpoint(CheckpointError),
}

/// Why the credit submission or its receipt was refused.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CreditFault {
    BeneficiaryMismatch {
        beneficiary: [u8; 32],
        recipient: [u8; 32],
    },
    ReserveNamespace,
    Intent(IntentError),
    Compile(CompileError),
    AgentContract(ContractError),
    Unverifiable(VerificationFailure),
    Refused {
        result_code: i32,
    },
    WrongActivity {
        expected: [u8; 32],
        found: [u8; 32],
    },
    WrongAsset {
        expected: AssetId,
        found: [u8; 32],
    },
    WrongAmount {
        expected: Amount,
        found: u128,
    },
    WrongReserve {
        expected: [u8; 32],
        found: [u8; 32],
    },
    WrongRecipient {
        expected: [u8; 32],
        found: [u8; 32],
    },
}

/// The three deposit failure classes the journey engine consumes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DepositFailure {
    CustodyFailed(CustodyFault),
    ProofUnavailable(ProofFault),
    CreditRefused(CreditFault),
}

/// The exact Paxeer quorum and confirmation policy permitted to mint a
/// custody proof.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DepositProofConfig {
    pub endpoints: Vec<EndpointConfig>,
    pub minimum_endpoint_agreement: usize,
    pub required_confirmations: u64,
}

/// Why a custody-proof authority configuration was refused.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DepositProofConfigError {
    Endpoints(ClientConfigError),
    Agreement(TrackerConfigError),
    ZeroRequiredConfirmations,
}

/// The configured authority that can mint a [`DepositProof`].
///
/// A report supplies opaque quorum evidence, but never selects the quorum or
/// confirmation policy under which the proof is accepted.
#[derive(Clone, Debug)]
pub struct DepositProofVerifier {
    binding: QuorumBinding,
    required_confirmations: u64,
}

struct AdmittedFinality<'a> {
    inclusion: TransactionInclusion,
    confirmations: u64,
    chain_id: u64,
    logs: &'a [LogRecord],
}

impl DepositProofVerifier {
    /// Validates and owns the exact endpoint quorum and confirmation depth
    /// under which deposit proofs may be minted.
    ///
    /// # Errors
    ///
    /// Refuses zero confirmation depth, an invalid endpoint agreement, or an
    /// invalid endpoint declaration.
    pub fn new(config: DepositProofConfig) -> Result<Self, DepositProofConfigError> {
        if config.required_confirmations == 0 {
            return Err(DepositProofConfigError::ZeroRequiredConfirmations);
        }
        crate::finality::validate_endpoint_agreement(
            &config.endpoints,
            config.minimum_endpoint_agreement,
        )
        .map_err(DepositProofConfigError::Agreement)?;
        let client = PaxeerClient::new(config.endpoints)
            .map_err(DepositProofConfigError::Endpoints)?;
        let binding = client.quorum_binding(config.minimum_endpoint_agreement);
        Ok(Self {
            binding,
            required_confirmations: config.required_confirmations,
        })
    }

    /// Verifies checkpoint evidence and constructs the custody proof as one
    /// typed operation under this verifier's policy.
    ///
    /// # Errors
    ///
    /// Classifies certificate failures as proof-unavailable and otherwise
    /// returns the same custody and chain failures as [`Self::obtain`].
    #[allow(clippy::too_many_arguments)]
    pub fn obtain_from_certificate(
        &self,
        report: &FinalityReport,
        vault: EvmAddress,
        certificate: &Certificate,
        bonded_set: &[GuarantorKey],
        registered_checkpoint_id: CheckpointId,
        registered_settlement_reference: Option<&[u8]>,
    ) -> Result<DepositProof, DepositFailure> {
        let checkpoint = FinalizedCheckpoint::verify(
            certificate,
            bonded_set,
            registered_checkpoint_id,
            registered_settlement_reference,
        )
        .map_err(|error| DepositFailure::ProofUnavailable(ProofFault::Checkpoint(error)))?;
        self.obtain(report, vault, checkpoint)
    }

    /// Constructs a finalized custody proof from one tracked transaction and
    /// a verified core checkpoint under this verifier's exact policy.
    ///
    /// # Errors
    ///
    /// Returns [`DepositFailure::CustodyFailed`] for a reverted or displaced
    /// transaction and [`DepositFailure::ProofUnavailable`] until every chain,
    /// quorum, policy, and proof binding is available.
    pub fn obtain(
        &self,
        report: &FinalityReport,
        vault: EvmAddress,
        checkpoint: FinalizedCheckpoint,
    ) -> Result<DepositProof, DepositFailure> {
        let admitted = self.admit_finality(report)?;
        let custody = custody_event(admitted.logs, vault)?;
        let derived = derive_deposit_id(admitted.chain_id, vault, &custody);
        if derived != custody.deposit_id {
            return Err(DepositFailure::ProofUnavailable(
                ProofFault::UnboundDeposit {
                    emitted: custody.deposit_id,
                    derived,
                },
            ));
        }
        let commitment = proof_commitment(report.transaction(), &custody, &checkpoint);
        Ok(DepositProof {
            transaction: report.transaction(),
            inclusion: admitted.inclusion,
            confirmations: admitted.confirmations,
            required: self.required_confirmations,
            chain_id: admitted.chain_id,
            vault,
            custody,
            checkpoint,
            finalized: true,
            commitment,
        })
    }

    fn admit_finality<'a>(
        &self,
        report: &'a FinalityReport,
    ) -> Result<AdmittedFinality<'a>, DepositFailure> {
        let (inclusion, reported_confirmations, reported_required) = match report.stage() {
            FinalityStage::Final {
                inclusion,
                confirmations,
                required,
            } => (inclusion, confirmations, required),
            FinalityStage::Displaced {
                lost,
                head,
                requeued,
            } => {
                return Err(DepositFailure::CustodyFailed(CustodyFault::Displaced {
                    lost,
                    head,
                    requeued,
                }));
            }
            stage => {
                return Err(DepositFailure::ProofUnavailable(ProofFault::NotFinal {
                    stage,
                }));
            }
        };
        if inclusion.execution == ExecutionOutcome::Reverted {
            return Err(DepositFailure::CustodyFailed(CustodyFault::Reverted {
                inclusion,
            }));
        }
        let evidence = report.evidence().ok_or(DepositFailure::ProofUnavailable(
            ProofFault::MissingQuorumEvidence,
        ))?;
        if evidence.binding() != &self.binding {
            return Err(DepositFailure::ProofUnavailable(
                ProofFault::EvidenceSourceMismatch,
            ));
        }
        if reported_required != self.required_confirmations {
            return Err(DepositFailure::ProofUnavailable(
                ProofFault::ConfirmationPolicyMismatch {
                    expected: self.required_confirmations,
                    reported: reported_required,
                },
            ));
        }
        let current = match evidence.transaction() {
            crate::client::TransactionView::Included(current) => current,
            crate::client::TransactionView::Unknown | crate::client::TransactionView::Pending => {
                return Err(DepositFailure::ProofUnavailable(
                    ProofFault::InclusionChanged {
                        tracked: inclusion.block,
                        observed: None,
                    },
                ));
            }
        };
        if evidence.canonical_block() != Some(inclusion.block) {
            return Err(DepositFailure::ProofUnavailable(
                ProofFault::InclusionChanged {
                    tracked: inclusion.block,
                    observed: evidence.canonical_block(),
                },
            ));
        }
        if current != inclusion {
            return Err(DepositFailure::ProofUnavailable(
                ProofFault::InclusionChanged {
                    tracked: inclusion.block,
                    observed: Some(current.block),
                },
            ));
        }
        let observed_confirmations = evidence
            .head()
            .saturating_sub(inclusion.block.number)
            .saturating_add(1);
        if reported_confirmations != observed_confirmations {
            return Err(DepositFailure::ProofUnavailable(
                ProofFault::ConfirmationEvidenceMismatch {
                    reported: reported_confirmations,
                    observed: observed_confirmations,
                },
            ));
        }
        if observed_confirmations < self.required_confirmations {
            return Err(DepositFailure::ProofUnavailable(ProofFault::NotFinal {
                stage: report.stage(),
            }));
        }
        let logs = evidence.receipt_logs().ok_or(DepositFailure::ProofUnavailable(
            ProofFault::MissingQuorumEvidence,
        ))?;
        Ok(AdmittedFinality {
            inclusion,
            confirmations: observed_confirmations,
            chain_id: evidence.chain_id(),
            logs,
        })
    }

    #[cfg(test)]
    pub(crate) fn verify_report_policy(
        &self,
        report: &FinalityReport,
    ) -> Result<(), DepositFailure> {
        self.admit_finality(report).map(|_| ())
    }
}

/// The finalized custody proof in the exact form consumed by
/// `lx_bridge_verify_deposit`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DepositProof {
    transaction: TransactionHash,
    inclusion: TransactionInclusion,
    confirmations: u64,
    required: u64,
    chain_id: u64,
    vault: EvmAddress,
    custody: CustodyDeposit,
    checkpoint: FinalizedCheckpoint,
    finalized: bool,
    commitment: [u8; 32],
}

impl DepositProof {
    #[must_use]
    pub const fn transaction(&self) -> TransactionHash {
        self.transaction
    }

    #[must_use]
    pub const fn inclusion(&self) -> TransactionInclusion {
        self.inclusion
    }

    #[must_use]
    pub const fn confirmations(&self) -> u64 {
        self.confirmations
    }

    #[must_use]
    pub const fn required(&self) -> u64 {
        self.required
    }

    #[must_use]
    pub const fn chain_id(&self) -> u64 {
        self.chain_id
    }

    #[must_use]
    pub const fn vault(&self) -> EvmAddress {
        self.vault
    }

    #[must_use]
    pub const fn custody(&self) -> CustodyDeposit {
        self.custody
    }

    #[must_use]
    pub const fn checkpoint(&self) -> &FinalizedCheckpoint {
        &self.checkpoint
    }

    #[must_use]
    pub const fn custody_reference(&self) -> [u8; 32] {
        self.transaction.bytes()
    }

    #[must_use]
    pub const fn commitment(&self) -> [u8; 32] {
        self.commitment
    }

    #[must_use]
    pub const fn finalized(&self) -> bool {
        self.finalized
    }

    #[must_use]
    pub const fn deposit_id(&self) -> DepositProofId {
        DepositProofId::new(self.custody.deposit_id)
    }

    /// Derives the one deterministic credit idempotency key this custody
    /// transaction can ever submit under.
    #[must_use]
    pub fn idempotency_key(&self) -> IdempotencyKey {
        let mut hasher = Sha256::new();
        hasher.update(CREDIT_KEY_DOMAIN);
        hasher.update(self.custody.deposit_id);
        IdempotencyKey::new(hasher.finalize().into())
    }

    /// Feeds this proof into the typed bridge deposit-credit intent.
    ///
    /// # Errors
    ///
    /// Refuses a reserve outside `system:paxeer-reserve`, a recipient that is
    /// not the custody beneficiary, and any intent-vocabulary rejection.
    pub fn credit_intent(
        &self,
        reserve: &AccountId,
        recipient: &AccountId,
    ) -> Result<Intent, DepositFailure> {
        if reserve.namespace() != AccountNamespace::SystemPaxeerReserve {
            return Err(DepositFailure::CreditRefused(CreditFault::ReserveNamespace));
        }
        let recipient_address = account_address(recipient);
        if recipient_address != self.custody.beneficiary {
            return Err(DepositFailure::CreditRefused(
                CreditFault::BeneficiaryMismatch {
                    beneficiary: self.custody.beneficiary,
                    recipient: recipient_address,
                },
            ));
        }
        let credit = BridgeDepositCredit::new(
            self.deposit_id(),
            self.checkpoint.id(),
            reserve.clone(),
            recipient.clone(),
            self.custody.asset,
            self.custody.amount,
            self.idempotency_key(),
        )
        .map_err(|error| DepositFailure::CreditRefused(CreditFault::Intent(error)))?;
        Ok(Intent::v1(IntentKind::BridgeDepositCredit(credit)))
    }

    /// Compiles the deposit-credit intent into the canonical payload admitted
    /// by the core-negotiated module registry.
    ///
    /// # Errors
    ///
    /// Returns a typed refusal for an invalid intent or undeclared payload.
    pub fn compile_credit(
        &self,
        reserve: &AccountId,
        recipient: &AccountId,
        registry: &ModuleRegistry,
    ) -> Result<CompiledIntent, DepositFailure> {
        let intent = self.credit_intent(reserve, recipient)?;
        compile(&intent, registry)
            .map_err(|error| DepositFailure::CreditRefused(CreditFault::Compile(error)))
    }

    /// Accepts a canonical credit receipt only when it verifies and binds to
    /// this deposit, the exact submitted activity, reserve, and recipient.
    ///
    /// # Errors
    ///
    /// Returns the precise verifier, result-code, activity, or transfer binding
    /// that refused the receipt.
    pub fn accept_credit(
        &self,
        receipt_bytes: &[u8],
        batch: &AuthorizedBatch,
        expected_activity_id: [u8; 32],
        reserve: &AccountId,
        recipient: &AccountId,
    ) -> Result<CreditReceipt, DepositFailure> {
        let verified = verify_outcome(receipt_bytes, batch)
            .map_err(|failure| DepositFailure::CreditRefused(CreditFault::Unverifiable(failure)))?;
        let (activity_id, result_code, asset, amount, from, to) =
            match verified.receipt().protocol() {
                Some(protocol) => (
                    protocol.activity_id(),
                    protocol.result_code(),
                    protocol.asset(),
                    protocol.amount(),
                    protocol.from(),
                    protocol.to(),
                ),
                None => {
                    return Err(DepositFailure::CreditRefused(CreditFault::Unverifiable(
                        VerificationFailure {
                            check: ReceiptCheck::ReceiptShape,
                        },
                    )));
                }
            };
        if activity_id != expected_activity_id {
            return Err(DepositFailure::CreditRefused(CreditFault::WrongActivity {
                expected: expected_activity_id,
                found: activity_id,
            }));
        }
        if result_code != 0 {
            return Err(DepositFailure::CreditRefused(CreditFault::Refused {
                result_code,
            }));
        }
        if asset != self.custody.asset.bytes() {
            return Err(DepositFailure::CreditRefused(CreditFault::WrongAsset {
                expected: self.custody.asset,
                found: asset,
            }));
        }
        if amount != self.custody.amount.value() {
            return Err(DepositFailure::CreditRefused(CreditFault::WrongAmount {
                expected: self.custody.amount,
                found: amount,
            }));
        }
        let reserve_address = account_address(reserve);
        if from != reserve_address {
            return Err(DepositFailure::CreditRefused(CreditFault::WrongReserve {
                expected: reserve_address,
                found: from,
            }));
        }
        let recipient_address = account_address(recipient);
        if recipient_address != self.custody.beneficiary || to != recipient_address {
            return Err(DepositFailure::CreditRefused(CreditFault::WrongRecipient {
                expected: self.custody.beneficiary,
                found: to,
            }));
        }
        Ok(CreditReceipt { verified })
    }
}

/// The verified credit receipt that completes one deposit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreditReceipt {
    verified: VerifiedReceipt,
}

impl CreditReceipt {
    #[must_use]
    pub const fn verified(&self) -> &VerifiedReceipt {
        &self.verified
    }

    #[must_use]
    pub fn activity_id(&self) -> [u8; 32] {
        match self.verified.receipt().protocol() {
            Some(receipt) => receipt.activity_id(),
            None => [0; 32],
        }
    }

    #[must_use]
    pub fn amount(&self) -> u128 {
        match self.verified.receipt().protocol() {
            Some(receipt) => receipt.amount(),
            None => 0,
        }
    }
}

/// Caller-owned fields that the Agent API requires in addition to the exact
/// compiled deposit-credit payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentCreditContext {
    pub request_id: RequestId,
    pub actor: AgentDid,
    pub authority: AuthorityRef,
    pub account_sequence: Sequence,
    pub timestamp_bound: AgentTimestampBound,
    pub fee_limit: AgentAmount,
}

/// A typed deposit-credit compilation handed to the stable Agent API contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreditPath {
    deposit: DepositProofId,
    idempotency_key: IdempotencyKey,
    compiled: CompiledIntent,
}

impl CreditPath {
    /// Creates the only canonical credit payload for this proof.
    ///
    /// # Errors
    ///
    /// Returns the same typed intent and compilation refusals as the proof.
    pub fn prepare(
        proof: &DepositProof,
        reserve: &AccountId,
        recipient: &AccountId,
        registry: &ModuleRegistry,
    ) -> Result<Self, DepositFailure> {
        let compiled = proof.compile_credit(reserve, recipient, registry)?;
        Ok(Self {
            deposit: proof.deposit_id(),
            idempotency_key: proof.idempotency_key(),
            compiled,
        })
    }

    #[must_use]
    pub const fn deposit(&self) -> DepositProofId {
        self.deposit
    }

    #[must_use]
    pub const fn idempotency_key(&self) -> IdempotencyKey {
        self.idempotency_key
    }

    #[must_use]
    pub const fn compiled(&self) -> &CompiledIntent {
        &self.compiled
    }

    /// Forms the exact idempotent Agent API prepare mutation for this credit.
    /// The human plane supplies identity, authority, sequence, time and fee;
    /// `layerx-intents` remains the sole payload encoding authority.
    ///
    /// # Errors
    ///
    /// Returns a typed Agent API contract refusal before any SDK call exists.
    pub fn agent_prepare(
        &self,
        context: AgentCreditContext,
    ) -> Result<IdempotentMutation<AgentPrepareRequest>, DepositFailure> {
        let key = self.idempotency_key.bytes();
        let idempotency = IdempotencyRef::new(hex(key)).map_err(credit_contract)?;
        let payload = PayloadBytes::new(self.compiled.payload().as_bytes().to_vec())
            .map_err(credit_contract)?;
        let timestamp_bound = context
            .timestamp_bound
            .validate()
            .map_err(credit_contract)?;
        let body_digest = BodyDigest(agent_body_digest(self, &context));
        Ok(IdempotentMutation {
            request_id: context.request_id,
            key: Key::new(key).map_err(credit_contract)?,
            body_digest,
            operation: AgentPrepareRequest {
                actor: context.actor,
                authority: context.authority,
                account_sequence: context.account_sequence,
                timestamp_bound,
                idempotency_key: idempotency,
                fee_limit: context.fee_limit,
                payload,
                payload_hash: self.compiled.payload_hash(),
            },
        })
    }

    /// Routes this credit through the typed SDK's daemon preparation operation.
    ///
    /// # Errors
    ///
    /// Returns the same typed contract refusal as [`Self::agent_prepare`].
    pub fn agent_call(
        &self,
        client: &AgentClient,
        context: AgentCreditContext,
    ) -> Result<Call<IdempotentMutation<AgentPrepareRequest>>, DepositFailure> {
        self.agent_prepare(context)
            .map(|request| client.prepare(request))
    }
}

fn credit_contract(error: ContractError) -> DepositFailure {
    DepositFailure::CreditRefused(CreditFault::AgentContract(error))
}

fn agent_body_digest(path: &CreditPath, context: &AgentCreditContext) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"LXP/human/agent-credit-request/v1\0");
    digest_field(&mut hasher, context.actor.as_str().as_bytes());
    digest_field(&mut hasher, context.authority.as_str().as_bytes());
    hasher.update(context.account_sequence.0.to_be_bytes());
    hasher.update(context.timestamp_bound.not_before.0.to_be_bytes());
    hasher.update(context.timestamp_bound.not_after.0.to_be_bytes());
    hasher.update(context.fee_limit.0.to_be_bytes());
    hasher.update(path.compiled.activity_type().value().to_be_bytes());
    hasher.update(path.idempotency_key.bytes());
    hasher.update(path.compiled.payload_hash());
    digest_field(&mut hasher, path.compiled.payload().as_bytes());
    hasher.finalize().into()
}

fn digest_field(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update(u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_be_bytes());
    hasher.update(bytes);
}

fn hex(bytes: [u8; 32]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(64);
    for byte in bytes {
        encoded.push(char::from(DIGITS[usize::from(byte >> 4)]));
        encoded.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn custody_event(logs: &[LogRecord], vault: EvmAddress) -> Result<CustodyDeposit, DepositFailure> {
    let mut found = None;
    for log in logs {
        if log.address != vault || log.topics.first() != Some(&CUSTODY_DEPOSIT_TOPIC) {
            continue;
        }
        if found.is_some() {
            return Err(DepositFailure::ProofUnavailable(
                ProofFault::MalformedCustodyEvent {
                    detail: "more than one custody deposit in one transaction".to_owned(),
                },
            ));
        }
        found = Some(log);
    }
    let Some(log) = found else {
        return Err(DepositFailure::ProofUnavailable(
            ProofFault::MissingCustodyEvent { vault },
        ));
    };
    let malformed = |detail: String| {
        DepositFailure::ProofUnavailable(ProofFault::MalformedCustodyEvent { detail })
    };
    if log.topics.len() != 4 {
        return Err(malformed(format!(
            "expected 4 topics, got {}",
            log.topics.len()
        )));
    }
    if log.data.len() != 96 {
        return Err(malformed(format!(
            "expected 96 data bytes, got {}",
            log.data.len()
        )));
    }
    let word = |index: usize| -> [u8; 32] {
        let mut bytes = [0_u8; 32];
        bytes.copy_from_slice(&log.data[index.saturating_mul(32)..(index + 1).saturating_mul(32)]);
        bytes
    };
    let deposit_id = log.topics[1];
    let asset = AssetId::new(log.topics[2]);
    let payer_word = log.topics[3];
    if payer_word[..12] != [0; 12] {
        return Err(malformed("payer topic is not an address".to_owned()));
    }
    let mut payer = [0_u8; 20];
    payer.copy_from_slice(&payer_word[12..]);
    let beneficiary = word(0);
    let amount_word = word(1);
    if amount_word[..16] != [0; 16] {
        return Err(DepositFailure::ProofUnavailable(
            ProofFault::AmountOverflow { beneficiary },
        ));
    }
    let mut amount_bytes = [0_u8; 16];
    amount_bytes.copy_from_slice(&amount_word[16..]);
    let nonce_word = word(2);
    if nonce_word[..24] != [0; 24] {
        return Err(malformed("nonce word is not a u64".to_owned()));
    }
    let mut nonce_bytes = [0_u8; 8];
    nonce_bytes.copy_from_slice(&nonce_word[24..]);
    Ok(CustodyDeposit {
        deposit_id,
        asset,
        payer: EvmAddress::new(payer),
        beneficiary,
        amount: Amount::from_be_bytes(amount_bytes),
        nonce: u64::from_be_bytes(nonce_bytes),
    })
}

fn word_u64(value: u64) -> [u8; 32] {
    let mut word = [0_u8; 32];
    word[24..].copy_from_slice(&value.to_be_bytes());
    word
}

fn word_address(address: EvmAddress) -> [u8; 32] {
    let mut word = [0_u8; 32];
    word[12..].copy_from_slice(&address.bytes());
    word
}

fn derive_deposit_id(chain_id: u64, vault: EvmAddress, custody: &CustodyDeposit) -> [u8; 32] {
    let mut amount_word = [0_u8; 32];
    amount_word[16..].copy_from_slice(&custody.amount.to_be_bytes());
    let mut domain_word = [0_u8; 32];
    domain_word[..CUSTODY_DEPOSIT_DOMAIN.len()].copy_from_slice(CUSTODY_DEPOSIT_DOMAIN);
    let mut hasher = Sha256::new();
    hasher.update(word_u64(0x100));
    hasher.update(word_u64(chain_id));
    hasher.update(word_address(vault));
    hasher.update(word_address(custody.payer));
    hasher.update(custody.asset.bytes());
    hasher.update(custody.beneficiary);
    hasher.update(amount_word);
    hasher.update(word_u64(custody.nonce));
    hasher.update(word_u64(
        u64::try_from(CUSTODY_DEPOSIT_DOMAIN.len()).unwrap_or(0),
    ));
    hasher.update(domain_word);
    hasher.finalize().into()
}

fn proof_commitment(
    transaction: TransactionHash,
    custody: &CustodyDeposit,
    checkpoint: &FinalizedCheckpoint,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(PROOF_COMMITMENT_DOMAIN);
    hasher.update(custody.deposit_id);
    hasher.update(transaction.bytes());
    hasher.update(custody.asset.bytes());
    hasher.update(custody.amount.to_be_bytes());
    hasher.update(checkpoint.id().bytes());
    hasher.update(checkpoint.state_root());
    hasher.update(checkpoint.network_id().to_be_bytes());
    hasher.update(checkpoint.protocol_version().to_be_bytes());
    hasher.update([1]);
    hasher.finalize().into()
}
