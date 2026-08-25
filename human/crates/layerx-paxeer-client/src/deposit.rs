use layerx_agent_api::error::RequestId;
use layerx_agent_api::idempotency::{BodyDigest, IdempotentMutation, Key};
use layerx_agent_api::identity::{AgentDid, AuthorityRef, ContractError};
use layerx_agent_api::prepare::{
    IdempotencyRef, PayloadBytes, PrepareRequest as AgentPrepareRequest,
    TimestampBound as AgentTimestampBound,
};
use layerx_agent_api::{Amount as AgentAmount, Sequence};
use ed25519_dalek::{Signature, VerifyingKey};
use layerx_intents::{CompiledIntent, Intent};
use layerx_proof::merkle::{leaf_hash, verify_leaf_hash, MerkleError, Proof};
use layerx_proof::receipt::AuthorizedBatch;
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
const DEPOSIT_ROOT_DOMAIN: &[u8] = b"LX:PAXEER:DEPOSIT:ROOT:v1";
const DEPOSIT_LEAF_DOMAIN: &[u8] = b"LX:PAXEER:DEPOSIT:LEAF:v1";
const DEPOSIT_NULLIFIER_DOMAIN: &[u8] = b"LX:DEPOSIT:NULLIFIER:v1";

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

/// The exact untrusted Paxeer custody-root registration published to Core.
/// Trust is conferred only by [`DepositProofVerifier::obtain`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DepositRootRegistration {
    pub checkpoint_id: [u8; 32],
    pub checkpoint_state_root: [u8; 32],
    pub deposit_root: [u8; 32],
    pub custody_reference: [u8; 32],
    pub network_id: u32,
    pub protocol_version: u16,
    pub signature: [u8; 64],
}

/// Untrusted index-aware inclusion material published beside a custody root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublishedDepositProof {
    pub registration: DepositRootRegistration,
    pub inclusion_proof: Proof,
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
    /// No authentic producer for the signed registration and inclusion path is
    /// configured at the Paxeer boundary.
    ProducerUnavailable,
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
    NonCanonicalRegistration {
        field: &'static str,
    },
    RegistrationNetworkMismatch {
        expected: u32,
        found: u32,
    },
    RegistrationProtocolMismatch {
        expected: u16,
        found: u16,
    },
    CustodyReferenceMismatch {
        expected: [u8; 32],
        found: [u8; 32],
    },
    InvalidDepositRootSignature,
    NonCanonicalDepositLeaf {
        field: &'static str,
    },
    DepositInclusion(MerkleError),
}

/// Why the credit submission or its receipt was refused.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CreditFault {
    BeneficiaryMismatch {
        beneficiary: [u8; 32],
        recipient: [u8; 32],
    },
    ReserveNamespace,
    /// Core exposes no activity ingress carrying the complete C custody proof.
    BridgeProofIngressUnavailable,
    AgentContract(ContractError),
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
    pub paxeer_checkpoint_authority: [u8; 32],
    pub custody_reference: [u8; 32],
    pub layerx_network_id: u32,
    pub layerx_protocol_version: u16,
}

/// Why a custody-proof authority configuration was refused.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DepositProofConfigError {
    Endpoints(ClientConfigError),
    Agreement(TrackerConfigError),
    ZeroRequiredConfirmations,
    InvalidCheckpointAuthority,
    ZeroCustodyReference,
    ZeroNetworkId,
    ZeroProtocolVersion,
}

/// The configured authority that can mint a [`DepositProof`].
///
/// A report supplies opaque quorum evidence, but never selects the quorum or
/// confirmation policy under which the proof is accepted.
#[derive(Clone, Debug)]
pub struct DepositProofVerifier {
    binding: QuorumBinding,
    required_confirmations: u64,
    checkpoint_authority: VerifyingKey,
    custody_reference: [u8; 32],
    layerx_network_id: u32,
    layerx_protocol_version: u16,
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
        if config.custody_reference == [0; 32] {
            return Err(DepositProofConfigError::ZeroCustodyReference);
        }
        if config.layerx_network_id == 0 {
            return Err(DepositProofConfigError::ZeroNetworkId);
        }
        if config.layerx_protocol_version == 0 {
            return Err(DepositProofConfigError::ZeroProtocolVersion);
        }
        let checkpoint_authority = VerifyingKey::from_bytes(&config.paxeer_checkpoint_authority)
            .map_err(|_| DepositProofConfigError::InvalidCheckpointAuthority)?;
        if checkpoint_authority.is_weak() {
            return Err(DepositProofConfigError::InvalidCheckpointAuthority);
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
            checkpoint_authority,
            custody_reference: config.custody_reference,
            layerx_network_id: config.layerx_network_id,
            layerx_protocol_version: config.layerx_protocol_version,
        })
    }

    /// Constructs a finalized custody proof from one tracked transaction and
    /// the exact signed Paxeer custody-root registration and index-aware path.
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
        published: PublishedDepositProof,
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
        self.verify_registration(&published.registration)?;
        let leaf_bytes = deposit_leaf_bytes(
            custody.deposit_id,
            published.registration.custody_reference,
            custody.asset,
            custody.amount,
            published.registration.checkpoint_id,
            published.registration.network_id,
            published.registration.protocol_version,
        )
        .map_err(DepositFailure::ProofUnavailable)?;
        let leaf = leaf_hash(&leaf_bytes).map_err(deposit_inclusion)?;
        verify_leaf_hash(
            &leaf,
            &published.inclusion_proof,
            &published.registration.deposit_root,
        )
        .map_err(deposit_inclusion)?;
        let nullifier = deposit_nullifier(custody.deposit_id);
        Ok(DepositProof {
            transaction: report.transaction(),
            inclusion: admitted.inclusion,
            confirmations: admitted.confirmations,
            required: self.required_confirmations,
            chain_id: admitted.chain_id,
            vault,
            custody,
            checkpoint_id: CheckpointId::new(published.registration.checkpoint_id),
            checkpoint_state_root: published.registration.checkpoint_state_root,
            deposit_root: published.registration.deposit_root,
            custody_reference: published.registration.custody_reference,
            network_id: published.registration.network_id,
            protocol_version: published.registration.protocol_version,
            registration_signature: published.registration.signature,
            inclusion_proof: published.inclusion_proof,
            leaf_hash: leaf,
            nullifier,
        })
    }

    fn verify_registration(
        &self,
        registration: &DepositRootRegistration,
    ) -> Result<(), DepositFailure> {
        if registration.network_id != self.layerx_network_id {
            return Err(DepositFailure::ProofUnavailable(
                ProofFault::RegistrationNetworkMismatch {
                    expected: self.layerx_network_id,
                    found: registration.network_id,
                },
            ));
        }
        if registration.protocol_version != self.layerx_protocol_version {
            return Err(DepositFailure::ProofUnavailable(
                ProofFault::RegistrationProtocolMismatch {
                    expected: self.layerx_protocol_version,
                    found: registration.protocol_version,
                },
            ));
        }
        if registration.custody_reference != self.custody_reference {
            return Err(DepositFailure::ProofUnavailable(
                ProofFault::CustodyReferenceMismatch {
                    expected: self.custody_reference,
                    found: registration.custody_reference,
                },
            ));
        }
        let message = deposit_root_registration_message(registration)
            .map_err(DepositFailure::ProofUnavailable)?;
        let signature = Signature::from_bytes(&registration.signature);
        self.checkpoint_authority
            .verify_strict(&message, &signature)
            .map_err(|_| {
                DepositFailure::ProofUnavailable(ProofFault::InvalidDepositRootSignature)
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
    checkpoint_id: CheckpointId,
    checkpoint_state_root: [u8; 32],
    deposit_root: [u8; 32],
    custody_reference: [u8; 32],
    network_id: u32,
    protocol_version: u16,
    registration_signature: [u8; 64],
    inclusion_proof: Proof,
    leaf_hash: [u8; 32],
    nullifier: [u8; 32],
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
    pub const fn checkpoint_id(&self) -> CheckpointId {
        self.checkpoint_id
    }

    #[must_use]
    pub const fn checkpoint_state_root(&self) -> [u8; 32] {
        self.checkpoint_state_root
    }

    #[must_use]
    pub const fn deposit_root(&self) -> [u8; 32] {
        self.deposit_root
    }

    #[must_use]
    pub const fn custody_reference(&self) -> [u8; 32] {
        self.custody_reference
    }

    #[must_use]
    pub const fn network_id(&self) -> u32 {
        self.network_id
    }

    #[must_use]
    pub const fn protocol_version(&self) -> u16 {
        self.protocol_version
    }

    #[must_use]
    pub const fn registration_signature(&self) -> [u8; 64] {
        self.registration_signature
    }

    #[must_use]
    pub const fn inclusion_proof(&self) -> &Proof {
        &self.inclusion_proof
    }

    #[must_use]
    pub const fn leaf_hash(&self) -> [u8; 32] {
        self.leaf_hash
    }

    #[must_use]
    pub const fn nullifier(&self) -> [u8; 32] {
        self.nullifier
    }

    #[must_use]
    pub const fn deposit_id(&self) -> DepositProofId {
        DepositProofId::new(self.custody.deposit_id)
    }

    /// Derives the one deterministic credit idempotency key this custody
    /// transaction can ever submit under.
    #[must_use]
    pub fn idempotency_key(&self) -> IdempotencyKey {
        IdempotencyKey::new(self.nullifier)
    }

    /// Feeds this proof into the typed bridge deposit-credit intent.
    ///
    /// # Errors
    ///
    /// Refuses a reserve outside `system:paxeer-reserve`, a recipient that is
    /// not the custody beneficiary, and the currently unavailable complete
    /// C proof ingress. The existing seven-field intent is not an equivalent
    /// substitute for `lx_deposit_proof`.
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
        Err(DepositFailure::CreditRefused(
            CreditFault::BridgeProofIngressUnavailable,
        ))
    }

    /// Compiles the deposit-credit intent into the canonical payload admitted
    /// by the core-negotiated module registry.
    ///
    /// # Errors
    ///
    /// Returns the typed complete-proof ingress refusal before compilation.
    pub fn compile_credit(
        &self,
        reserve: &AccountId,
        recipient: &AccountId,
        registry: &ModuleRegistry,
    ) -> Result<CompiledIntent, DepositFailure> {
        let _ = registry;
        self.credit_intent(reserve, recipient).and_then(|_| {
            Err(DepositFailure::CreditRefused(
                CreditFault::BridgeProofIngressUnavailable,
            ))
        })
    }

    /// Refuses receipt acceptance until Core exposes a complete-proof activity
    /// whose receipt can bind this deposit nullifier and exact submitted proof.
    ///
    /// # Errors
    ///
    /// Returns the same typed proof-ingress refusal as credit preparation after
    /// checking the reserve and beneficiary boundary.
    pub fn accept_credit(
        &self,
        receipt_bytes: &[u8],
        batch: &AuthorizedBatch,
        expected_activity_id: [u8; 32],
        reserve: &AccountId,
        recipient: &AccountId,
    ) -> Result<(), DepositFailure> {
        let _ = (receipt_bytes, batch, expected_activity_id);
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
        Err(DepositFailure::CreditRefused(
            CreditFault::BridgeProofIngressUnavailable,
        ))
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

/// Serializes the exact bytes signed by the Paxeer checkpoint authority for a
/// deposit-root registration.
///
/// # Errors
///
/// Refuses every zero field rejected by `lx_paxeer_deposit_root_message`.
pub fn deposit_root_registration_message(
    registration: &DepositRootRegistration,
) -> Result<Vec<u8>, ProofFault> {
    for (field, value) in [
        ("checkpoint_id", registration.checkpoint_id),
        ("checkpoint_state_root", registration.checkpoint_state_root),
        ("deposit_root", registration.deposit_root),
        ("custody_reference", registration.custody_reference),
    ] {
        if value == [0; 32] {
            return Err(ProofFault::NonCanonicalRegistration { field });
        }
    }
    if registration.network_id == 0 {
        return Err(ProofFault::NonCanonicalRegistration {
            field: "network_id",
        });
    }
    if registration.protocol_version == 0 {
        return Err(ProofFault::NonCanonicalRegistration {
            field: "protocol_version",
        });
    }
    let mut message = Vec::with_capacity(DEPOSIT_ROOT_DOMAIN.len() + 32 * 4 + 4 + 2);
    message.extend_from_slice(DEPOSIT_ROOT_DOMAIN);
    message.extend_from_slice(&registration.checkpoint_id);
    message.extend_from_slice(&registration.checkpoint_state_root);
    message.extend_from_slice(&registration.deposit_root);
    message.extend_from_slice(&registration.custody_reference);
    message.extend_from_slice(&registration.network_id.to_be_bytes());
    message.extend_from_slice(&registration.protocol_version.to_be_bytes());
    Ok(message)
}

/// Serializes one canonical C `lx_deposit_proof` leaf before Merkle leaf-domain
/// hashing.
///
/// # Errors
///
/// Refuses every zero economic or checkpoint field rejected by
/// `lx_paxeer_deposit_leaf_hash`.
#[allow(clippy::too_many_arguments)]
pub fn deposit_leaf_bytes(
    deposit_id: [u8; 32],
    custody_reference: [u8; 32],
    asset: AssetId,
    amount: Amount,
    checkpoint_id: [u8; 32],
    network_id: u32,
    protocol_version: u16,
) -> Result<Vec<u8>, ProofFault> {
    for (field, value) in [
        ("deposit_id", deposit_id),
        ("custody_reference", custody_reference),
        ("asset_id", asset.bytes()),
        ("checkpoint_id", checkpoint_id),
    ] {
        if value == [0; 32] {
            return Err(ProofFault::NonCanonicalDepositLeaf { field });
        }
    }
    if amount.value() == 0 {
        return Err(ProofFault::NonCanonicalDepositLeaf { field: "amount" });
    }
    if network_id == 0 {
        return Err(ProofFault::NonCanonicalDepositLeaf {
            field: "network_id",
        });
    }
    if protocol_version == 0 {
        return Err(ProofFault::NonCanonicalDepositLeaf {
            field: "protocol_version",
        });
    }
    let mut bytes = Vec::with_capacity(DEPOSIT_LEAF_DOMAIN.len() + 32 * 4 + 16 + 4 + 2);
    bytes.extend_from_slice(DEPOSIT_LEAF_DOMAIN);
    bytes.extend_from_slice(&deposit_id);
    bytes.extend_from_slice(&custody_reference);
    bytes.extend_from_slice(&asset.bytes());
    bytes.extend_from_slice(&amount.to_be_bytes());
    bytes.extend_from_slice(&checkpoint_id);
    bytes.extend_from_slice(&network_id.to_be_bytes());
    bytes.extend_from_slice(&protocol_version.to_be_bytes());
    Ok(bytes)
}

fn deposit_nullifier(deposit_id: [u8; 32]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(DEPOSIT_NULLIFIER_DOMAIN);
    hasher.update(deposit_id);
    hasher.finalize().into()
}

fn deposit_inclusion(error: MerkleError) -> DepositFailure {
    DepositFailure::ProofUnavailable(ProofFault::DepositInclusion(error))
}
