use ed25519_dalek::{Signature, VerifyingKey};
use layerx_agent_api::error::RequestId;
use layerx_agent_api::idempotency::{BodyDigest, IdempotentMutation, Key};
use layerx_agent_api::identity::{AgentDid, AuthorityRef, ContractError};
use layerx_agent_api::prepare::{
    IdempotencyRef, PayloadBytes, PrepareRequest as AgentPrepareRequest,
    TimestampBound as AgentTimestampBound,
};
use layerx_agent_api::{Amount as AgentAmount, Sequence};
use layerx_intents::{CompiledIntent, Intent};
use layerx_proof::merkle::{decode_proof, encode_proof};
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
const CUSTODY_DEPOSIT_DOMAIN_LENGTH: u64 = 29;
const _: [(); 29] = [(); CUSTODY_DEPOSIT_DOMAIN.len()];
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
    FinalityChainIdMismatch {
        expected: u64,
        found: u64,
    },
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

const DEPOSIT_FAILURE_VERSION: u8 = 1;
const DEPOSIT_FAILURE_MAX_BYTES: usize = 65_536;
const DEPOSIT_FAILURE_MAX_TEXT: usize = 4_096;
const DEPOSIT_FAILURE_MAX_ENDPOINTS: usize = 64;

impl DepositFailure {
    pub(crate) fn encode_failure_native(
        &self,
        maximum_bytes: usize,
    ) -> Result<Vec<u8>, DepositNativeError> {
        let mut out = Vec::new();
        out.push(DEPOSIT_FAILURE_VERSION);
        match self {
            Self::CustodyFailed(fault) => {
                out.push(0);
                encode_custody_fault(&mut out, fault)?;
            }
            Self::ProofUnavailable(fault) => {
                out.push(1);
                encode_proof_fault(&mut out, fault)?;
            }
            Self::CreditRefused(fault) => {
                out.push(2);
                encode_credit_fault(&mut out, fault)?;
            }
        }
        if out.len() > maximum_bytes || out.len() > DEPOSIT_FAILURE_MAX_BYTES {
            return Err(DepositNativeError::Limit);
        }
        Ok(out)
    }

    pub(crate) fn decode_failure_native(bytes: &[u8]) -> Result<Self, DepositNativeError> {
        if bytes.len() > DEPOSIT_FAILURE_MAX_BYTES {
            return Err(DepositNativeError::Limit);
        }
        let mut reader = DepositReader::new(bytes);
        if reader.u8()? != DEPOSIT_FAILURE_VERSION {
            return Err(DepositNativeError::Encoding);
        }
        let failure = match reader.u8()? {
            0 => Self::CustodyFailed(decode_custody_fault(&mut reader)?),
            1 => Self::ProofUnavailable(decode_proof_fault(&mut reader)?),
            2 => Self::CreditRefused(decode_credit_fault(&mut reader)?),
            _ => return Err(DepositNativeError::Encoding),
        };
        reader.finish()?;
        Ok(failure)
    }
}

fn put_text(out: &mut Vec<u8>, text: &str) -> Result<(), DepositNativeError> {
    if text.len() > DEPOSIT_FAILURE_MAX_TEXT {
        return Err(DepositNativeError::Limit);
    }
    let length = u16::try_from(text.len()).map_err(|_| DepositNativeError::Limit)?;
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(text.as_bytes());
    Ok(())
}

fn put_inclusion(out: &mut Vec<u8>, value: &TransactionInclusion) {
    out.extend_from_slice(&value.block.number.to_be_bytes());
    out.extend_from_slice(&value.block.hash);
    out.extend_from_slice(&value.transaction_index.to_be_bytes());
    out.push(match value.execution {
        ExecutionOutcome::Succeeded => 0,
        ExecutionOutcome::Reverted => 1,
    });
    match value.deployed_contract {
        None => out.push(0),
        Some(address) => {
            out.push(1);
            out.extend_from_slice(&address.bytes());
        }
    }
}

fn get_inclusion(
    reader: &mut DepositReader<'_>,
) -> Result<TransactionInclusion, DepositNativeError> {
    let block = BlockRef {
        number: reader.u64()?,
        hash: reader.array()?,
    };
    let transaction_index = reader.u64()?;
    let execution = match reader.u8()? {
        0 => ExecutionOutcome::Succeeded,
        1 => ExecutionOutcome::Reverted,
        _ => return Err(DepositNativeError::Encoding),
    };
    let deployed_contract = match reader.u8()? {
        0 => None,
        1 => Some(EvmAddress::new(reader.array()?)),
        _ => return Err(DepositNativeError::Encoding),
    };
    Ok(TransactionInclusion {
        block,
        transaction_index,
        execution,
        deployed_contract,
    })
}

fn encode_custody_fault(out: &mut Vec<u8>, fault: &CustodyFault) -> Result<(), DepositNativeError> {
    match fault {
        CustodyFault::Reverted { inclusion } => {
            out.push(0);
            put_inclusion(out, inclusion);
        }
        CustodyFault::Displaced {
            lost,
            head,
            requeued,
        } => {
            out.push(1);
            put_inclusion(out, lost);
            out.extend_from_slice(&head.to_be_bytes());
            out.push(u8::from(*requeued));
        }
    }
    Ok(())
}

fn decode_custody_fault(
    reader: &mut DepositReader<'_>,
) -> Result<CustodyFault, DepositNativeError> {
    match reader.u8()? {
        0 => Ok(CustodyFault::Reverted {
            inclusion: get_inclusion(reader)?,
        }),
        1 => {
            let lost = get_inclusion(reader)?;
            let head = reader.u64()?;
            let requeued = reader.boolean()?;
            Ok(CustodyFault::Displaced {
                lost,
                head,
                requeued,
            })
        }
        _ => Err(DepositNativeError::Encoding),
    }
}

fn put_block(out: &mut Vec<u8>, block: &BlockRef) {
    out.extend_from_slice(&block.number.to_be_bytes());
    out.extend_from_slice(&block.hash);
}
fn get_block(reader: &mut DepositReader<'_>) -> Result<BlockRef, DepositNativeError> {
    Ok(BlockRef {
        number: reader.u64()?,
        hash: reader.array()?,
    })
}

fn encode_stage(out: &mut Vec<u8>, stage: &FinalityStage) {
    match stage {
        FinalityStage::Announced => out.push(0),
        FinalityStage::Missing { head } => {
            out.push(1);
            out.extend_from_slice(&head.to_be_bytes());
        }
        FinalityStage::Pooled { head } => {
            out.push(2);
            out.extend_from_slice(&head.to_be_bytes());
        }
        FinalityStage::Confirming {
            inclusion,
            confirmations,
            required,
        } => {
            out.push(3);
            put_inclusion(out, inclusion);
            out.extend_from_slice(&confirmations.to_be_bytes());
            out.extend_from_slice(&required.to_be_bytes());
        }
        FinalityStage::Final {
            inclusion,
            confirmations,
            required,
        } => {
            out.push(4);
            put_inclusion(out, inclusion);
            out.extend_from_slice(&confirmations.to_be_bytes());
            out.extend_from_slice(&required.to_be_bytes());
        }
        FinalityStage::Displaced {
            lost,
            head,
            requeued,
        } => {
            out.push(5);
            put_inclusion(out, lost);
            out.extend_from_slice(&head.to_be_bytes());
            out.push(u8::from(*requeued));
        }
    }
}

fn decode_stage(reader: &mut DepositReader<'_>) -> Result<FinalityStage, DepositNativeError> {
    match reader.u8()? {
        0 => Ok(FinalityStage::Announced),
        1 => Ok(FinalityStage::Missing {
            head: reader.u64()?,
        }),
        2 => Ok(FinalityStage::Pooled {
            head: reader.u64()?,
        }),
        3 | 4 => {
            let tag = reader.bytes[reader.at - 1];
            let inclusion = get_inclusion(reader)?;
            let confirmations = reader.u64()?;
            let required = reader.u64()?;
            if tag == 3 {
                Ok(FinalityStage::Confirming {
                    inclusion,
                    confirmations,
                    required,
                })
            } else {
                Ok(FinalityStage::Final {
                    inclusion,
                    confirmations,
                    required,
                })
            }
        }
        5 => {
            let lost = get_inclusion(reader)?;
            let head = reader.u64()?;
            let requeued = reader.boolean()?;
            Ok(FinalityStage::Displaced {
                lost,
                head,
                requeued,
            })
        }
        _ => Err(DepositNativeError::Encoding),
    }
}

fn encode_endpoint_error(
    out: &mut Vec<u8>,
    error: &EndpointError,
) -> Result<(), DepositNativeError> {
    if error.failures.is_empty() {
        return Err(DepositNativeError::Encoding);
    }
    if error.failures.len() > DEPOSIT_FAILURE_MAX_ENDPOINTS {
        return Err(DepositNativeError::Limit);
    }
    out.push(u8::try_from(error.failures.len()).map_err(|_| DepositNativeError::Limit)?);
    for failure in &error.failures {
        put_text(out, &failure.url)?;
        encode_endpoint_fault(out, &failure.fault)?;
    }
    Ok(())
}

fn decode_endpoint_error(
    reader: &mut DepositReader<'_>,
) -> Result<EndpointError, DepositNativeError> {
    let count = usize::from(reader.u8()?);
    if count == 0 {
        return Err(DepositNativeError::Encoding);
    }
    if count > DEPOSIT_FAILURE_MAX_ENDPOINTS {
        return Err(DepositNativeError::Limit);
    }
    let mut failures = Vec::with_capacity(count);
    for _ in 0..count {
        failures.push(crate::rpc::EndpointFailure {
            url: reader.text()?,
            fault: decode_endpoint_fault(reader)?,
        });
    }
    Ok(EndpointError { failures })
}

fn encode_endpoint_fault(
    out: &mut Vec<u8>,
    fault: &crate::rpc::EndpointFault,
) -> Result<(), DepositNativeError> {
    use crate::rpc::EndpointFault;
    match fault {
        EndpointFault::UnsupportedUrl => out.push(0),
        EndpointFault::InsecureTransport => out.push(1),
        EndpointFault::InvalidTrustAnchor => out.push(2),
        EndpointFault::Authentication { detail } => {
            out.push(3);
            put_text(out, detail)?;
        }
        EndpointFault::Connect { detail } => {
            out.push(4);
            put_text(out, detail)?;
        }
        EndpointFault::Transport { detail } => {
            out.push(5);
            put_text(out, detail)?;
        }
        EndpointFault::Http { status } => {
            out.push(6);
            out.extend_from_slice(&status.to_be_bytes());
        }
        EndpointFault::ResponseTooLarge => out.push(7),
        EndpointFault::AmbiguousFraming => out.push(8),
        EndpointFault::MalformedResponse => out.push(9),
        EndpointFault::Rpc { code, message } => {
            out.push(10);
            out.extend_from_slice(&code.to_be_bytes());
            put_text(out, message)?;
        }
        EndpointFault::ChainMismatch { expected, actual } => {
            out.push(11);
            out.extend_from_slice(&expected.to_be_bytes());
            out.extend_from_slice(&actual.to_be_bytes());
        }
        EndpointFault::InconsistentObservation => out.push(12),
        EndpointFault::UnexpectedValue { detail } => {
            out.push(13);
            put_text(out, detail)?;
        }
    }
    Ok(())
}

fn decode_endpoint_fault(
    reader: &mut DepositReader<'_>,
) -> Result<crate::rpc::EndpointFault, DepositNativeError> {
    use crate::rpc::EndpointFault;
    match reader.u8()? {
        0 => Ok(EndpointFault::UnsupportedUrl),
        1 => Ok(EndpointFault::InsecureTransport),
        2 => Ok(EndpointFault::InvalidTrustAnchor),
        3 => Ok(EndpointFault::Authentication {
            detail: reader.text()?,
        }),
        4 => Ok(EndpointFault::Connect {
            detail: reader.text()?,
        }),
        5 => Ok(EndpointFault::Transport {
            detail: reader.text()?,
        }),
        6 => Ok(EndpointFault::Http {
            status: reader.u16()?,
        }),
        7 => Ok(EndpointFault::ResponseTooLarge),
        8 => Ok(EndpointFault::AmbiguousFraming),
        9 => Ok(EndpointFault::MalformedResponse),
        10 => Ok(EndpointFault::Rpc {
            code: reader.i64()?,
            message: reader.text()?,
        }),
        11 => Ok(EndpointFault::ChainMismatch {
            expected: reader.u64()?,
            actual: reader.u64()?,
        }),
        12 => Ok(EndpointFault::InconsistentObservation),
        13 => Ok(EndpointFault::UnexpectedValue {
            detail: reader.text()?,
        }),
        _ => Err(DepositNativeError::Encoding),
    }
}

fn encode_merkle(out: &mut Vec<u8>, error: &MerkleError) -> Result<(), DepositNativeError> {
    match error {
        MerkleError::Encoding => out.push(0),
        MerkleError::EmptyTree => out.push(1),
        MerkleError::LeafIndex { index, count } => {
            out.push(2);
            out.extend_from_slice(&index.to_be_bytes());
            out.extend_from_slice(&count.to_be_bytes());
        }
        MerkleError::PathLength { expected, actual } => {
            out.push(3);
            out.extend_from_slice(
                &u64::try_from(*expected)
                    .map_err(|_| DepositNativeError::Limit)?
                    .to_be_bytes(),
            );
            out.extend_from_slice(
                &u64::try_from(*actual)
                    .map_err(|_| DepositNativeError::Limit)?
                    .to_be_bytes(),
            );
        }
        MerkleError::PromotionSibling { level } => {
            out.push(4);
            out.extend_from_slice(
                &u64::try_from(*level)
                    .map_err(|_| DepositNativeError::Limit)?
                    .to_be_bytes(),
            );
        }
        MerkleError::RootMismatch => out.push(5),
        MerkleError::TreeTooLarge => out.push(6),
        MerkleError::Hash => out.push(7),
    }
    Ok(())
}

fn decode_merkle(reader: &mut DepositReader<'_>) -> Result<MerkleError, DepositNativeError> {
    match reader.u8()? {
        0 => Ok(MerkleError::Encoding),
        1 => Ok(MerkleError::EmptyTree),
        2 => Ok(MerkleError::LeafIndex {
            index: reader.u32()?,
            count: reader.u32()?,
        }),
        3 => Ok(MerkleError::PathLength {
            expected: usize::try_from(reader.u64()?).map_err(|_| DepositNativeError::Limit)?,
            actual: usize::try_from(reader.u64()?).map_err(|_| DepositNativeError::Limit)?,
        }),
        4 => Ok(MerkleError::PromotionSibling {
            level: usize::try_from(reader.u64()?).map_err(|_| DepositNativeError::Limit)?,
        }),
        5 => Ok(MerkleError::RootMismatch),
        6 => Ok(MerkleError::TreeTooLarge),
        7 => Ok(MerkleError::Hash),
        _ => Err(DepositNativeError::Encoding),
    }
}

fn put_static_field(out: &mut Vec<u8>, field: &'static str) -> Result<(), DepositNativeError> {
    put_text(out, field)
}

fn decode_static_field(reader: &mut DepositReader<'_>) -> Result<&'static str, DepositNativeError> {
    let value = reader.text()?;
    match value.as_str() {
        "tenant" => Ok("tenant"),
        "agent_did" => Ok("agent_did"),
        "authority_ref" => Ok("authority_ref"),
        "client" => Ok("client"),
        "policy_version" => Ok("policy_version"),
        "session_id" => Ok("session_id"),
        "capability_id" => Ok("capability_id"),
        "budget_id" => Ok("budget_id"),
        "counterparty" => Ok("counterparty"),
        "asset" => Ok("asset"),
        "purpose" => Ok("purpose"),
        "protocol_authority" => Ok("protocol_authority"),
        "idempotency_key" => Ok("idempotency_key"),
        "reason" => Ok("reason"),
        "availability_selector" => Ok("availability_selector"),
        "fact_set" => Ok("fact_set"),
        "offline_evidence" => Ok("offline_evidence"),
        "projection_rationale" => Ok("projection_rationale"),
        "event_bytes" => Ok("event_bytes"),
        "filter_account_outside_tenant" => Ok("filter_account_outside_tenant"),
        "filter_agent_outside_tenant" => Ok("filter_agent_outside_tenant"),
        "filter_asset_outside_tenant" => Ok("filter_asset_outside_tenant"),
        "filter_counterparty_outside_tenant" => Ok("filter_counterparty_outside_tenant"),
        "filter_module_outside_tenant" => Ok("filter_module_outside_tenant"),
        "transition_cause" => Ok("transition_cause"),
        "expiry" => Ok("expiry"),
        "limit" => Ok("limit"),
        "availability_bound" => Ok("availability_bound"),
        "history_range" => Ok("history_range"),
        "page_limit" => Ok("page_limit"),
        "gap_range" => Ok("gap_range"),
        "timestamp_bound" => Ok("timestamp_bound"),
        "checkpoint_id" => Ok("checkpoint_id"),
        "checkpoint_state_root" => Ok("checkpoint_state_root"),
        "deposit_root" => Ok("deposit_root"),
        "custody_reference" => Ok("custody_reference"),
        "network_id" => Ok("network_id"),
        "protocol_version" => Ok("protocol_version"),
        "deposit_id" => Ok("deposit_id"),
        "asset_id" => Ok("asset_id"),
        "amount" => Ok("amount"),
        _ => Err(DepositNativeError::Encoding),
    }
}

fn encode_proof_fault(out: &mut Vec<u8>, fault: &ProofFault) -> Result<(), DepositNativeError> {
    match fault {
        ProofFault::ProducerUnavailable => out.push(0),
        ProofFault::NotFinal { stage } => {
            out.push(1);
            encode_stage(out, stage);
        }
        ProofFault::Unreadable { error } => {
            out.push(2);
            encode_endpoint_error(out, error)?;
        }
        ProofFault::InclusionChanged { tracked, observed } => {
            out.push(3);
            put_block(out, tracked);
            match observed {
                None => out.push(0),
                Some(block) => {
                    out.push(1);
                    put_block(out, block);
                }
            }
        }
        ProofFault::MissingQuorumEvidence => out.push(4),
        ProofFault::EvidenceSourceMismatch => out.push(5),
        ProofFault::FinalityChainIdMismatch { expected, found } => {
            out.push(6);
            out.extend_from_slice(&expected.to_be_bytes());
            out.extend_from_slice(&found.to_be_bytes());
        }
        ProofFault::ConfirmationPolicyMismatch { expected, reported } => {
            out.push(7);
            out.extend_from_slice(&expected.to_be_bytes());
            out.extend_from_slice(&reported.to_be_bytes());
        }
        ProofFault::ConfirmationEvidenceMismatch { reported, observed } => {
            out.push(8);
            out.extend_from_slice(&reported.to_be_bytes());
            out.extend_from_slice(&observed.to_be_bytes());
        }
        ProofFault::MissingCustodyEvent { vault } => {
            out.push(9);
            out.extend_from_slice(&vault.bytes());
        }
        ProofFault::MalformedCustodyEvent { detail } => {
            out.push(10);
            put_text(out, detail)?;
        }
        ProofFault::AmountOverflow { beneficiary } => {
            out.push(11);
            out.extend_from_slice(beneficiary);
        }
        ProofFault::UnboundDeposit { emitted, derived } => {
            out.push(12);
            out.extend_from_slice(emitted);
            out.extend_from_slice(derived);
        }
        ProofFault::NonCanonicalRegistration { field } => {
            out.push(13);
            put_static_field(out, field)?;
        }
        ProofFault::RegistrationNetworkMismatch { expected, found } => {
            out.push(14);
            out.extend_from_slice(&expected.to_be_bytes());
            out.extend_from_slice(&found.to_be_bytes());
        }
        ProofFault::RegistrationProtocolMismatch { expected, found } => {
            out.push(15);
            out.extend_from_slice(&expected.to_be_bytes());
            out.extend_from_slice(&found.to_be_bytes());
        }
        ProofFault::CustodyReferenceMismatch { expected, found } => {
            out.push(16);
            out.extend_from_slice(expected);
            out.extend_from_slice(found);
        }
        ProofFault::InvalidDepositRootSignature => out.push(17),
        ProofFault::NonCanonicalDepositLeaf { field } => {
            out.push(18);
            put_static_field(out, field)?;
        }
        ProofFault::DepositInclusion(error) => {
            out.push(19);
            encode_merkle(out, error)?;
        }
    }
    Ok(())
}

fn decode_proof_fault(reader: &mut DepositReader<'_>) -> Result<ProofFault, DepositNativeError> {
    match reader.u8()? {
        0 => Ok(ProofFault::ProducerUnavailable),
        1 => Ok(ProofFault::NotFinal {
            stage: decode_stage(reader)?,
        }),
        2 => Ok(ProofFault::Unreadable {
            error: decode_endpoint_error(reader)?,
        }),
        3 => {
            let tracked = get_block(reader)?;
            let observed = match reader.u8()? {
                0 => None,
                1 => Some(get_block(reader)?),
                _ => return Err(DepositNativeError::Encoding),
            };
            Ok(ProofFault::InclusionChanged { tracked, observed })
        }
        4 => Ok(ProofFault::MissingQuorumEvidence),
        5 => Ok(ProofFault::EvidenceSourceMismatch),
        6 => Ok(ProofFault::FinalityChainIdMismatch {
            expected: reader.u64()?,
            found: reader.u64()?,
        }),
        7 => Ok(ProofFault::ConfirmationPolicyMismatch {
            expected: reader.u64()?,
            reported: reader.u64()?,
        }),
        8 => Ok(ProofFault::ConfirmationEvidenceMismatch {
            reported: reader.u64()?,
            observed: reader.u64()?,
        }),
        9 => Ok(ProofFault::MissingCustodyEvent {
            vault: EvmAddress::new(reader.array()?),
        }),
        10 => Ok(ProofFault::MalformedCustodyEvent {
            detail: reader.text()?,
        }),
        11 => Ok(ProofFault::AmountOverflow {
            beneficiary: reader.array()?,
        }),
        12 => Ok(ProofFault::UnboundDeposit {
            emitted: reader.array()?,
            derived: reader.array()?,
        }),
        13 => Ok(ProofFault::NonCanonicalRegistration {
            field: decode_static_field(reader)?,
        }),
        14 => Ok(ProofFault::RegistrationNetworkMismatch {
            expected: reader.u32()?,
            found: reader.u32()?,
        }),
        15 => Ok(ProofFault::RegistrationProtocolMismatch {
            expected: reader.u16()?,
            found: reader.u16()?,
        }),
        16 => Ok(ProofFault::CustodyReferenceMismatch {
            expected: reader.array()?,
            found: reader.array()?,
        }),
        17 => Ok(ProofFault::InvalidDepositRootSignature),
        18 => Ok(ProofFault::NonCanonicalDepositLeaf {
            field: decode_static_field(reader)?,
        }),
        19 => Ok(ProofFault::DepositInclusion(decode_merkle(reader)?)),
        _ => Err(DepositNativeError::Encoding),
    }
}

fn encode_credit_fault(out: &mut Vec<u8>, fault: &CreditFault) -> Result<(), DepositNativeError> {
    match fault {
        CreditFault::BeneficiaryMismatch {
            beneficiary,
            recipient,
        } => {
            out.push(0);
            out.extend_from_slice(beneficiary);
            out.extend_from_slice(recipient);
        }
        CreditFault::ReserveNamespace => out.push(1),
        CreditFault::BridgeProofIngressUnavailable => out.push(2),
        CreditFault::AgentContract(error) => {
            out.push(3);
            match error {
                ContractError::Empty(field) => {
                    out.push(0);
                    put_static_field(out, field)?;
                }
                ContractError::Zero(field) => {
                    out.push(1);
                    put_static_field(out, field)?;
                }
                ContractError::DaemonLimitFunding => out.push(2),
            }
        }
    }
    Ok(())
}

fn decode_credit_fault(reader: &mut DepositReader<'_>) -> Result<CreditFault, DepositNativeError> {
    match reader.u8()? {
        0 => Ok(CreditFault::BeneficiaryMismatch {
            beneficiary: reader.array()?,
            recipient: reader.array()?,
        }),
        1 => Ok(CreditFault::ReserveNamespace),
        2 => Ok(CreditFault::BridgeProofIngressUnavailable),
        3 => Ok(CreditFault::AgentContract(match reader.u8()? {
            0 => ContractError::Empty(decode_static_field(reader)?),
            1 => ContractError::Zero(decode_static_field(reader)?),
            2 => ContractError::DaemonLimitFunding,
            _ => return Err(DepositNativeError::Encoding),
        })),
        _ => Err(DepositNativeError::Encoding),
    }
}

/// The exact Paxeer chain, quorum, and confirmation policy permitted to mint
/// a custody proof.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DepositProofConfig {
    pub endpoints: Vec<EndpointConfig>,
    pub minimum_endpoint_agreement: usize,
    pub required_confirmations: u64,
    pub paxeer_chain_id: u64,
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
    ZeroPaxeerChainId,
    EndpointChainIdMismatch { expected: u64, found: u64 },
    InvalidCheckpointAuthority,
    ZeroCustodyReference,
    ZeroNetworkId,
    ZeroProtocolVersion,
    UnsupportedProtocolVersion,
}

/// The configured authority that can mint a [`DepositProof`].
///
/// A report supplies opaque quorum evidence, but never selects the quorum or
/// confirmation policy under which the proof is accepted.
#[derive(Clone, Debug)]
pub struct DepositProofVerifier {
    binding: QuorumBinding,
    required_confirmations: u64,
    paxeer_chain_id: u64,
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
    /// Validates and owns the exact Paxeer chain, endpoint quorum, and
    /// confirmation depth under which deposit proofs may be minted.
    ///
    /// # Errors
    ///
    /// Refuses zero chain or confirmation values, an invalid endpoint
    /// agreement, or an endpoint declared for another chain.
    pub fn new(config: DepositProofConfig) -> Result<Self, DepositProofConfigError> {
        if config.required_confirmations == 0 {
            return Err(DepositProofConfigError::ZeroRequiredConfirmations);
        }
        if config.paxeer_chain_id == 0 {
            return Err(DepositProofConfigError::ZeroPaxeerChainId);
        }
        if let Some(endpoint) = config
            .endpoints
            .iter()
            .find(|endpoint| endpoint.expected_chain_id != config.paxeer_chain_id)
        {
            return Err(DepositProofConfigError::EndpointChainIdMismatch {
                expected: config.paxeer_chain_id,
                found: endpoint.expected_chain_id,
            });
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
        if config.layerx_protocol_version != layerx_wire::limits::PROTOCOL_VERSION {
            return Err(DepositProofConfigError::UnsupportedProtocolVersion);
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
        let client =
            PaxeerClient::new(config.endpoints).map_err(DepositProofConfigError::Endpoints)?;
        let binding = client.quorum_binding(config.minimum_endpoint_agreement);
        Ok(Self {
            binding,
            required_confirmations: config.required_confirmations,
            paxeer_chain_id: config.paxeer_chain_id,
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
            .map_err(|_| DepositFailure::ProofUnavailable(ProofFault::InvalidDepositRootSignature))
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
        if evidence.chain_id() != self.paxeer_chain_id {
            return Err(DepositFailure::ProofUnavailable(
                ProofFault::FinalityChainIdMismatch {
                    expected: self.paxeer_chain_id,
                    found: evidence.chain_id(),
                },
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
        let logs = evidence
            .receipt_logs()
            .ok_or(DepositFailure::ProofUnavailable(
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
    pub(crate) fn encode_native(
        &self,
        maximum_bytes: usize,
    ) -> Result<Vec<u8>, DepositNativeError> {
        const FIXED_BYTES: usize = 486;
        if maximum_bytes == 0 {
            return Err(DepositNativeError::Limit);
        }
        let merkle = encode_proof(&self.inclusion_proof);
        let length = u16::try_from(merkle.len()).map_err(|_| DepositNativeError::Limit)?;
        let capacity = FIXED_BYTES
            .checked_add(merkle.len())
            .ok_or(DepositNativeError::Limit)?;
        if capacity > maximum_bytes || capacity > DEPOSIT_NATIVE_PAYLOAD_MAX {
            return Err(DepositNativeError::Limit);
        }
        let mut out = Vec::with_capacity(capacity);
        out.extend_from_slice(&self.transaction.bytes());
        out.extend_from_slice(&self.inclusion.block.number.to_be_bytes());
        out.extend_from_slice(&self.inclusion.block.hash);
        out.extend_from_slice(&self.inclusion.transaction_index.to_be_bytes());
        out.push(match self.inclusion.execution {
            ExecutionOutcome::Succeeded => 1,
            ExecutionOutcome::Reverted => 0,
        });
        match self.inclusion.deployed_contract {
            Some(address) => {
                out.push(1);
                out.extend_from_slice(&address.bytes());
            }
            None => {
                out.push(0);
                out.extend_from_slice(&[0; 20]);
            }
        }
        out.extend_from_slice(&self.confirmations.to_be_bytes());
        out.extend_from_slice(&self.required.to_be_bytes());
        out.extend_from_slice(&self.chain_id.to_be_bytes());
        out.extend_from_slice(&self.vault.bytes());
        out.extend_from_slice(&self.custody.deposit_id);
        out.extend_from_slice(&self.custody.asset.bytes());
        out.extend_from_slice(&self.custody.payer.bytes());
        out.extend_from_slice(&self.custody.beneficiary);
        out.extend_from_slice(&self.custody.amount.to_be_bytes());
        out.extend_from_slice(&self.custody.nonce.to_be_bytes());
        out.extend_from_slice(&self.checkpoint_id.bytes());
        out.extend_from_slice(&self.checkpoint_state_root);
        out.extend_from_slice(&self.deposit_root);
        out.extend_from_slice(&self.custody_reference);
        out.extend_from_slice(&self.network_id.to_be_bytes());
        out.extend_from_slice(&self.protocol_version.to_be_bytes());
        out.extend_from_slice(&self.registration_signature);
        out.extend_from_slice(&length.to_be_bytes());
        out.extend_from_slice(&merkle);
        Ok(out)
    }

    pub(crate) fn decode_native(bytes: &[u8]) -> Result<Self, DepositNativeError> {
        const FIXED_BYTES: usize = 486;
        if bytes.len() < FIXED_BYTES || bytes.len() > DEPOSIT_NATIVE_PAYLOAD_MAX {
            return Err(DepositNativeError::Encoding);
        }
        let mut reader = DepositReader::new(bytes);
        let transaction = TransactionHash::new(reader.array()?);
        let block = BlockRef {
            number: reader.u64()?,
            hash: reader.array()?,
        };
        let transaction_index = reader.u64()?;
        let execution = match reader.u8()? {
            1 => ExecutionOutcome::Succeeded,
            0 => ExecutionOutcome::Reverted,
            _ => return Err(DepositNativeError::Encoding),
        };
        let deployed = reader.u8()?;
        let deployed_bytes = reader.array()?;
        let deployed_contract = match deployed {
            0 if deployed_bytes == [0; 20] => None,
            1 => Some(EvmAddress::new(deployed_bytes)),
            _ => return Err(DepositNativeError::Encoding),
        };
        let confirmations = reader.u64()?;
        let required = reader.u64()?;
        let chain_id = reader.u64()?;
        let vault = EvmAddress::new(reader.array()?);
        let custody = CustodyDeposit {
            deposit_id: reader.array()?,
            asset: AssetId::new(reader.array()?),
            payer: EvmAddress::new(reader.array()?),
            beneficiary: reader.array()?,
            amount: Amount::from_be_bytes(reader.array()?),
            nonce: reader.u64()?,
        };
        let checkpoint_id = CheckpointId::new(reader.array()?);
        let checkpoint_state_root = reader.array()?;
        let deposit_root = reader.array()?;
        let custody_reference = reader.array()?;
        let network_id = reader.u32()?;
        let protocol_version = reader.u16()?;
        let registration_signature = reader.array()?;
        let proof_length = usize::from(reader.u16()?);
        if reader.remaining() != proof_length {
            return Err(DepositNativeError::Encoding);
        }
        let inclusion_proof =
            decode_proof(reader.take(proof_length)?).map_err(DepositNativeError::Merkle)?;
        reader.finish()?;
        if execution == ExecutionOutcome::Reverted {
            return Err(DepositNativeError::Custody(CustodyFault::Reverted {
                inclusion: TransactionInclusion {
                    block,
                    transaction_index,
                    execution,
                    deployed_contract,
                },
            }));
        }
        if required == 0 || confirmations < required || chain_id == 0 {
            return Err(DepositNativeError::Encoding);
        }
        let derived = derive_deposit_id(chain_id, vault, &custody);
        if derived != custody.deposit_id {
            return Err(DepositNativeError::Proof(ProofFault::UnboundDeposit {
                emitted: custody.deposit_id,
                derived,
            }));
        }
        let registration = DepositRootRegistration {
            checkpoint_id: checkpoint_id.bytes(),
            checkpoint_state_root,
            deposit_root,
            custody_reference,
            network_id,
            protocol_version,
            signature: registration_signature,
        };
        deposit_root_registration_message(&registration).map_err(DepositNativeError::Proof)?;
        let leaf_bytes = deposit_leaf_bytes(
            custody.deposit_id,
            custody_reference,
            custody.asset,
            custody.amount,
            checkpoint_id.bytes(),
            network_id,
            protocol_version,
        )
        .map_err(DepositNativeError::Proof)?;
        let computed_leaf = leaf_hash(&leaf_bytes).map_err(DepositNativeError::Merkle)?;
        verify_leaf_hash(&computed_leaf, &inclusion_proof, &deposit_root)
            .map_err(DepositNativeError::Merkle)?;
        let nullifier = deposit_nullifier(custody.deposit_id);
        Ok(Self {
            transaction,
            inclusion: TransactionInclusion {
                block,
                transaction_index,
                execution,
                deployed_contract,
            },
            confirmations,
            required,
            chain_id,
            vault,
            custody,
            checkpoint_id,
            checkpoint_state_root,
            deposit_root,
            custody_reference,
            network_id,
            protocol_version,
            registration_signature,
            inclusion_proof,
            leaf_hash: computed_leaf,
            nullifier,
        })
    }

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DepositNativeError {
    Encoding,
    Limit,
    Custody(CustodyFault),
    Proof(ProofFault),
    Merkle(MerkleError),
}

pub(crate) const DEPOSIT_NATIVE_PAYLOAD_MAX: usize =
    486 + 10 + 32 * layerx_proof::merkle::MAX_DEPTH;

struct DepositReader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> DepositReader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }
    fn take(&mut self, count: usize) -> Result<&'a [u8], DepositNativeError> {
        let end = self
            .at
            .checked_add(count)
            .ok_or(DepositNativeError::Encoding)?;
        let value = self
            .bytes
            .get(self.at..end)
            .ok_or(DepositNativeError::Encoding)?;
        self.at = end;
        Ok(value)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], DepositNativeError> {
        self.take(N)?
            .try_into()
            .map_err(|_| DepositNativeError::Encoding)
    }
    fn u8(&mut self) -> Result<u8, DepositNativeError> {
        Ok(self.array::<1>()?[0])
    }
    fn u16(&mut self) -> Result<u16, DepositNativeError> {
        Ok(u16::from_be_bytes(self.array()?))
    }
    fn u32(&mut self) -> Result<u32, DepositNativeError> {
        Ok(u32::from_be_bytes(self.array()?))
    }
    fn u64(&mut self) -> Result<u64, DepositNativeError> {
        Ok(u64::from_be_bytes(self.array()?))
    }
    fn i64(&mut self) -> Result<i64, DepositNativeError> {
        Ok(i64::from_be_bytes(self.array()?))
    }
    fn boolean(&mut self) -> Result<bool, DepositNativeError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(DepositNativeError::Encoding),
        }
    }
    fn text(&mut self) -> Result<String, DepositNativeError> {
        let length = usize::from(self.u16()?);
        if length > DEPOSIT_FAILURE_MAX_TEXT {
            return Err(DepositNativeError::Limit);
        }
        String::from_utf8(self.take(length)?.to_vec()).map_err(|_| DepositNativeError::Encoding)
    }
    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.at)
    }
    fn finish(self) -> Result<(), DepositNativeError> {
        if self.at == self.bytes.len() {
            Ok(())
        } else {
            Err(DepositNativeError::Encoding)
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
                protocol_activity_type: self.compiled.activity_type().value(),
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
    hasher.update(word_u64(CUSTODY_DEPOSIT_DOMAIN_LENGTH));
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
