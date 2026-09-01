use std::time::Duration;

use layerx_proof::receipt::{
    verify_outcome, AuthorizedBatch, ReceiptCheck, VerificationFailure, VerifiedReceipt,
};
use layerx_types::intent::EvmAddress;
use sha2::{Digest as _, Sha256};

use crate::client::{
    BlockRef, ClientConfigError, EndpointError, ExecutionOutcome, PaxeerClient, TransactionHash,
    TransactionInclusion,
};
use crate::finality::{
    FinalityReport, FinalityStage, FinalityTracker, TrackerConfig, TrackerConfigError,
};
use crate::json::Json;
use crate::rpc::EndpointConfig;

const WORD: usize = 32;
const MAX_PROOF_DEPTH: usize = 256;
const ATTESTATION_WORDS: usize = 18;
const ALL_AVAILABILITY_CLASSES: u8 = 0x1f;

const WITHDRAWAL_DOMAIN: &[u8] = b"LX:WITHDRAWAL:v1";
const MERKLE_LEAF_DOMAIN: &[u8] = b"LXP/v1/merkle-leaf\0";
const MERKLE_NODE_DOMAIN: &[u8] = b"LXP/v1/merkle-node\0";

const SELECTOR_QUEUE_CLAIM: [u8; 4] = [0x0d, 0x79, 0x92, 0xc8];
const SELECTOR_FINALISE_CLAIM: [u8; 4] = [0x38, 0x51, 0xa8, 0x61];
const SELECTOR_CANCEL_CLAIM: [u8; 4] = [0x52, 0xc1, 0xd8, 0x2c];
const SELECTOR_CLAIM: [u8; 4] = [0xbd, 0x66, 0x52, 0x8a];
const SELECTOR_CHALLENGE: [u8; 4] = [0xcf, 0xfd, 0x46, 0xdc];
const SELECTOR_REGISTRY: [u8; 4] = [0x7b, 0x10, 0x39, 0x99];
const SELECTOR_NETWORK_ID: [u8; 4] = [0x90, 0x25, 0xe6, 0x4c];
const SELECTOR_CHALLENGE_MANAGER: [u8; 4] = [0x02, 0x3a, 0x96, 0xfe];
const SELECTOR_VAULT: [u8; 4] = [0xfb, 0xfa, 0x77, 0xcf];
const SELECTOR_ASSET_REGISTRY: [u8; 4] = [0x97, 0x9d, 0x7e, 0x86];
const SELECTOR_ASSET: [u8; 4] = [0x85, 0x39, 0xcc, 0xf4];
const SELECTOR_BALANCE_OF: [u8; 4] = [0x70, 0xa0, 0x82, 0x31];
const SELECTOR_WITHDRAWAL_LEAF: [u8; 4] = [0x54, 0x00, 0x16, 0x0a];
const SELECTOR_WITHDRAWAL_NULLIFIER: [u8; 4] = [0x3e, 0x51, 0xd1, 0x52];
const SELECTOR_FINALISED_STATE_ROOT: [u8; 4] = [0xdf, 0xd0, 0x60, 0x94];
const SELECTOR_RECORDED_CERTIFICATE: [u8; 4] = [0x05, 0xd9, 0x4f, 0x01];
const SELECTOR_GUARANTOR_ELIGIBILITY: [u8; 4] = [0x9e, 0x2b, 0x11, 0xf1];
const SELECTOR_PROTOCOL_VERSION: [u8; 4] = [0x2a, 0xe9, 0xc6, 0x00];
const SELECTOR_NULLIFIER_REGISTRY: [u8; 4] = [0xb8, 0x70, 0x67, 0x6c];
const SELECTOR_NULLIFIER_STATUS: [u8; 4] = [0x52, 0xad, 0x0d, 0x5e];

const CLAIM_QUEUED_TOPIC: [u8; 32] = [
    0xc7, 0x32, 0xa8, 0x7b, 0x48, 0x0b, 0xe9, 0x51, 0xee, 0x9f, 0x6c, 0x11, 0x51, 0xf3, 0x77, 0x7c,
    0x75, 0x8a, 0x03, 0xaf, 0x59, 0xfa, 0x46, 0x78, 0x9b, 0xe6, 0x90, 0x8b, 0x76, 0xac, 0xa0, 0x98,
];
const CLAIM_FINALISED_TOPIC: [u8; 32] = [
    0xc0, 0x1c, 0xc7, 0x28, 0xe6, 0x7a, 0x51, 0x18, 0x15, 0xa1, 0x5f, 0x0f, 0x00, 0x30, 0xfc, 0x5a,
    0xc8, 0xdf, 0xc1, 0x5f, 0x7d, 0x0d, 0x45, 0xbe, 0x09, 0xef, 0xe2, 0xd4, 0x50, 0x85, 0x75, 0x82,
];
const CLAIM_CANCELLED_TOPIC: [u8; 32] = [
    0xbe, 0x66, 0x5e, 0xa0, 0x14, 0xd5, 0xba, 0xfd, 0xbd, 0x6e, 0xd7, 0xce, 0x37, 0xf7, 0x51, 0x1a,
    0xae, 0xb9, 0xf8, 0xc6, 0xb0, 0xfd, 0xd0, 0x17, 0x19, 0x9e, 0xb4, 0x66, 0xda, 0x11, 0x64, 0x2f,
];
const CUSTODY_RELEASE_TOPIC: [u8; 32] = [
    0x37, 0x56, 0x7a, 0x5b, 0x2d, 0xe5, 0x43, 0xb7, 0x07, 0x16, 0x2e, 0xf2, 0x36, 0x9f, 0x95, 0xd3,
    0x9e, 0x53, 0xee, 0x43, 0x62, 0x1b, 0xd3, 0x29, 0xa9, 0x96, 0xc2, 0xb7, 0x26, 0xf9, 0x0f, 0x43,
];
/// Declared Paxeer withdrawal boundary and finality policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WithdrawalConfig {
    pub endpoints: Vec<EndpointConfig>,
    pub minimum_endpoint_agreement: usize,
    pub claims_contract: EvmAddress,
    pub required_confirmations: u64,
    pub poll_cadence: Duration,
    pub delayed_after_polls: u64,
}

/// Why withdrawal boundary configuration was refused.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WithdrawalConfigError {
    Endpoints(ClientConfigError),
    Agreement(TrackerConfigError),
    ZeroClaimsContract,
    ZeroRequiredConfirmations,
    ZeroPollCadence,
    ZeroDelayedAfterPolls,
}

/// Exact facts the verified `LayerX` debit receipt must establish.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebitExpectation {
    pub activity_id: [u8; 32],
    pub network_id: u32,
    pub withdrawal_id: [u8; 32],
    pub account: [u8; 32],
    pub withdrawals_account: [u8; 32],
    pub asset_id: [u8; 32],
    pub amount: u128,
    pub recipient: EvmAddress,
}

impl DebitExpectation {
    /// Constructs the exact debit claim only after rejecting every empty
    /// consensus binding. This is the owner-side ingress used by bounded wire
    /// decoders; callers cannot accidentally admit the less strict test
    /// literal representation.
    pub fn validated(
        activity_id: [u8; 32],
        network_id: u32,
        withdrawal_id: [u8; 32],
        account: [u8; 32],
        withdrawals_account: [u8; 32],
        asset_id: [u8; 32],
        amount: u128,
        recipient: EvmAddress,
    ) -> Result<Self, DebitFault> {
        let value = Self {
            activity_id,
            network_id,
            withdrawal_id,
            account,
            withdrawals_account,
            asset_id,
            amount,
            recipient,
        };
        validate_debit_expectation(&value)?;
        Ok(value)
    }
}

/// Exact reason canonical `LayerX` debit evidence was refused.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DebitFault {
    EmptyField(&'static str),
    Unverifiable(VerificationFailure),
    Refused { result_code: i32 },
    WrongActivity { expected: [u8; 32], found: [u8; 32] },
    WrongAsset { expected: [u8; 32], found: [u8; 32] },
    WrongAmount { expected: u128, found: u128 },
    WrongAccount { expected: [u8; 32], found: [u8; 32] },
    WrongWithdrawalsAccount { expected: [u8; 32], found: [u8; 32] },
}

/// A `LayerX` debit admitted only after canonical receipt verification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommittedWithdrawalDebit {
    expectation: DebitExpectation,
    receipt_reference: [u8; 32],
    verified: VerifiedReceipt,
}

impl CommittedWithdrawalDebit {
    /// Verifies the canonical signed `LayerX` receipt and every withdrawal debit binding.
    ///
    /// # Errors
    ///
    /// Returns the first structural, signature, result, or field mismatch.
    pub fn verify(
        receipt_bytes: &[u8],
        batch: &AuthorizedBatch,
        expectation: DebitExpectation,
    ) -> Result<Self, DebitFault> {
        validate_debit_expectation(&expectation)?;
        let verified = verify_outcome(receipt_bytes, batch).map_err(DebitFault::Unverifiable)?;
        let protocol = verified.receipt().protocol().ok_or({
            DebitFault::Unverifiable(VerificationFailure {
                check: ReceiptCheck::ReceiptShape,
            })
        })?;
        if protocol.activity_id() != expectation.activity_id {
            return Err(DebitFault::WrongActivity {
                expected: expectation.activity_id,
                found: protocol.activity_id(),
            });
        }
        if protocol.result_code() != 0 {
            return Err(DebitFault::Refused {
                result_code: protocol.result_code(),
            });
        }
        if protocol.asset() != expectation.asset_id {
            return Err(DebitFault::WrongAsset {
                expected: expectation.asset_id,
                found: protocol.asset(),
            });
        }
        if protocol.amount() != expectation.amount {
            return Err(DebitFault::WrongAmount {
                expected: expectation.amount,
                found: protocol.amount(),
            });
        }
        if protocol.from() != expectation.account {
            return Err(DebitFault::WrongAccount {
                expected: expectation.account,
                found: protocol.from(),
            });
        }
        if protocol.to() != expectation.withdrawals_account {
            return Err(DebitFault::WrongWithdrawalsAccount {
                expected: expectation.withdrawals_account,
                found: protocol.to(),
            });
        }
        Ok(Self {
            expectation,
            receipt_reference: Sha256::digest(receipt_bytes).into(),
            verified,
        })
    }

    #[must_use]
    pub const fn expectation(&self) -> DebitExpectation {
        self.expectation
    }

    #[must_use]
    pub const fn receipt_reference(&self) -> [u8; 32] {
        self.receipt_reference
    }

    #[must_use]
    pub const fn verified_receipt(&self) -> &VerifiedReceipt {
        &self.verified
    }
}

/// One EVM guarantor attestation in the exact Paxeer checkpoint ABI.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WithdrawalAttestation {
    pub protocol_version: u16,
    pub network_id: u32,
    pub paxeer_chain_id: u64,
    pub settlement_contract: EvmAddress,
    pub epoch: u64,
    pub checkpoint_id: [u8; 32],
    pub checkpoint_hash: [u8; 32],
    pub guarantor_id: [u8; 32],
    pub batch_number: u64,
    pub data_availability_root: [u8; 32],
    pub replayed: bool,
    pub data_available: bool,
    pub availability_class_mask: u8,
    pub attested_at: u64,
    pub signer: EvmAddress,
    pub signature_r: [u8; 32],
    pub signature_s: [u8; 32],
    pub signature_v: u8,
}

/// Finalised checkpoint and state-membership material required by `queueClaim`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckpointProof {
    pub checkpoint_hash: [u8; 32],
    pub state_root: [u8; 32],
    pub epoch: u64,
    pub batch_number: u64,
    pub data_availability_root: [u8; 32],
    pub leaf_index: u64,
    pub siblings: Vec<[u8; 32]>,
    pub attestations: Vec<WithdrawalAttestation>,
}

impl CheckpointProof {
    /// Constructs structurally canonical checkpoint material at the wire
    /// boundary. Debit-specific root and attestation-domain checks remain in
    /// `WithdrawalBoundary`, where the verified debit and configured chain
    /// domain are available.
    pub fn validated(
        checkpoint_hash: [u8; 32],
        state_root: [u8; 32],
        epoch: u64,
        batch_number: u64,
        data_availability_root: [u8; 32],
        leaf_index: u64,
        siblings: Vec<[u8; 32]>,
        attestations: Vec<WithdrawalAttestation>,
    ) -> Result<Self, ClaimRefusal> {
        for (field, value) in [
            ("checkpoint_hash", checkpoint_hash),
            ("state_root", state_root),
            ("data_availability_root", data_availability_root),
        ] {
            if value == [0; 32] {
                return Err(ClaimRefusal::EmptyCheckpointField(field));
            }
        }
        if epoch == 0 || batch_number == 0 {
            return Err(ClaimRefusal::EmptyCheckpointField("epoch_or_batch"));
        }
        if siblings.len() > MAX_PROOF_DEPTH {
            return Err(ClaimRefusal::ProofTooDeep {
                depth: siblings.len(),
            });
        }
        if siblings.len() < 64
            && leaf_index
                .checked_shr(u32::try_from(siblings.len()).unwrap_or(64))
                .unwrap_or(0)
                != 0
        {
            return Err(ClaimRefusal::LeafIndexOutOfRange {
                leaf_index,
                depth: siblings.len(),
            });
        }
        if attestations.is_empty() {
            return Err(ClaimRefusal::NoAttestations);
        }
        let mut previous = None;
        for (index, attestation) in attestations.iter().enumerate() {
            for (valid, field) in [
                (
                    attestation.protocol_version == layerx_wire::limits::PROTOCOL_VERSION,
                    "protocol_version",
                ),
                (attestation.network_id != 0, "network_id"),
                (attestation.paxeer_chain_id != 0, "paxeer_chain_id"),
                (
                    attestation.settlement_contract.bytes() != [0; 20],
                    "settlement_contract",
                ),
                (attestation.epoch == epoch, "epoch"),
                (
                    attestation.checkpoint_id == checkpoint_hash,
                    "checkpoint_id",
                ),
                (
                    attestation.checkpoint_hash == checkpoint_hash,
                    "checkpoint_hash",
                ),
                (attestation.guarantor_id != [0; 32], "guarantor_id"),
                (attestation.batch_number == batch_number, "batch_number"),
                (
                    attestation.data_availability_root == data_availability_root,
                    "data_availability_root",
                ),
                (attestation.replayed, "replayed"),
                (attestation.data_available, "data_available"),
                (
                    attestation.availability_class_mask == ALL_AVAILABILITY_CLASSES,
                    "availability_class_mask",
                ),
                (attestation.attested_at != 0, "attested_at"),
                (attestation.signer.bytes() != [0; 20], "signer"),
                (attestation.signature_r != [0; 32], "signature_r"),
                (attestation.signature_s != [0; 32], "signature_s"),
                (matches!(attestation.signature_v, 27 | 28), "signature_v"),
            ] {
                if !valid {
                    return Err(ClaimRefusal::InvalidAttestation { index, field });
                }
            }
            if previous.is_some_and(|prior| prior >= attestation.guarantor_id) {
                return Err(ClaimRefusal::UnsortedGuarantors { index });
            }
            previous = Some(attestation.guarantor_id);
        }
        Ok(Self {
            checkpoint_hash,
            state_root,
            epoch,
            batch_number,
            data_availability_root,
            leaf_index,
            siblings,
            attestations,
        })
    }
}

/// Why a claim was refused before a wallet transaction could be requested.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClaimRefusal {
    NetworkMismatch {
        debit: u32,
        paxeer: u32,
    },
    EmptyCheckpointField(&'static str),
    ProofTooDeep {
        depth: usize,
    },
    LeafIndexOutOfRange {
        leaf_index: u64,
        depth: usize,
    },
    RootMismatch {
        computed: [u8; 32],
        declared: [u8; 32],
    },
    NoAttestations,
    InvalidAttestation {
        index: usize,
        field: &'static str,
    },
    UnsortedGuarantors {
        index: usize,
    },
    ContractLeafMismatch {
        local: [u8; 32],
        declared: [u8; 32],
    },
    ContractNullifierMismatch {
        local: [u8; 32],
        declared: [u8; 32],
    },
    CheckpointNotFinalised {
        checkpoint: [u8; 32],
    },
    CertificateNotRecorded {
        checkpoint: [u8; 32],
    },
}

/// Any typed failure while constructing, observing, or verifying a withdrawal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WithdrawalError {
    Endpoint(EndpointError),
    Contract {
        detail: String,
    },
    Refused(ClaimRefusal),
    NotFinal {
        stage: FinalityStage,
    },
    Displaced {
        lost: TransactionInclusion,
        head: u64,
        requeued: bool,
    },
    Reverted {
        inclusion: TransactionInclusion,
    },
    InclusionChanged {
        tracked: BlockRef,
        observed: Option<BlockRef>,
    },
    MissingQuorumEvidence,
    EvidenceSourceMismatch,
    TransactionTarget {
        expected: EvmAddress,
        found: Option<EvmAddress>,
    },
    TransactionInput,
    TransactionValue,
    MissingEvent(&'static str),
    DuplicateEvent(&'static str),
    MalformedEvent {
        event: &'static str,
        detail: String,
    },
    ClaimState {
        detail: String,
    },
    PayoutNotVerified {
        detail: String,
    },
    CancellationNotVerified {
        detail: String,
    },
}

/// A contract-valid claim assembled from one verified debit and checkpoint proof.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WithdrawalClaim {
    contract: EvmAddress,
    debit: CommittedWithdrawalDebit,
    proof: CheckpointProof,
    leaf: [u8; 32],
    nullifier: [u8; 32],
    calldata: Vec<u8>,
}

impl WithdrawalClaim {
    #[must_use]
    pub const fn contract(&self) -> EvmAddress {
        self.contract
    }

    #[must_use]
    pub const fn debit(&self) -> &CommittedWithdrawalDebit {
        &self.debit
    }

    #[must_use]
    pub const fn proof(&self) -> &CheckpointProof {
        &self.proof
    }

    #[must_use]
    pub const fn leaf(&self) -> [u8; 32] {
        self.leaf
    }

    #[must_use]
    pub const fn nullifier(&self) -> [u8; 32] {
        self.nullifier
    }

    #[must_use]
    pub fn calldata(&self) -> &[u8] {
        &self.calldata
    }
}

/// A queued claim whose transaction and on-chain claim record both verified final.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubmittedWithdrawalClaim {
    claim: WithdrawalClaim,
    claim_id: [u8; 32],
    available_at: u64,
    submission_transaction: TransactionHash,
    submission_inclusion: TransactionInclusion,
}

impl SubmittedWithdrawalClaim {
    #[must_use]
    pub const fn claim(&self) -> &WithdrawalClaim {
        &self.claim
    }

    #[must_use]
    pub const fn claim_id(&self) -> [u8; 32] {
        self.claim_id
    }

    #[must_use]
    pub const fn available_at(&self) -> u64 {
        self.available_at
    }

    #[must_use]
    pub const fn submission_transaction(&self) -> TransactionHash {
        self.submission_transaction
    }

    #[must_use]
    pub const fn submission_inclusion(&self) -> TransactionInclusion {
        self.submission_inclusion
    }

    #[must_use]
    pub fn finalise_calldata(&self) -> Vec<u8> {
        call_data(SELECTOR_FINALISE_CLAIM, &[self.claim_id])
    }

    #[must_use]
    pub fn cancellation_calldata(&self) -> Vec<u8> {
        call_data(SELECTOR_CANCEL_CLAIM, &[self.claim_id])
    }
}

/// Contract-defined reason the checkpoint payout is being checked.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChallengeKind {
    Fraud,
    DataAvailability,
    Equivocation,
}

/// Honest challenge hold, including all timing the contract actually declares.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChallengeHold {
    pub kind: ChallengeKind,
    pub evidence_hash: [u8; 32],
    pub raised_at: u64,
    pub window_closes_at: u64,
    pub observed_at: u64,
    pub window_elapsed: bool,
    pub resolution_has_no_on_chain_deadline: bool,
}

/// Paxeer custody disposition after an upheld checkpoint challenge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaxeerFundsDisposition {
    RetainedInVault {
        vault: EvmAddress,
        asset_id: [u8; 32],
        amount: u128,
    },
}

/// `LayerX` debit disposition after Paxeer correctly refuses payout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolDebitDisposition {
    RemainsCommittedPendingProtocolRecovery { debit_receipt_reference: [u8; 32] },
}

/// Both sides of the funds boundary after a challenged claim is cancelled.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CancelledFundsDisposition {
    pub paxeer: PaxeerFundsDisposition,
    pub layerx: ProtocolDebitDisposition,
}

/// Honest claim state; a `Paid` state is deliberately absent until payout evidence verifies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClaimProgress {
    WaitingForChallengeWindow {
        available_at: u64,
        observed_at: u64,
        remaining: Duration,
    },
    ReadyToFinalise {
        available_at: u64,
        observed_at: u64,
    },
    ChallengeHeld(ChallengeHold),
    ChallengeUpheldAwaitingCancellation {
        disposition: CancelledFundsDisposition,
    },
    PaidAwaitingPayoutVerification,
    Cancelled {
        disposition: CancelledFundsDisposition,
    },
}

/// Final evidence joining the `LayerX` debit, checkpoint proof and Paxeer payout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PayoutEvidence {
    pub debit_receipt_reference: [u8; 32],
    pub checkpoint_hash: [u8; 32],
    pub claim_id: [u8; 32],
    pub payout_transaction: TransactionHash,
    pub payout_inclusion: TransactionInclusion,
    pub vault: EvmAddress,
    pub token: EvmAddress,
    pub asset_id: [u8; 32],
    pub recipient: EvmAddress,
    pub amount: u128,
}

/// Final evidence that an upheld challenge cancelled payout without releasing custody.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CancellationEvidence {
    pub debit_receipt_reference: [u8; 32],
    pub checkpoint_hash: [u8; 32],
    pub claim_id: [u8; 32],
    pub cancellation_transaction: TransactionHash,
    pub cancellation_inclusion: TransactionInclusion,
    pub disposition: CancelledFundsDisposition,
}

/// Real Paxeer withdrawal boundary: contract reads, claim construction and evidence verification.
#[derive(Clone, Debug)]
pub struct WithdrawalBoundary {
    client: PaxeerClient,
    endpoints: Vec<EndpointConfig>,
    claims_contract: EvmAddress,
    required_confirmations: u64,
    poll_cadence: Duration,
    delayed_after_polls: u64,
    minimum_endpoint_agreement: usize,
}

impl WithdrawalBoundary {
    /// Validates and adopts a declared Paxeer withdrawal boundary.
    ///
    /// # Errors
    ///
    /// Refuses missing endpoints, zero contract/depth/cadence/stall bounds, or invalid URLs.
    pub fn new(config: WithdrawalConfig) -> Result<Self, WithdrawalConfigError> {
        if config.claims_contract.bytes() == [0; 20] {
            return Err(WithdrawalConfigError::ZeroClaimsContract);
        }
        if config.required_confirmations == 0 {
            return Err(WithdrawalConfigError::ZeroRequiredConfirmations);
        }
        if config.poll_cadence.is_zero() {
            return Err(WithdrawalConfigError::ZeroPollCadence);
        }
        if config.delayed_after_polls == 0 {
            return Err(WithdrawalConfigError::ZeroDelayedAfterPolls);
        }
        crate::finality::validate_endpoint_agreement(
            &config.endpoints,
            config.minimum_endpoint_agreement,
        )
        .map_err(WithdrawalConfigError::Agreement)?;
        let client = PaxeerClient::new(config.endpoints.clone())
            .map_err(WithdrawalConfigError::Endpoints)?;
        Ok(Self {
            client,
            endpoints: config.endpoints,
            claims_contract: config.claims_contract,
            required_confirmations: config.required_confirmations,
            poll_cadence: config.poll_cadence,
            delayed_after_polls: config.delayed_after_polls,
            minimum_endpoint_agreement: config.minimum_endpoint_agreement,
        })
    }

    #[must_use]
    pub const fn claims_contract(&self) -> EvmAddress {
        self.claims_contract
    }

    /// Constructs exact `queueClaim` bytes only after local proof checks and live Paxeer
    /// finalised-root/certificate checks agree.
    ///
    /// # Errors
    ///
    /// Returns the first proof, contract, endpoint, network, leaf or nullifier mismatch.
    pub fn construct_claim(
        &self,
        debit: CommittedWithdrawalDebit,
        proof: CheckpointProof,
    ) -> Result<WithdrawalClaim, WithdrawalError> {
        validate_checkpoint_proof(&debit, &proof)?;
        let paxeer_network = self.u32_view(
            self.claims_contract,
            &call_data(SELECTOR_NETWORK_ID, &[]),
            "networkId",
        )?;
        if paxeer_network != debit.expectation.network_id {
            return Err(WithdrawalError::Refused(ClaimRefusal::NetworkMismatch {
                debit: debit.expectation.network_id,
                paxeer: paxeer_network,
            }));
        }
        let withdrawal_words = withdrawal_words(&debit.expectation, proof.checkpoint_hash);
        let local_leaf = withdrawal_leaf(&debit.expectation);
        let declared_leaf = self.word_view(
            self.claims_contract,
            &call_data(SELECTOR_WITHDRAWAL_LEAF, &withdrawal_words),
            "withdrawalLeaf",
        )?;
        if local_leaf != declared_leaf {
            return Err(WithdrawalError::Refused(
                ClaimRefusal::ContractLeafMismatch {
                    local: local_leaf,
                    declared: declared_leaf,
                },
            ));
        }
        let local_nullifier =
            withdrawal_nullifier(paxeer_network, &debit.expectation, proof.checkpoint_hash);
        let declared_nullifier = self.word_view(
            self.claims_contract,
            &call_data(SELECTOR_WITHDRAWAL_NULLIFIER, &withdrawal_words),
            "withdrawalNullifier",
        )?;
        if local_nullifier != declared_nullifier {
            return Err(WithdrawalError::Refused(
                ClaimRefusal::ContractNullifierMismatch {
                    local: local_nullifier,
                    declared: declared_nullifier,
                },
            ));
        }
        let registry = self.address_view(
            self.claims_contract,
            &call_data(SELECTOR_REGISTRY, &[]),
            "registry",
        )?;
        let settlement_contract = self.address_view(
            registry,
            &call_data(SELECTOR_GUARANTOR_ELIGIBILITY, &[]),
            "guarantorEligibility",
        )?;
        let protocol_version = self.u32_view(
            registry,
            &call_data(SELECTOR_PROTOCOL_VERSION, &[]),
            "protocolVersion",
        )?;
        let chain_id = quantity(&self.rpc("eth_chainId", &[])?, "eth_chainId")?;
        validate_attestation_domain(
            &proof,
            protocol_version,
            debit.expectation.network_id,
            chain_id,
            settlement_contract,
        )?;
        let registered_root = self.word_view(
            registry,
            &call_data(SELECTOR_FINALISED_STATE_ROOT, &[proof.checkpoint_hash]),
            "finalisedStateRoot",
        )?;
        if registered_root != proof.state_root {
            return Err(WithdrawalError::Refused(
                ClaimRefusal::CheckpointNotFinalised {
                    checkpoint: proof.checkpoint_hash,
                },
            ));
        }
        let recorded = self.bool_view(
            registry,
            &recorded_certificate_calldata(proof.checkpoint_hash, &proof.attestations),
            "isRecordedCertificate",
        )?;
        if !recorded {
            return Err(WithdrawalError::Refused(
                ClaimRefusal::CertificateNotRecorded {
                    checkpoint: proof.checkpoint_hash,
                },
            ));
        }
        let calldata = queue_claim_calldata(&debit.expectation, &proof);
        Ok(WithdrawalClaim {
            contract: self.claims_contract,
            debit,
            proof,
            leaf: local_leaf,
            nullifier: local_nullifier,
            calldata,
        })
    }

    /// Creates a configured tracker for a wallet-submitted claim or permissionless payout call.
    ///
    /// # Errors
    ///
    /// Returns the tracker's declared configuration refusal.
    pub fn track(
        &self,
        transaction: TransactionHash,
    ) -> Result<FinalityTracker, TrackerConfigError> {
        FinalityTracker::new(
            TrackerConfig {
                endpoints: self.endpoints.clone(),
                minimum_endpoint_agreement: self.minimum_endpoint_agreement,
                required_confirmations: self.required_confirmations,
                poll_cadence: self.poll_cadence,
                delayed_after_polls: self.delayed_after_polls,
            },
            transaction,
        )
    }

    /// Accepts a submitted claim only after the transaction, queue event and stored claim agree.
    ///
    /// # Errors
    ///
    /// Returns a finality, execution, transaction, event or stored-state mismatch.
    pub fn accept_submission(
        &self,
        claim: WithdrawalClaim,
        report: &FinalityReport,
    ) -> Result<SubmittedWithdrawalClaim, WithdrawalError> {
        self.submission_from_queue(claim, report, Some(1))
    }

    /// Reconstructs a previously accepted submission from its immutable queue
    /// transaction after a service restart. The current claim may already be
    /// queued, paid, or cancelled, but its original queue event and every
    /// stored claim field must still bind to the supplied claim.
    ///
    /// # Errors
    ///
    /// Returns the same transaction/event/state mismatches as
    /// [`Self::accept_submission`] and refuses absent or unknown claim states.
    pub fn restore_submission(
        &self,
        claim: WithdrawalClaim,
        report: &FinalityReport,
    ) -> Result<SubmittedWithdrawalClaim, WithdrawalError> {
        self.submission_from_queue(claim, report, None)
    }

    fn submission_from_queue(
        &self,
        claim: WithdrawalClaim,
        report: &FinalityReport,
        expected_status: Option<u8>,
    ) -> Result<SubmittedWithdrawalClaim, WithdrawalError> {
        let observed = self.verify_transaction(report, self.claims_contract, claim.calldata())?;
        let queued = unique_log(
            &observed.logs,
            self.claims_contract,
            CLAIM_QUEUED_TOPIC,
            "ClaimQueued",
        )?;
        let event = decode_claim_queued(queued)?;
        if event.nullifier != claim.nullifier
            || event.checkpoint_hash != claim.proof.checkpoint_hash
            || event.asset_id != claim.debit.expectation.asset_id
            || event.recipient != claim.debit.expectation.recipient
            || event.amount != claim.debit.expectation.amount
            || event.claim_id == [0; 32]
        {
            return Err(WithdrawalError::MalformedEvent {
                event: "ClaimQueued",
                detail: "event fields do not bind to the constructed claim".to_owned(),
            });
        }
        let record = self.claim_record(event.claim_id)?;
        let status = expected_status.unwrap_or(record.status);
        if !matches!(status, 1..=3) {
            return Err(WithdrawalError::ClaimState {
                detail: format!("unknown or absent claim status {status}"),
            });
        }
        verify_claim_record(&claim, event.claim_id, event.available_at, &record, status)?;
        Ok(SubmittedWithdrawalClaim {
            claim,
            claim_id: event.claim_id,
            available_at: event.available_at,
            submission_transaction: report.transaction(),
            submission_inclusion: observed.inclusion,
        })
    }

    /// Reads the current claim/challenge state without ever turning contract `Paid` into user-visible
    /// payout completion before the payout transaction itself is verified.
    ///
    /// # Errors
    ///
    /// Returns malformed or contradictory contract state and endpoint failures.
    pub fn progress(
        &self,
        submitted: &SubmittedWithdrawalClaim,
    ) -> Result<ClaimProgress, WithdrawalError> {
        let record = self.claim_record(submitted.claim_id)?;
        verify_claim_record(
            &submitted.claim,
            submitted.claim_id,
            submitted.available_at,
            &record,
            record.status,
        )?;
        let disposition = self.cancelled_disposition(submitted)?;
        match record.status {
            2 => return Ok(ClaimProgress::PaidAwaitingPayoutVerification),
            3 => return Ok(ClaimProgress::Cancelled { disposition }),
            1 => {}
            status => {
                return Err(WithdrawalError::ClaimState {
                    detail: format!("unknown or absent claim status {status}"),
                })
            }
        }
        let challenge_manager = self.address_view(
            self.claims_contract,
            &call_data(SELECTOR_CHALLENGE_MANAGER, &[]),
            "challengeManager",
        )?;
        let challenge =
            self.challenge_record(challenge_manager, submitted.claim.proof.checkpoint_hash)?;
        let now = self.latest_timestamp()?;
        match challenge.status {
            1 => Ok(ClaimProgress::ChallengeHeld(ChallengeHold {
                kind: challenge.kind,
                evidence_hash: challenge.evidence_hash,
                raised_at: challenge.raised_at,
                window_closes_at: submitted.available_at,
                observed_at: now,
                window_elapsed: now >= submitted.available_at,
                resolution_has_no_on_chain_deadline: true,
            })),
            3 => Ok(ClaimProgress::ChallengeUpheldAwaitingCancellation { disposition }),
            0 | 2 if now < submitted.available_at => Ok(ClaimProgress::WaitingForChallengeWindow {
                available_at: submitted.available_at,
                observed_at: now,
                remaining: Duration::from_secs(submitted.available_at.saturating_sub(now)),
            }),
            0 | 2 => Ok(ClaimProgress::ReadyToFinalise {
                available_at: submitted.available_at,
                observed_at: now,
            }),
            status => Err(WithdrawalError::ClaimState {
                detail: format!("unknown challenge status {status}"),
            }),
        }
    }

    /// Verifies final Paxeer payout from the exact finalise call, contract state, claims event,
    /// vault release event and independently read recipient token balance.
    ///
    /// # Errors
    ///
    /// Returns finality, execution, transaction, state or evidence mismatches. No evidence means no
    /// paid-out result.
    pub fn verify_payout(
        &self,
        submitted: &SubmittedWithdrawalClaim,
        report: &FinalityReport,
    ) -> Result<PayoutEvidence, WithdrawalError> {
        let expected_input = submitted.finalise_calldata();
        let observed = self.verify_transaction(report, self.claims_contract, &expected_input)?;
        let record = self.claim_record(submitted.claim_id)?;
        verify_claim_record(
            &submitted.claim,
            submitted.claim_id,
            submitted.available_at,
            &record,
            2,
        )?;
        verify_indexed_pair(
            unique_log(
                &observed.logs,
                self.claims_contract,
                CLAIM_FINALISED_TOPIC,
                "ClaimFinalised",
            )?,
            "ClaimFinalised",
            submitted.claim_id,
            submitted.claim.nullifier,
        )?;
        let vault = self.address_view(
            self.claims_contract,
            &call_data(SELECTOR_VAULT, &[]),
            "vault",
        )?;
        verify_custody_release(
            unique_log(
                &observed.logs,
                vault,
                CUSTODY_RELEASE_TOPIC,
                "CustodyRelease",
            )?,
            submitted,
            self.claims_contract,
        )?;
        let asset_registry = self.address_view(
            vault,
            &call_data(SELECTOR_ASSET_REGISTRY, &[]),
            "assetRegistry",
        )?;
        let token = self.asset_token(asset_registry, submitted.claim.debit.expectation.asset_id)?;
        let balance = self.u256_view(
            token,
            &call_data(
                SELECTOR_BALANCE_OF,
                &[address_word(submitted.claim.debit.expectation.recipient)],
            ),
            "balanceOf",
        )?;
        if balance < submitted.claim.debit.expectation.amount {
            return Err(WithdrawalError::PayoutNotVerified {
                detail: "recipient token balance is below the released amount".to_owned(),
            });
        }
        self.verify_nullifier(submitted.claim.nullifier, 2)?;
        Ok(PayoutEvidence {
            debit_receipt_reference: submitted.claim.debit.receipt_reference,
            checkpoint_hash: submitted.claim.proof.checkpoint_hash,
            claim_id: submitted.claim_id,
            payout_transaction: report.transaction(),
            payout_inclusion: observed.inclusion,
            vault,
            token,
            asset_id: submitted.claim.debit.expectation.asset_id,
            recipient: submitted.claim.debit.expectation.recipient,
            amount: submitted.claim.debit.expectation.amount,
        })
    }

    /// Verifies an upheld-challenge cancellation and proves the cancellation transaction emitted no
    /// release of this claim's custody.
    ///
    /// # Errors
    ///
    /// Returns finality, execution, challenge, claim-state, event or custody-release mismatches.
    pub fn verify_cancellation(
        &self,
        submitted: &SubmittedWithdrawalClaim,
        report: &FinalityReport,
    ) -> Result<CancellationEvidence, WithdrawalError> {
        let expected_input = submitted.cancellation_calldata();
        let observed = self.verify_transaction(report, self.claims_contract, &expected_input)?;
        let record = self.claim_record(submitted.claim_id)?;
        verify_claim_record(
            &submitted.claim,
            submitted.claim_id,
            submitted.available_at,
            &record,
            3,
        )?;
        verify_indexed_pair(
            unique_log(
                &observed.logs,
                self.claims_contract,
                CLAIM_CANCELLED_TOPIC,
                "ClaimCancelled",
            )?,
            "ClaimCancelled",
            submitted.claim_id,
            submitted.claim.nullifier,
        )?;
        let challenge_manager = self.address_view(
            self.claims_contract,
            &call_data(SELECTOR_CHALLENGE_MANAGER, &[]),
            "challengeManager",
        )?;
        let challenge =
            self.challenge_record(challenge_manager, submitted.claim.proof.checkpoint_hash)?;
        if challenge.status != 3 {
            return Err(WithdrawalError::CancellationNotVerified {
                detail: "checkpoint challenge is not upheld".to_owned(),
            });
        }
        let vault = self.address_view(
            self.claims_contract,
            &call_data(SELECTOR_VAULT, &[]),
            "vault",
        )?;
        if observed.logs.iter().any(|log| {
            log.address == vault
                && log.topics.first() == Some(&CUSTODY_RELEASE_TOPIC)
                && log.topics.get(1) == Some(&submitted.claim_id)
        }) {
            return Err(WithdrawalError::CancellationNotVerified {
                detail: "cancelled claim emitted a custody release".to_owned(),
            });
        }
        self.verify_nullifier(submitted.claim.nullifier, 3)?;
        let disposition = self.cancelled_disposition(submitted)?;
        Ok(CancellationEvidence {
            debit_receipt_reference: submitted.claim.debit.receipt_reference,
            checkpoint_hash: submitted.claim.proof.checkpoint_hash,
            claim_id: submitted.claim_id,
            cancellation_transaction: report.transaction(),
            cancellation_inclusion: observed.inclusion,
            disposition,
        })
    }

    fn verify_transaction(
        &self,
        report: &FinalityReport,
        expected_target: EvmAddress,
        expected_input: &[u8],
    ) -> Result<ObservedTransaction, WithdrawalError> {
        let (tracked, confirmations) = match report.stage() {
            FinalityStage::Final {
                inclusion,
                confirmations,
                ..
            } => (inclusion, confirmations),
            FinalityStage::Displaced {
                lost,
                head,
                requeued,
            } => {
                return Err(WithdrawalError::Displaced {
                    lost,
                    head,
                    requeued,
                })
            }
            stage => return Err(WithdrawalError::NotFinal { stage }),
        };
        if confirmations < self.required_confirmations {
            return Err(WithdrawalError::NotFinal {
                stage: report.stage(),
            });
        }
        if tracked.execution == ExecutionOutcome::Reverted {
            return Err(WithdrawalError::Reverted { inclusion: tracked });
        }
        let evidence = report
            .evidence()
            .ok_or(WithdrawalError::MissingQuorumEvidence)?;
        if evidence.binding() != &self.client.quorum_binding(self.minimum_endpoint_agreement) {
            return Err(WithdrawalError::EvidenceSourceMismatch);
        }
        let observed_inclusion = match evidence.transaction() {
            crate::client::TransactionView::Included(inclusion) => inclusion,
            crate::client::TransactionView::Unknown | crate::client::TransactionView::Pending => {
                return Err(WithdrawalError::InclusionChanged {
                    tracked: tracked.block,
                    observed: None,
                });
            }
        };
        if observed_inclusion != tracked {
            return Err(WithdrawalError::InclusionChanged {
                tracked: tracked.block,
                observed: Some(observed_inclusion.block),
            });
        }
        if evidence.canonical_block() != Some(tracked.block) {
            return Err(WithdrawalError::InclusionChanged {
                tracked: tracked.block,
                observed: evidence.canonical_block(),
            });
        }
        let observed_confirmations = evidence
            .head()
            .saturating_sub(tracked.block.number)
            .saturating_add(1);
        if observed_confirmations < self.required_confirmations {
            return Err(WithdrawalError::NotFinal {
                stage: report.stage(),
            });
        }
        let transaction = self.transaction(report.transaction())?;
        if transaction.to != Some(expected_target) {
            return Err(WithdrawalError::TransactionTarget {
                expected: expected_target,
                found: transaction.to,
            });
        }
        if transaction.input != expected_input {
            return Err(WithdrawalError::TransactionInput);
        }
        if transaction.value.iter().any(|byte| *byte != 0) {
            return Err(WithdrawalError::TransactionValue);
        }
        let logs = evidence
            .receipt_logs()
            .ok_or(WithdrawalError::MissingQuorumEvidence)?
            .iter()
            .map(|log| LogRecord {
                address: log.address,
                topics: log.topics.clone(),
                data: log.data.clone(),
            })
            .collect();
        Ok(ObservedTransaction {
            inclusion: observed_inclusion,
            logs,
        })
    }

    fn cancelled_disposition(
        &self,
        submitted: &SubmittedWithdrawalClaim,
    ) -> Result<CancelledFundsDisposition, WithdrawalError> {
        let vault = self.address_view(
            self.claims_contract,
            &call_data(SELECTOR_VAULT, &[]),
            "vault",
        )?;
        Ok(CancelledFundsDisposition {
            paxeer: PaxeerFundsDisposition::RetainedInVault {
                vault,
                asset_id: submitted.claim.debit.expectation.asset_id,
                amount: submitted.claim.debit.expectation.amount,
            },
            layerx: ProtocolDebitDisposition::RemainsCommittedPendingProtocolRecovery {
                debit_receipt_reference: submitted.claim.debit.receipt_reference,
            },
        })
    }

    fn claim_record(&self, claim_id: [u8; 32]) -> Result<ClaimRecord, WithdrawalError> {
        let bytes = self.call_contract(
            self.claims_contract,
            &call_data(SELECTOR_CLAIM, &[claim_id]),
        )?;
        let words = exact_words(&bytes, 7, "claim")?;
        Ok(ClaimRecord {
            nullifier: words[0],
            checkpoint_hash: words[1],
            asset_id: words[2],
            recipient: word_address_decode(&words[3], "claim.recipient")?,
            amount: word_u128(&words[4], "claim.amount")?,
            available_at: word_u64(&words[5], "claim.availableAt")?,
            status: word_u8(&words[6], "claim.status")?,
        })
    }

    fn challenge_record(
        &self,
        challenge_manager: EvmAddress,
        checkpoint: [u8; 32],
    ) -> Result<ChallengeRecord, WithdrawalError> {
        let bytes = self.call_contract(
            challenge_manager,
            &call_data(SELECTOR_CHALLENGE, &[checkpoint]),
        )?;
        let words = exact_words(&bytes, 6, "challenge")?;
        let kind = match word_u8(&words[4], "challenge.kind")? {
            0 => ChallengeKind::Fraud,
            1 => ChallengeKind::DataAvailability,
            2 => ChallengeKind::Equivocation,
            other => {
                return Err(WithdrawalError::Contract {
                    detail: format!("challenge.kind: unknown value {other}"),
                })
            }
        };
        Ok(ChallengeRecord {
            evidence_hash: words[1],
            raised_at: word_u64(&words[3], "challenge.raisedAt")?,
            kind,
            status: word_u8(&words[5], "challenge.status")?,
        })
    }

    fn verify_nullifier(&self, nullifier: [u8; 32], expected: u8) -> Result<(), WithdrawalError> {
        let registry = self.address_view(
            self.claims_contract,
            &call_data(SELECTOR_NULLIFIER_REGISTRY, &[]),
            "nullifierRegistry",
        )?;
        let status = self.u8_view(
            registry,
            &call_data(SELECTOR_NULLIFIER_STATUS, &[nullifier]),
            "nullifier.status",
        )?;
        if status == expected {
            Ok(())
        } else {
            Err(WithdrawalError::ClaimState {
                detail: format!("nullifier status {status}, expected {expected}"),
            })
        }
    }

    fn asset_token(
        &self,
        registry: EvmAddress,
        asset_id: [u8; 32],
    ) -> Result<EvmAddress, WithdrawalError> {
        let bytes = self.call_contract(registry, &call_data(SELECTOR_ASSET, &[asset_id]))?;
        let words = exact_words(&bytes, 6, "asset")?;
        let token = word_address_decode(&words[0], "asset.token")?;
        if token.bytes() == [0; 20] {
            return Err(WithdrawalError::PayoutNotVerified {
                detail: "asset registry returned a zero token".to_owned(),
            });
        }
        Ok(token)
    }

    fn latest_timestamp(&self) -> Result<u64, WithdrawalError> {
        let block = self.rpc(
            "eth_getBlockByNumber",
            &[Json::Text("latest".to_owned()), Json::Bool(false)],
        )?;
        let timestamp = required(&block, "timestamp")?;
        quantity(timestamp, "block.timestamp")
    }

    fn transaction(&self, transaction: TransactionHash) -> Result<ObservedCall, WithdrawalError> {
        let value = self.rpc(
            "eth_getTransactionByHash",
            &[Json::Text(transaction.to_hex())],
        )?;
        if value.is_null() {
            return Err(WithdrawalError::Contract {
                detail: "transaction disappeared after finality".to_owned(),
            });
        }
        if fixed::<32>(required(&value, "hash")?, "transaction.hash")? != transaction.bytes() {
            return Err(WithdrawalError::Contract {
                detail: "transaction hash does not match its request".to_owned(),
            });
        }
        let to = match value.member("to") {
            None | Some(Json::Null) => None,
            Some(word) => Some(EvmAddress::new(fixed::<20>(word, "transaction.to")?)),
        };
        Ok(ObservedCall {
            to,
            input: variable_bytes(required(&value, "input")?, "transaction.input")?,
            value: quantity_bytes(required(&value, "value")?, "transaction.value")?,
        })
    }

    fn call_contract(&self, contract: EvmAddress, data: &[u8]) -> Result<Vec<u8>, WithdrawalError> {
        let result = self.rpc(
            "eth_call",
            &[
                Json::Object(vec![
                    ("to".to_owned(), Json::Text(bytes_hex(&contract.bytes()))),
                    ("data".to_owned(), Json::Text(bytes_hex(data))),
                ]),
                Json::Text("latest".to_owned()),
            ],
        )?;
        variable_bytes(&result, "eth_call result")
    }

    fn rpc(&self, method: &str, params: &[Json]) -> Result<Json, WithdrawalError> {
        self.client
            .agreed_call(method, params, self.minimum_endpoint_agreement)
            .map_err(WithdrawalError::Endpoint)
    }

    fn word_view(
        &self,
        contract: EvmAddress,
        data: &[u8],
        what: &str,
    ) -> Result<[u8; 32], WithdrawalError> {
        exact_word(&self.call_contract(contract, data)?, what)
    }

    fn bool_view(
        &self,
        contract: EvmAddress,
        data: &[u8],
        what: &str,
    ) -> Result<bool, WithdrawalError> {
        match word_u8(&self.word_view(contract, data, what)?, what)? {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(WithdrawalError::Contract {
                detail: format!("{what}: expected boolean, got {other}"),
            }),
        }
    }

    fn u8_view(
        &self,
        contract: EvmAddress,
        data: &[u8],
        what: &str,
    ) -> Result<u8, WithdrawalError> {
        word_u8(&self.word_view(contract, data, what)?, what)
    }

    fn u32_view(
        &self,
        contract: EvmAddress,
        data: &[u8],
        what: &str,
    ) -> Result<u32, WithdrawalError> {
        let value = word_u64(&self.word_view(contract, data, what)?, what)?;
        u32::try_from(value).map_err(|_| WithdrawalError::Contract {
            detail: format!("{what}: value exceeds u32"),
        })
    }

    fn u256_view(
        &self,
        contract: EvmAddress,
        data: &[u8],
        what: &str,
    ) -> Result<u128, WithdrawalError> {
        word_u128(&self.word_view(contract, data, what)?, what)
    }

    fn address_view(
        &self,
        contract: EvmAddress,
        data: &[u8],
        what: &str,
    ) -> Result<EvmAddress, WithdrawalError> {
        word_address_decode(&self.word_view(contract, data, what)?, what)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ClaimRecord {
    nullifier: [u8; 32],
    checkpoint_hash: [u8; 32],
    asset_id: [u8; 32],
    recipient: EvmAddress,
    amount: u128,
    available_at: u64,
    status: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ChallengeRecord {
    evidence_hash: [u8; 32],
    raised_at: u64,
    kind: ChallengeKind,
    status: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LogRecord {
    address: EvmAddress,
    topics: Vec<[u8; 32]>,
    data: Vec<u8>,
}

struct ObservedCall {
    to: Option<EvmAddress>,
    input: Vec<u8>,
    value: [u8; 32],
}

struct ObservedTransaction {
    inclusion: TransactionInclusion,
    logs: Vec<LogRecord>,
}

struct ClaimQueuedEvent {
    claim_id: [u8; 32],
    nullifier: [u8; 32],
    checkpoint_hash: [u8; 32],
    asset_id: [u8; 32],
    recipient: EvmAddress,
    amount: u128,
    available_at: u64,
}

fn validate_debit_expectation(expectation: &DebitExpectation) -> Result<(), DebitFault> {
    for (name, value) in [
        ("activity_id", expectation.activity_id),
        ("withdrawal_id", expectation.withdrawal_id),
        ("account", expectation.account),
        ("withdrawals_account", expectation.withdrawals_account),
        ("asset_id", expectation.asset_id),
    ] {
        if value == [0; 32] {
            return Err(DebitFault::EmptyField(name));
        }
    }
    if expectation.network_id == 0 {
        return Err(DebitFault::EmptyField("network_id"));
    }
    if expectation.amount == 0 {
        return Err(DebitFault::EmptyField("amount"));
    }
    if expectation.recipient.bytes() == [0; 20] {
        return Err(DebitFault::EmptyField("recipient"));
    }
    Ok(())
}

fn validate_checkpoint_proof(
    debit: &CommittedWithdrawalDebit,
    proof: &CheckpointProof,
) -> Result<(), WithdrawalError> {
    for (name, value) in [
        ("checkpoint_hash", proof.checkpoint_hash),
        ("state_root", proof.state_root),
        ("data_availability_root", proof.data_availability_root),
    ] {
        if value == [0; 32] {
            return Err(WithdrawalError::Refused(
                ClaimRefusal::EmptyCheckpointField(name),
            ));
        }
    }
    if proof.epoch == 0 || proof.batch_number == 0 {
        return Err(WithdrawalError::Refused(
            ClaimRefusal::EmptyCheckpointField("epoch_or_batch"),
        ));
    }
    let depth = proof.siblings.len();
    if depth > MAX_PROOF_DEPTH {
        return Err(WithdrawalError::Refused(ClaimRefusal::ProofTooDeep {
            depth,
        }));
    }
    if depth < 64
        && proof
            .leaf_index
            .checked_shr(u32::try_from(depth).unwrap_or(64))
            .unwrap_or(0)
            != 0
    {
        return Err(WithdrawalError::Refused(
            ClaimRefusal::LeafIndexOutOfRange {
                leaf_index: proof.leaf_index,
                depth,
            },
        ));
    }
    let leaf = withdrawal_leaf(&debit.expectation);
    let computed = proof_root(leaf, proof.leaf_index, &proof.siblings);
    if computed != proof.state_root {
        return Err(WithdrawalError::Refused(ClaimRefusal::RootMismatch {
            computed,
            declared: proof.state_root,
        }));
    }
    if proof.attestations.is_empty() {
        return Err(WithdrawalError::Refused(ClaimRefusal::NoAttestations));
    }
    let mut previous = None;
    for (index, attestation) in proof.attestations.iter().enumerate() {
        for (valid, field) in [
            (
                attestation.protocol_version == layerx_wire::limits::PROTOCOL_VERSION,
                "protocol_version",
            ),
            (
                attestation.network_id == debit.expectation.network_id,
                "network_id",
            ),
            (attestation.paxeer_chain_id != 0, "paxeer_chain_id"),
            (
                attestation.settlement_contract.bytes() != [0; 20],
                "settlement_contract",
            ),
            (attestation.epoch == proof.epoch, "epoch"),
            (
                attestation.checkpoint_id == proof.checkpoint_hash,
                "checkpoint_id",
            ),
            (
                attestation.checkpoint_hash == proof.checkpoint_hash,
                "checkpoint_hash",
            ),
            (attestation.guarantor_id != [0; 32], "guarantor_id"),
            (
                attestation.batch_number == proof.batch_number,
                "batch_number",
            ),
            (
                attestation.data_availability_root == proof.data_availability_root,
                "data_availability_root",
            ),
            (attestation.replayed, "replayed"),
            (attestation.data_available, "data_available"),
            (
                attestation.availability_class_mask == ALL_AVAILABILITY_CLASSES,
                "availability_class_mask",
            ),
            (attestation.attested_at != 0, "attested_at"),
            (attestation.signer.bytes() != [0; 20], "signer"),
            (attestation.signature_r != [0; 32], "signature_r"),
            (attestation.signature_s != [0; 32], "signature_s"),
            (matches!(attestation.signature_v, 27 | 28), "signature_v"),
        ] {
            if !valid {
                return Err(WithdrawalError::Refused(ClaimRefusal::InvalidAttestation {
                    index,
                    field,
                }));
            }
        }
        if previous.is_some_and(|prior| prior >= attestation.guarantor_id) {
            return Err(WithdrawalError::Refused(ClaimRefusal::UnsortedGuarantors {
                index,
            }));
        }
        previous = Some(attestation.guarantor_id);
    }
    Ok(())
}

fn validate_attestation_domain(
    proof: &CheckpointProof,
    protocol_version: u32,
    network_id: u32,
    paxeer_chain_id: u64,
    settlement_contract: EvmAddress,
) -> Result<(), WithdrawalError> {
    for (index, attestation) in proof.attestations.iter().enumerate() {
        for (valid, field) in [
            (
                u32::from(attestation.protocol_version) == protocol_version,
                "protocol_version",
            ),
            (attestation.network_id == network_id, "network_id"),
            (
                attestation.paxeer_chain_id == paxeer_chain_id,
                "paxeer_chain_id",
            ),
            (
                attestation.settlement_contract == settlement_contract,
                "settlement_contract",
            ),
        ] {
            if !valid {
                return Err(WithdrawalError::Refused(ClaimRefusal::InvalidAttestation {
                    index,
                    field,
                }));
            }
        }
    }
    Ok(())
}

fn withdrawal_leaf(expectation: &DebitExpectation) -> [u8; 32] {
    digest_parts(&[
        MERKLE_LEAF_DOMAIN,
        &expectation.withdrawal_id,
        &expectation.account,
        &expectation.asset_id,
        &expectation.amount.to_be_bytes(),
        &address_word(expectation.recipient),
    ])
}

fn withdrawal_nullifier(
    network_id: u32,
    expectation: &DebitExpectation,
    checkpoint_hash: [u8; 32],
) -> [u8; 32] {
    digest_parts(&[
        WITHDRAWAL_DOMAIN,
        &network_id.to_be_bytes(),
        &expectation.withdrawal_id,
        &expectation.account,
        &expectation.asset_id,
        &expectation.amount.to_be_bytes(),
        &checkpoint_hash,
    ])
}

fn proof_root(leaf: [u8; 32], leaf_index: u64, siblings: &[[u8; 32]]) -> [u8; 32] {
    let mut node = leaf;
    for (level, sibling) in siblings.iter().enumerate() {
        let bit = u32::try_from(level)
            .ok()
            .and_then(|shift| leaf_index.checked_shr(shift))
            .unwrap_or(0)
            & 1;
        node = if bit == 0 {
            merkle_node(&node, sibling)
        } else {
            merkle_node(sibling, &node)
        };
    }
    node
}

fn merkle_node(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    digest_parts(&[MERKLE_NODE_DOMAIN, left, right])
}

fn digest_parts(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
}

fn withdrawal_words(expectation: &DebitExpectation, checkpoint_hash: [u8; 32]) -> [[u8; 32]; 6] {
    [
        expectation.withdrawal_id,
        expectation.account,
        expectation.asset_id,
        quantity_word(&expectation.amount.to_be_bytes()),
        address_word(expectation.recipient),
        checkpoint_hash,
    ]
}

fn queue_claim_calldata(expectation: &DebitExpectation, proof: &CheckpointProof) -> Vec<u8> {
    let mut words = Vec::<[u8; 32]>::new();
    words.extend_from_slice(&withdrawal_words(expectation, proof.checkpoint_hash));
    words.push(proof.state_root);
    words.push(quantity_word(&proof.epoch.to_be_bytes()));
    words.push(quantity_word(&proof.batch_number.to_be_bytes()));
    words.push(proof.data_availability_root);
    let head_bytes = 12_usize.saturating_mul(WORD);
    let proof_words = 3_usize.saturating_add(proof.siblings.len());
    words.push(usize_word(head_bytes));
    words.push(usize_word(
        head_bytes.saturating_add(proof_words.saturating_mul(WORD)),
    ));
    words.push(quantity_word(&proof.leaf_index.to_be_bytes()));
    words.push(usize_word(WORD.saturating_mul(2)));
    words.push(usize_word(proof.siblings.len()));
    words.extend_from_slice(&proof.siblings);
    words.push(usize_word(proof.attestations.len()));
    for attestation in &proof.attestations {
        words.extend_from_slice(&attestation_words(attestation));
    }
    call_data(SELECTOR_QUEUE_CLAIM, &words)
}

fn recorded_certificate_calldata(
    checkpoint_hash: [u8; 32],
    attestations: &[WithdrawalAttestation],
) -> Vec<u8> {
    let mut words = vec![
        checkpoint_hash,
        usize_word(WORD.saturating_mul(2)),
        usize_word(attestations.len()),
    ];
    for attestation in attestations {
        words.extend_from_slice(&attestation_words(attestation));
    }
    call_data(SELECTOR_RECORDED_CERTIFICATE, &words)
}

fn attestation_words(attestation: &WithdrawalAttestation) -> [[u8; 32]; ATTESTATION_WORDS] {
    [
        quantity_word(&attestation.protocol_version.to_be_bytes()),
        quantity_word(&attestation.network_id.to_be_bytes()),
        quantity_word(&attestation.paxeer_chain_id.to_be_bytes()),
        address_word(attestation.settlement_contract),
        quantity_word(&attestation.epoch.to_be_bytes()),
        attestation.checkpoint_id,
        attestation.checkpoint_hash,
        attestation.guarantor_id,
        quantity_word(&attestation.batch_number.to_be_bytes()),
        attestation.data_availability_root,
        quantity_word(&[u8::from(attestation.replayed)]),
        quantity_word(&[u8::from(attestation.data_available)]),
        quantity_word(&[attestation.availability_class_mask]),
        quantity_word(&attestation.attested_at.to_be_bytes()),
        address_word(attestation.signer),
        attestation.signature_r,
        attestation.signature_s,
        quantity_word(&[attestation.signature_v]),
    ]
}

fn call_data(selector: [u8; 4], words: &[[u8; 32]]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(4_usize.saturating_add(words.len().saturating_mul(WORD)));
    bytes.extend_from_slice(&selector);
    for word in words {
        bytes.extend_from_slice(word);
    }
    bytes
}

fn quantity_word(bytes: &[u8]) -> [u8; 32] {
    let mut word = [0_u8; 32];
    for (slot, byte) in word
        .iter_mut()
        .skip(WORD.saturating_sub(bytes.len()))
        .zip(bytes)
    {
        *slot = *byte;
    }
    word
}

fn usize_word(value: usize) -> [u8; 32] {
    quantity_word(&value.to_be_bytes())
}

fn address_word(address: EvmAddress) -> [u8; 32] {
    let mut word = [0_u8; 32];
    word[12..].copy_from_slice(&address.bytes());
    word
}

fn validate_claim_record(
    claim: &WithdrawalClaim,
    claim_id: [u8; 32],
    available_at: u64,
    record: &ClaimRecord,
) -> Result<(), WithdrawalError> {
    if claim_id == [0; 32]
        || record.nullifier != claim.nullifier
        || record.checkpoint_hash != claim.proof.checkpoint_hash
        || record.asset_id != claim.debit.expectation.asset_id
        || record.recipient != claim.debit.expectation.recipient
        || record.amount != claim.debit.expectation.amount
        || record.available_at != available_at
    {
        return Err(WithdrawalError::ClaimState {
            detail: "stored claim does not bind to the constructed claim".to_owned(),
        });
    }
    Ok(())
}

fn verify_claim_record(
    claim: &WithdrawalClaim,
    claim_id: [u8; 32],
    available_at: u64,
    record: &ClaimRecord,
    expected_status: u8,
) -> Result<(), WithdrawalError> {
    validate_claim_record(claim, claim_id, available_at, record)?;
    if record.status != expected_status {
        return Err(WithdrawalError::ClaimState {
            detail: format!("claim status {}, expected {expected_status}", record.status),
        });
    }
    Ok(())
}

fn unique_log<'a>(
    logs: &'a [LogRecord],
    address: EvmAddress,
    topic: [u8; 32],
    name: &'static str,
) -> Result<&'a LogRecord, WithdrawalError> {
    let mut matches = logs
        .iter()
        .filter(|log| log.address == address && log.topics.first() == Some(&topic));
    let first = matches.next().ok_or(WithdrawalError::MissingEvent(name))?;
    if matches.next().is_some() {
        return Err(WithdrawalError::DuplicateEvent(name));
    }
    Ok(first)
}

fn decode_claim_queued(log: &LogRecord) -> Result<ClaimQueuedEvent, WithdrawalError> {
    if log.topics.len() != 4 || log.data.len() != 128 {
        return Err(WithdrawalError::MalformedEvent {
            event: "ClaimQueued",
            detail: format!(
                "expected 4 topics/128 bytes, got {}/{}",
                log.topics.len(),
                log.data.len()
            ),
        });
    }
    let words = bytes_words(&log.data);
    Ok(ClaimQueuedEvent {
        claim_id: log.topics[1],
        nullifier: log.topics[2],
        checkpoint_hash: log.topics[3],
        asset_id: words[0],
        recipient: word_address_decode(&words[1], "ClaimQueued.recipient")?,
        amount: word_u128(&words[2], "ClaimQueued.amount")?,
        available_at: word_u64(&words[3], "ClaimQueued.availableAt")?,
    })
}

fn verify_indexed_pair(
    log: &LogRecord,
    name: &'static str,
    first: [u8; 32],
    second: [u8; 32],
) -> Result<(), WithdrawalError> {
    if log.topics.as_slice()
        != [
            if name == "ClaimFinalised" {
                CLAIM_FINALISED_TOPIC
            } else {
                CLAIM_CANCELLED_TOPIC
            },
            first,
            second,
        ]
        || !log.data.is_empty()
    {
        return Err(WithdrawalError::MalformedEvent {
            event: name,
            detail: "indexed identifiers or empty event data mismatch".to_owned(),
        });
    }
    Ok(())
}

fn verify_custody_release(
    log: &LogRecord,
    submitted: &SubmittedWithdrawalClaim,
    settlement_module: EvmAddress,
) -> Result<(), WithdrawalError> {
    let expected = &submitted.claim.debit.expectation;
    if log.topics.len() != 4 || log.data.len() != 64 {
        return Err(WithdrawalError::MalformedEvent {
            event: "CustodyRelease",
            detail: "expected 4 topics and 64 data bytes".to_owned(),
        });
    }
    let words = bytes_words(&log.data);
    let recipient = word_address_decode(&log.topics[3], "CustodyRelease.recipient")?;
    let module = word_address_decode(&words[1], "CustodyRelease.settlementModule")?;
    if log.topics[1] != submitted.claim_id
        || log.topics[2] != expected.asset_id
        || recipient != expected.recipient
        || word_u128(&words[0], "CustodyRelease.amount")? != expected.amount
        || module != settlement_module
    {
        return Err(WithdrawalError::PayoutNotVerified {
            detail: "custody release does not bind to the claim".to_owned(),
        });
    }
    Ok(())
}

fn required<'a>(value: &'a Json, name: &str) -> Result<&'a Json, WithdrawalError> {
    value.member(name).ok_or_else(|| WithdrawalError::Contract {
        detail: format!("missing {name}"),
    })
}

fn quantity(value: &Json, what: &str) -> Result<u64, WithdrawalError> {
    let bytes = quantity_bytes(value, what)?;
    word_u64(&bytes, what)
}

fn quantity_bytes(value: &Json, what: &str) -> Result<[u8; 32], WithdrawalError> {
    let text = value.as_text().ok_or_else(|| WithdrawalError::Contract {
        detail: format!("{what}: expected hex quantity"),
    })?;
    let digits = text
        .strip_prefix("0x")
        .ok_or_else(|| WithdrawalError::Contract {
            detail: format!("{what}: missing 0x prefix"),
        })?;
    if digits.is_empty() || digits.len() > 64 {
        return Err(WithdrawalError::Contract {
            detail: format!("{what}: invalid quantity width"),
        });
    }
    let mut normalized = String::new();
    if digits.len() % 2 != 0 {
        normalized.push('0');
    }
    normalized.push_str(digits);
    let bytes = decode_digits(&normalized, what)?;
    let mut word = [0_u8; 32];
    word[WORD.saturating_sub(bytes.len())..].copy_from_slice(&bytes);
    Ok(word)
}

fn fixed<const N: usize>(value: &Json, what: &str) -> Result<[u8; N], WithdrawalError> {
    let bytes = variable_bytes(value, what)?;
    bytes
        .try_into()
        .map_err(|bytes: Vec<u8>| WithdrawalError::Contract {
            detail: format!("{what}: expected {N} bytes, got {}", bytes.len()),
        })
}

fn variable_bytes(value: &Json, what: &str) -> Result<Vec<u8>, WithdrawalError> {
    let text = value.as_text().ok_or_else(|| WithdrawalError::Contract {
        detail: format!("{what}: expected hex bytes"),
    })?;
    let digits = text
        .strip_prefix("0x")
        .ok_or_else(|| WithdrawalError::Contract {
            detail: format!("{what}: missing 0x prefix"),
        })?;
    if digits.len() % 2 != 0 {
        return Err(WithdrawalError::Contract {
            detail: format!("{what}: odd hex length"),
        });
    }
    decode_digits(digits, what)
}

fn decode_digits(digits: &str, what: &str) -> Result<Vec<u8>, WithdrawalError> {
    digits
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = hex_nibble(pair[0]);
            let low = hex_nibble(pair[1]);
            match (high, low) {
                (Some(high), Some(low)) => Ok((high << 4) | low),
                _ => Err(WithdrawalError::Contract {
                    detail: format!("{what}: non-hex digit"),
                }),
            }
        })
        .collect()
}

const fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn bytes_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::from("0x");
    for byte in bytes {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    text
}

fn exact_word(bytes: &[u8], what: &str) -> Result<[u8; 32], WithdrawalError> {
    bytes.try_into().map_err(|_| WithdrawalError::Contract {
        detail: format!("{what}: expected one word, got {} bytes", bytes.len()),
    })
}

fn exact_words(bytes: &[u8], count: usize, what: &str) -> Result<Vec<[u8; 32]>, WithdrawalError> {
    if bytes.len() != count.saturating_mul(WORD) {
        return Err(WithdrawalError::Contract {
            detail: format!("{what}: expected {count} words, got {} bytes", bytes.len()),
        });
    }
    Ok(bytes_words(bytes))
}

fn bytes_words(bytes: &[u8]) -> Vec<[u8; 32]> {
    bytes
        .chunks_exact(WORD)
        .map(|chunk| {
            let mut word = [0_u8; 32];
            word.copy_from_slice(chunk);
            word
        })
        .collect()
}

fn word_address_decode(word: &[u8; 32], what: &str) -> Result<EvmAddress, WithdrawalError> {
    if word[..12].iter().any(|byte| *byte != 0) {
        return Err(WithdrawalError::Contract {
            detail: format!("{what}: word is not an address"),
        });
    }
    let mut address = [0_u8; 20];
    address.copy_from_slice(&word[12..]);
    Ok(EvmAddress::new(address))
}

fn word_u8(word: &[u8; 32], what: &str) -> Result<u8, WithdrawalError> {
    if word[..31].iter().any(|byte| *byte != 0) {
        return Err(WithdrawalError::Contract {
            detail: format!("{what}: value exceeds u8"),
        });
    }
    Ok(word[31])
}

fn word_u64(word: &[u8; 32], what: &str) -> Result<u64, WithdrawalError> {
    if word[..24].iter().any(|byte| *byte != 0) {
        return Err(WithdrawalError::Contract {
            detail: format!("{what}: value exceeds u64"),
        });
    }
    let mut bytes = [0_u8; 8];
    bytes.copy_from_slice(&word[24..]);
    Ok(u64::from_be_bytes(bytes))
}

fn word_u128(word: &[u8; 32], what: &str) -> Result<u128, WithdrawalError> {
    if word[..16].iter().any(|byte| *byte != 0) {
        return Err(WithdrawalError::Contract {
            detail: format!("{what}: value exceeds u128"),
        });
    }
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&word[16..]);
    Ok(u128::from_be_bytes(bytes))
}
