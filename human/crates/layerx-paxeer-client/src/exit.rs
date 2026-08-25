use std::time::Duration;

use layerx_types::intent::EvmAddress;
use sha2::{Digest as _, Sha256};

use crate::client::{
    ClientConfigError, EndpointError, ExecutionOutcome, PaxeerClient, TransactionHash,
    TransactionInclusion,
};
use crate::finality::{
    FinalityReport, FinalityStage, FinalityTracker, TrackerConfig, TrackerConfigError,
};
use crate::rpc::EndpointConfig;

const SELECTOR_ELIGIBLE: [u8; 4] = [0xd8, 0x32, 0xd9, 0x2f];
const SELECTOR_LATEST_CHECKPOINT: [u8; 4] = [0xb3, 0x4e, 0xb1, 0x82];
const SELECTOR_NETWORK_ID: [u8; 4] = [0x90, 0x25, 0xe6, 0x4c];
const SELECTOR_REGISTRY: [u8; 4] = [0x7b, 0x10, 0x39, 0x99];
const SELECTOR_NULLIFIER_REGISTRY: [u8; 4] = [0xb8, 0x70, 0x67, 0x6c];
const SELECTOR_LIVENESS_BOUND: [u8; 4] = [0xc5, 0x52, 0x72, 0x66];
const SELECTOR_REQUIRED_WITHDRAWAL_ID: [u8; 4] = [0xce, 0x3c, 0xda, 0x10];
const SELECTOR_EXIT_NULLIFIER: [u8; 4] = [0xcf, 0x44, 0xf7, 0x74];
const SELECTOR_EXECUTE_EXIT: [u8; 4] = [0x4a, 0xa3, 0x8d, 0x9e];
const SELECTOR_FINALISED_STATE_ROOT: [u8; 4] = [0xdf, 0xd0, 0x60, 0x94];
const SELECTOR_REGISTERED_AT: [u8; 4] = [0x3d, 0xd8, 0xa0, 0x07];
const SELECTOR_IS_RECORDED_CERTIFICATE: [u8; 4] = [0x98, 0x6f, 0x94, 0x8b];
const SELECTOR_NULLIFIER_STATUS: [u8; 4] = [0x52, 0xad, 0x0d, 0x5e];
const SELECTOR_WITHDRAWAL_ID_USED: [u8; 4] = [0x69, 0x8d, 0xb2, 0x67];
const SELECTOR_CLAIM_FOR_NULLIFIER: [u8; 4] = [0xb5, 0x7e, 0xa0, 0xa6];

const WITHDRAWAL_ID_DOMAIN: &[u8] = b"LXP/v1/emergency-withdrawal-id\x00";
const NULLIFIER_DOMAIN: &[u8] = b"LX:WITHDRAWAL:v1";
const MERKLE_LEAF_DOMAIN: &[u8] = b"LXP/v1/merkle-leaf\x00";
const MERKLE_NODE_DOMAIN: &[u8] = b"LXP/v1/merkle-node\x00";

const MAX_PROOF_DEPTH: usize = 256;
const WORD: usize = 32;
const ATTESTATION_WORDS: usize = 13;

/// Declared configuration for the emergency-exit path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExitConfig {
    pub endpoints: Vec<EndpointConfig>,
    pub minimum_endpoint_agreement: usize,
    pub exit_contract: EvmAddress,
    pub required_confirmations: u64,
    pub poll_cadence: Duration,
    pub delayed_after_polls: u64,
}

/// Why the declared emergency-exit configuration was refused.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExitConfigError {
    Endpoints(ClientConfigError),
    Agreement(TrackerConfigError),
    ZeroExitContract,
    ZeroRequiredConfirmations,
    ZeroPollCadence,
    ZeroDelayedAfterPolls,
}

/// One recorded guarantor attestation from the published checkpoint certificate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GuarantorAttestation {
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

/// Published checkpoint evidence proving one account balance for the exit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExitEvidence {
    pub account: [u8; 32],
    pub asset_id: [u8; 32],
    pub finalised_balance: u128,
    pub recipient: EvmAddress,
    pub leaf_index: u64,
    pub siblings: Vec<[u8; 32]>,
    pub attestations: Vec<GuarantorAttestation>,
}

/// Emergency-exit eligibility exactly as Paxeer contract state declares it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExitEligibility {
    Eligible {
        checkpoint: [u8; 32],
    },
    NetworkOperatingNormally {
        checkpoint: [u8; 32],
        registered_at: u64,
        liveness_bound: u64,
    },
    NoFinalisedCheckpoint,
}

/// Typed reason a claim was refused before any wallet involvement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExitRefusal {
    NotEligible {
        eligibility: ExitEligibility,
    },
    EmptyAccount,
    EmptyAsset,
    ZeroBalance,
    ZeroRecipient,
    ProofTooDeep {
        depth: usize,
    },
    LeafIndexOutOfRange {
        leaf_index: u64,
        depth: usize,
    },
    BalanceNotProven {
        computed_root: [u8; 32],
        state_root: [u8; 32],
    },
    CertificateNotRecorded {
        checkpoint: [u8; 32],
    },
    Held {
        nullifier: [u8; 32],
        claim: [u8; 32],
    },
    AlreadyExited {
        nullifier: [u8; 32],
        claim: Option<[u8; 32]>,
    },
    ClaimCancelled {
        nullifier: [u8; 32],
        claim: [u8; 32],
    },
}

/// Why the exit path could not produce a verified result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExitError {
    Endpoint(EndpointError),
    Contract { detail: String },
    Refused(ExitRefusal),
}

/// A verified exit claim in the exact form the Paxeer contract requires.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExitClaim {
    pub contract: EvmAddress,
    pub calldata: Vec<u8>,
    pub checkpoint: [u8; 32],
    pub state_root: [u8; 32],
    pub withdrawal_id: [u8; 32],
    pub nullifier: [u8; 32],
    pub account: [u8; 32],
    pub asset_id: [u8; 32],
    pub finalised_balance: u128,
    pub recipient: EvmAddress,
}

/// Staged progress of a submitted exit under withdrawal honesty rules.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExitProgress {
    Pending,
    Confirming {
        execution: ExecutionOutcome,
        confirmations: u64,
        required: u64,
    },
    Displaced {
        requeued: bool,
    },
    Settled {
        inclusion: TransactionInclusion,
        confirmations: u64,
    },
    Refused {
        inclusion: TransactionInclusion,
        confirmations: u64,
    },
}

impl ExitProgress {
    /// Maps one finality report onto the exit's staged progress, with
    /// settlement reported only on verified Paxeer finality.
    #[must_use]
    pub const fn of(report: &FinalityReport) -> Self {
        match report.stage() {
            FinalityStage::Announced
            | FinalityStage::Missing { .. }
            | FinalityStage::Pooled { .. } => Self::Pending,
            FinalityStage::Confirming {
                inclusion,
                confirmations,
                required,
            } => Self::Confirming {
                execution: inclusion.execution,
                confirmations,
                required,
            },
            FinalityStage::Displaced { requeued, .. } => Self::Displaced { requeued },
            FinalityStage::Final {
                inclusion,
                confirmations,
                ..
            } => match inclusion.execution {
                ExecutionOutcome::Succeeded => Self::Settled {
                    inclusion,
                    confirmations,
                },
                ExecutionOutcome::Reverted => Self::Refused {
                    inclusion,
                    confirmations,
                },
            },
        }
    }
}

/// The emergency-exit path against the last finalised checkpoint on Paxeer.
#[derive(Clone, Debug)]
pub struct EmergencyExit {
    client: PaxeerClient,
    endpoints: Vec<EndpointConfig>,
    contract: EvmAddress,
    required_confirmations: u64,
    poll_cadence: Duration,
    delayed_after_polls: u64,
    minimum_endpoint_agreement: usize,
}

impl EmergencyExit {
    /// Validates and adopts the declared emergency-exit configuration.
    ///
    /// # Errors
    ///
    /// Refuses a zero contract address, zero confirmation depth, zero cadence,
    /// a zero stall bound, or an invalid endpoint declaration.
    pub fn new(config: ExitConfig) -> Result<Self, ExitConfigError> {
        if config.exit_contract.bytes() == [0; 20] {
            return Err(ExitConfigError::ZeroExitContract);
        }
        if config.required_confirmations == 0 {
            return Err(ExitConfigError::ZeroRequiredConfirmations);
        }
        if config.poll_cadence.is_zero() {
            return Err(ExitConfigError::ZeroPollCadence);
        }
        if config.delayed_after_polls == 0 {
            return Err(ExitConfigError::ZeroDelayedAfterPolls);
        }
        crate::finality::validate_endpoint_agreement(
            &config.endpoints,
            config.minimum_endpoint_agreement,
        )
        .map_err(ExitConfigError::Agreement)?;
        let client =
            PaxeerClient::new(config.endpoints.clone()).map_err(ExitConfigError::Endpoints)?;
        Ok(Self {
            client,
            endpoints: config.endpoints,
            contract: config.exit_contract,
            required_confirmations: config.required_confirmations,
            poll_cadence: config.poll_cadence,
            delayed_after_polls: config.delayed_after_polls,
            minimum_endpoint_agreement: config.minimum_endpoint_agreement,
        })
    }

    #[must_use]
    pub const fn contract(&self) -> EvmAddress {
        self.contract
    }

    #[must_use]
    pub const fn required_confirmations(&self) -> u64 {
        self.required_confirmations
    }

    /// Reads emergency-exit eligibility from Paxeer contract state.
    ///
    /// # Errors
    ///
    /// Returns the endpoint failure or undecodable contract answer.
    pub fn eligibility(&self) -> Result<ExitEligibility, ExitError> {
        let checkpoint = self.word_view(
            self.contract,
            &call_data(SELECTOR_LATEST_CHECKPOINT, &[]),
            "latestCheckpointHash",
        )?;
        if checkpoint == [0; 32] {
            return Ok(ExitEligibility::NoFinalisedCheckpoint);
        }
        let eligible = self.bool_view(
            self.contract,
            &call_data(SELECTOR_ELIGIBLE, &[]),
            "eligible",
        )?;
        if eligible {
            return Ok(ExitEligibility::Eligible { checkpoint });
        }
        let registry = self.address_view(
            self.contract,
            &call_data(SELECTOR_REGISTRY, &[]),
            "registry",
        )?;
        let registered_at = self.u64_view(
            registry,
            &call_data(SELECTOR_REGISTERED_AT, &[checkpoint]),
            "registeredAt",
        )?;
        let liveness_bound = self.u64_view(
            self.contract,
            &call_data(SELECTOR_LIVENESS_BOUND, &[]),
            "livenessBound",
        )?;
        Ok(ExitEligibility::NetworkOperatingNormally {
            checkpoint,
            registered_at,
            liveness_bound,
        })
    }

    /// Constructs the exit claim against the last finalised checkpoint,
    /// verifying eligibility, the balance proof, the recorded certificate and
    /// the nullifier standing before any wallet involvement.
    ///
    /// # Errors
    ///
    /// Returns the typed refusal, endpoint failure or contract inconsistency.
    pub fn construct_claim(&self, evidence: &ExitEvidence) -> Result<ExitClaim, ExitError> {
        validate_fields(evidence)?;
        let checkpoint = match self.eligibility()? {
            ExitEligibility::Eligible { checkpoint } => checkpoint,
            eligibility => {
                return Err(ExitError::Refused(ExitRefusal::NotEligible { eligibility }))
            }
        };
        let state_root = self.finalised_state_root(checkpoint)?;
        verify_balance_proof(evidence, state_root)?;
        let network_id = self.u32_view(
            self.contract,
            &call_data(SELECTOR_NETWORK_ID, &[]),
            "networkId",
        )?;
        let withdrawal_id = self.resolved_withdrawal_id(network_id, evidence, checkpoint)?;
        let nullifier = self.resolved_nullifier(network_id, withdrawal_id, evidence, checkpoint)?;
        self.verify_standing(nullifier, withdrawal_id)?;
        self.verify_recorded_certificate(checkpoint, &evidence.attestations)?;
        let calldata = execute_exit_calldata(withdrawal_id, evidence, checkpoint, state_root);
        Ok(ExitClaim {
            contract: self.contract,
            calldata,
            checkpoint,
            state_root,
            withdrawal_id,
            nullifier,
            account: evidence.account,
            asset_id: evidence.asset_id,
            finalised_balance: evidence.finalised_balance,
            recipient: evidence.recipient,
        })
    }

    /// Tracks a submitted exit transaction to Paxeer finality with the same
    /// verification rigor as withdrawals.
    ///
    /// # Errors
    ///
    /// Returns the tracker's typed configuration refusal.
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

    fn finalised_state_root(&self, checkpoint: [u8; 32]) -> Result<[u8; 32], ExitError> {
        let registry = self.address_view(
            self.contract,
            &call_data(SELECTOR_REGISTRY, &[]),
            "registry",
        )?;
        let state_root = self.word_view(
            registry,
            &call_data(SELECTOR_FINALISED_STATE_ROOT, &[checkpoint]),
            "finalisedStateRoot",
        )?;
        if state_root == [0; 32] {
            return Err(ExitError::Contract {
                detail: "finalisedStateRoot: zero root for the latest checkpoint".to_owned(),
            });
        }
        Ok(state_root)
    }

    fn resolved_withdrawal_id(
        &self,
        network_id: u32,
        evidence: &ExitEvidence,
        checkpoint: [u8; 32],
    ) -> Result<[u8; 32], ExitError> {
        let local = emergency_withdrawal_id(
            network_id,
            &evidence.account,
            &evidence.asset_id,
            &checkpoint,
        );
        let declared = self.word_view(
            self.contract,
            &call_data(
                SELECTOR_REQUIRED_WITHDRAWAL_ID,
                &[evidence.account, evidence.asset_id, checkpoint],
            ),
            "requiredWithdrawalId",
        )?;
        if declared != local {
            return Err(ExitError::Contract {
                detail: "requiredWithdrawalId: contract disagrees with the local domain".to_owned(),
            });
        }
        Ok(local)
    }

    fn resolved_nullifier(
        &self,
        network_id: u32,
        withdrawal_id: [u8; 32],
        evidence: &ExitEvidence,
        checkpoint: [u8; 32],
    ) -> Result<[u8; 32], ExitError> {
        let local = exit_nullifier(
            network_id,
            &withdrawal_id,
            &evidence.account,
            &evidence.asset_id,
            evidence.finalised_balance,
            &checkpoint,
        );
        let declared = self.word_view(
            self.contract,
            &call_data(
                SELECTOR_EXIT_NULLIFIER,
                &claim_words(withdrawal_id, evidence, checkpoint),
            ),
            "exitNullifier",
        )?;
        if declared != local {
            return Err(ExitError::Contract {
                detail: "exitNullifier: contract disagrees with the local domain".to_owned(),
            });
        }
        Ok(local)
    }

    fn verify_standing(
        &self,
        nullifier: [u8; 32],
        withdrawal_id: [u8; 32],
    ) -> Result<(), ExitError> {
        let nullifiers = self.address_view(
            self.contract,
            &call_data(SELECTOR_NULLIFIER_REGISTRY, &[]),
            "nullifierRegistry",
        )?;
        let status = self.u64_view(
            nullifiers,
            &call_data(SELECTOR_NULLIFIER_STATUS, &[nullifier]),
            "status",
        )?;
        if status != 0 {
            let claim = self.word_view(
                nullifiers,
                &call_data(SELECTOR_CLAIM_FOR_NULLIFIER, &[nullifier]),
                "claimForNullifier",
            )?;
            let refusal = match status {
                1 => ExitRefusal::Held { nullifier, claim },
                2 => ExitRefusal::AlreadyExited {
                    nullifier,
                    claim: Some(claim),
                },
                3 => ExitRefusal::ClaimCancelled { nullifier, claim },
                other => {
                    return Err(ExitError::Contract {
                        detail: format!("status: unknown nullifier status {other}"),
                    })
                }
            };
            return Err(ExitError::Refused(refusal));
        }
        let used = self.bool_view(
            nullifiers,
            &call_data(SELECTOR_WITHDRAWAL_ID_USED, &[withdrawal_id]),
            "withdrawalIdUsed",
        )?;
        if used {
            return Err(ExitError::Refused(ExitRefusal::AlreadyExited {
                nullifier,
                claim: None,
            }));
        }
        Ok(())
    }

    fn verify_recorded_certificate(
        &self,
        checkpoint: [u8; 32],
        attestations: &[GuarantorAttestation],
    ) -> Result<(), ExitError> {
        if attestations.is_empty() {
            return Err(ExitError::Refused(ExitRefusal::CertificateNotRecorded {
                checkpoint,
            }));
        }
        let registry = self.address_view(
            self.contract,
            &call_data(SELECTOR_REGISTRY, &[]),
            "registry",
        )?;
        let recorded = self.bool_view(
            registry,
            &recorded_certificate_calldata(checkpoint, attestations),
            "isRecordedCertificate",
        )?;
        if recorded {
            Ok(())
        } else {
            Err(ExitError::Refused(ExitRefusal::CertificateNotRecorded {
                checkpoint,
            }))
        }
    }

    fn view(&self, contract: EvmAddress, data: &[u8]) -> Result<Vec<u8>, ExitError> {
        self.client
            .agreed_contract_call(contract, data, self.minimum_endpoint_agreement)
            .map_err(ExitError::Endpoint)
    }

    fn word_view(
        &self,
        contract: EvmAddress,
        data: &[u8],
        what: &str,
    ) -> Result<[u8; 32], ExitError> {
        exact_word(&self.view(contract, data)?, what)
    }

    fn bool_view(&self, contract: EvmAddress, data: &[u8], what: &str) -> Result<bool, ExitError> {
        let word = self.word_view(contract, data, what)?;
        match word_quantity(&word, what)? {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(ExitError::Contract {
                detail: format!("{what}: expected a boolean, got {other}"),
            }),
        }
    }

    fn u64_view(&self, contract: EvmAddress, data: &[u8], what: &str) -> Result<u64, ExitError> {
        let word = self.word_view(contract, data, what)?;
        word_quantity(&word, what)
    }

    fn u32_view(&self, contract: EvmAddress, data: &[u8], what: &str) -> Result<u32, ExitError> {
        let word = self.word_view(contract, data, what)?;
        let quantity = word_quantity(&word, what)?;
        u32::try_from(quantity).map_err(|_| ExitError::Contract {
            detail: format!("{what}: quantity {quantity} exceeds u32"),
        })
    }

    fn address_view(
        &self,
        contract: EvmAddress,
        data: &[u8],
        what: &str,
    ) -> Result<EvmAddress, ExitError> {
        let word = self.word_view(contract, data, what)?;
        if word.iter().take(12).any(|byte| *byte != 0) {
            return Err(ExitError::Contract {
                detail: format!("{what}: return word is not an address"),
            });
        }
        let mut bytes = [0_u8; 20];
        for (slot, byte) in bytes.iter_mut().zip(word.iter().skip(12)) {
            *slot = *byte;
        }
        Ok(EvmAddress::new(bytes))
    }
}

fn validate_fields(evidence: &ExitEvidence) -> Result<(), ExitError> {
    if evidence.account == [0; 32] {
        return Err(ExitError::Refused(ExitRefusal::EmptyAccount));
    }
    if evidence.asset_id == [0; 32] {
        return Err(ExitError::Refused(ExitRefusal::EmptyAsset));
    }
    if evidence.finalised_balance == 0 {
        return Err(ExitError::Refused(ExitRefusal::ZeroBalance));
    }
    if evidence.recipient.bytes() == [0; 20] {
        return Err(ExitError::Refused(ExitRefusal::ZeroRecipient));
    }
    Ok(())
}

fn verify_balance_proof(evidence: &ExitEvidence, state_root: [u8; 32]) -> Result<(), ExitError> {
    let depth = evidence.siblings.len();
    if depth > MAX_PROOF_DEPTH {
        return Err(ExitError::Refused(ExitRefusal::ProofTooDeep { depth }));
    }
    let shifted = u32::try_from(depth)
        .ok()
        .and_then(|bits| evidence.leaf_index.checked_shr(bits))
        .unwrap_or(0);
    if shifted != 0 {
        return Err(ExitError::Refused(ExitRefusal::LeafIndexOutOfRange {
            leaf_index: evidence.leaf_index,
            depth,
        }));
    }
    let leaf = balance_leaf(
        &evidence.account,
        &evidence.asset_id,
        evidence.finalised_balance,
        evidence.recipient,
    );
    let computed_root = proof_root(leaf, evidence.leaf_index, &evidence.siblings);
    if computed_root == state_root {
        Ok(())
    } else {
        Err(ExitError::Refused(ExitRefusal::BalanceNotProven {
            computed_root,
            state_root,
        }))
    }
}

fn proof_root(leaf: [u8; 32], leaf_index: u64, siblings: &[[u8; 32]]) -> [u8; 32] {
    let mut node = leaf;
    for (level, sibling) in siblings.iter().enumerate() {
        let bit = u32::try_from(level)
            .ok()
            .and_then(|bits| leaf_index.checked_shr(bits))
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

/// Hashes one balance leaf under the exact Paxeer bridge leaf domain.
#[must_use]
pub fn balance_leaf(
    account: &[u8; 32],
    asset_id: &[u8; 32],
    finalised_balance: u128,
    recipient: EvmAddress,
) -> [u8; 32] {
    digest_parts(&[
        MERKLE_LEAF_DOMAIN,
        account,
        asset_id,
        &finalised_balance.to_be_bytes(),
        &address_word(recipient),
    ])
}

/// Hashes two child digests under the exact Paxeer bridge node domain.
#[must_use]
pub fn merkle_node(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    digest_parts(&[MERKLE_NODE_DOMAIN, left, right])
}

/// Computes the emergency withdrawal identifier the exit contract requires.
#[must_use]
pub fn emergency_withdrawal_id(
    network_id: u32,
    account: &[u8; 32],
    asset_id: &[u8; 32],
    checkpoint: &[u8; 32],
) -> [u8; 32] {
    digest_parts(&[
        WITHDRAWAL_ID_DOMAIN,
        &network_id.to_be_bytes(),
        account,
        asset_id,
        checkpoint,
    ])
}

/// Computes the withdrawal nullifier the exit consumes exactly once.
#[must_use]
pub fn exit_nullifier(
    network_id: u32,
    withdrawal_id: &[u8; 32],
    account: &[u8; 32],
    asset_id: &[u8; 32],
    finalised_balance: u128,
    checkpoint: &[u8; 32],
) -> [u8; 32] {
    digest_parts(&[
        NULLIFIER_DOMAIN,
        &network_id.to_be_bytes(),
        withdrawal_id,
        account,
        asset_id,
        &finalised_balance.to_be_bytes(),
        checkpoint,
    ])
}

fn digest_parts(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
}

fn claim_words(
    withdrawal_id: [u8; 32],
    evidence: &ExitEvidence,
    checkpoint: [u8; 32],
) -> [[u8; 32]; 6] {
    [
        withdrawal_id,
        evidence.account,
        evidence.asset_id,
        quantity_word(&evidence.finalised_balance.to_be_bytes()),
        address_word(evidence.recipient),
        checkpoint,
    ]
}

fn execute_exit_calldata(
    withdrawal_id: [u8; 32],
    evidence: &ExitEvidence,
    checkpoint: [u8; 32],
    state_root: [u8; 32],
) -> Vec<u8> {
    let mut words: Vec<[u8; 32]> = Vec::new();
    words.extend_from_slice(&claim_words(withdrawal_id, evidence, checkpoint));
    words.push(state_root);
    let proof_offset = words.len().saturating_add(2).saturating_mul(WORD);
    let proof_words = evidence.siblings.len().saturating_add(3);
    words.push(usize_word(proof_offset));
    words.push(usize_word(
        proof_offset.saturating_add(proof_words.saturating_mul(WORD)),
    ));
    words.push(quantity_word(&evidence.leaf_index.to_be_bytes()));
    words.push(usize_word(WORD.saturating_mul(2)));
    words.push(usize_word(evidence.siblings.len()));
    words.extend_from_slice(&evidence.siblings);
    words.push(usize_word(evidence.attestations.len()));
    for attestation in &evidence.attestations {
        words.extend_from_slice(&attestation_words(attestation));
    }
    call_data(SELECTOR_EXECUTE_EXIT, &words)
}

fn recorded_certificate_calldata(
    checkpoint: [u8; 32],
    attestations: &[GuarantorAttestation],
) -> Vec<u8> {
    let mut words: Vec<[u8; 32]> = vec![
        checkpoint,
        usize_word(WORD.saturating_mul(2)),
        usize_word(attestations.len()),
    ];
    for attestation in attestations {
        words.extend_from_slice(&attestation_words(attestation));
    }
    call_data(SELECTOR_IS_RECORDED_CERTIFICATE, &words)
}

fn attestation_words(attestation: &GuarantorAttestation) -> [[u8; 32]; ATTESTATION_WORDS] {
    [
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
    let mut data = Vec::with_capacity(words.len().saturating_mul(WORD).saturating_add(4));
    data.extend_from_slice(&selector);
    for word in words {
        data.extend_from_slice(word);
    }
    data
}

fn quantity_word(big_endian: &[u8]) -> [u8; 32] {
    let mut word = [0_u8; 32];
    for (slot, byte) in word
        .iter_mut()
        .skip(WORD.saturating_sub(big_endian.len()))
        .zip(big_endian)
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
    for (slot, byte) in word.iter_mut().skip(12).zip(address.bytes()) {
        *slot = byte;
    }
    word
}

fn exact_word(bytes: &[u8], what: &str) -> Result<[u8; 32], ExitError> {
    if bytes.len() != WORD {
        return Err(ExitError::Contract {
            detail: format!("{what}: expected 32 return bytes, got {}", bytes.len()),
        });
    }
    let mut word = [0_u8; 32];
    for (slot, byte) in word.iter_mut().zip(bytes) {
        *slot = *byte;
    }
    Ok(word)
}

fn word_quantity(word: &[u8; 32], what: &str) -> Result<u64, ExitError> {
    if word.iter().take(24).any(|byte| *byte != 0) {
        return Err(ExitError::Contract {
            detail: format!("{what}: return word exceeds u64"),
        });
    }
    Ok(word
        .iter()
        .skip(24)
        .fold(0_u64, |quantity, byte| (quantity << 8) | u64::from(*byte)))
}
