use std::time::Duration;

use layerx_proof::state_witness::StateWitness;
use layerx_types::intent::EvmAddress;

use crate::client::{
    ClientConfigError, EndpointError, ExecutionOutcome, LogRecord, PaxeerClient, TransactionHash,
    TransactionInclusion,
};
use crate::custody::{
    decode_bool, decode_claim, decode_emergency_exit_executed, decode_status, exit_claim_id,
    exit_eligible_calldata, exit_recipient_message, exit_withdrawal_id, get_claim_calldata,
    nullifier_status_calldata, withdrawal_nullifier, CustodyClaim, EmergencyExitExecuted,
    ForcedExitMaterial, CUSTODY_PRECOMPILE, EMERGENCY_EXIT_EXECUTED_TOPIC,
};
use crate::finality::{
    FinalityReport, FinalityStage, FinalityTracker, TrackerConfig, TrackerConfigError,
};
use crate::json::Json;
use crate::rpc::EndpointConfig;
use crate::withdraw::{
    ANCHOR_PRECOMPILE, SELECTOR_FINALIZED_STATE_ROOT, SELECTOR_LATEST_FINALIZED,
};

const WORD: usize = 32;

/// Declared configuration for the forced-exit path of the custody precompile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExitConfig {
    pub endpoints: Vec<EndpointConfig>,
    pub minimum_endpoint_agreement: usize,
    /// The `LayerX` network identifier the custody module is configured with;
    /// it binds the recipient signature, withdrawal identifier and nullifier.
    pub network_id: u32,
    pub required_confirmations: u64,
    pub poll_cadence: Duration,
    pub delayed_after_polls: u64,
}

/// Why the declared forced-exit configuration was refused.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExitConfigError {
    Endpoints(ClientConfigError),
    Agreement(TrackerConfigError),
    ZeroNetworkId,
    ZeroRequiredConfirmations,
    ZeroPollCadence,
    ZeroDelayedAfterPolls,
}

/// The forced-exit material and the whole balance it proves.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExitEvidence {
    pub material: ForcedExitMaterial,
    /// The account balance the witness proves; the exit pays exactly this.
    pub finalised_balance: u128,
}

/// Forced-exit eligibility exactly as the custody precompile declares it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExitEligibility {
    Eligible {
        batch_number: u64,
        state_root: [u8; 32],
    },
    NetworkOperatingNormally {
        batch_number: u64,
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
    /// The material is empty, oversized or not a canonical witness.
    Material(&'static str),
    /// The precompile only accepts an exit against the latest finalized batch.
    StaleBatch {
        latest: u64,
        supplied: u64,
    },
    NativeBalanceNotProven,
    RecipientNotAuthorized,
    Held {
        nullifier: [u8; 32],
    },
    AlreadyExited {
        nullifier: [u8; 32],
    },
    ClaimCancelled {
        nullifier: [u8; 32],
    },
}

/// Why the exit path could not produce a verified result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExitError {
    Endpoint(EndpointError),
    Contract { detail: String },
    Refused(ExitRefusal),
    MissingEvent,
    DuplicateEvent,
    EventMismatch,
}

/// A verified forced exit in the exact form the custody precompile requires.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExitClaim {
    /// Always the custody precompile.
    pub contract: EvmAddress,
    /// Exact `requestForcedExit` bytes: queues the exit and starts its delay.
    pub calldata: Vec<u8>,
    /// Exact `executeForcedExit` bytes: pays a queued exit once its delay
    /// elapsed (or queues and pays in one call when the delay is zero).
    pub execute_calldata: Vec<u8>,
    pub claim_id: [u8; 32],
    pub batch_number: u64,
    /// The latest finalized state root: the anchor of the exit.
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

/// The forced-exit path against the latest finalized checkpoint on Paxeer.
#[derive(Clone, Debug)]
pub struct EmergencyExit {
    client: PaxeerClient,
    endpoints: Vec<EndpointConfig>,
    network_id: u32,
    required_confirmations: u64,
    poll_cadence: Duration,
    delayed_after_polls: u64,
    minimum_endpoint_agreement: usize,
}

impl EmergencyExit {
    /// Validates and adopts the declared forced-exit configuration.
    ///
    /// # Errors
    ///
    /// Refuses a zero network identifier, zero confirmation depth, zero
    /// cadence, a zero stall bound, or an invalid endpoint declaration.
    pub fn new(config: ExitConfig) -> Result<Self, ExitConfigError> {
        if config.network_id == 0 {
            return Err(ExitConfigError::ZeroNetworkId);
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
            network_id: config.network_id,
            required_confirmations: config.required_confirmations,
            poll_cadence: config.poll_cadence,
            delayed_after_polls: config.delayed_after_polls,
            minimum_endpoint_agreement: config.minimum_endpoint_agreement,
        })
    }

    /// The custody precompile every exit transaction targets.
    #[must_use]
    pub const fn contract(&self) -> EvmAddress {
        CUSTODY_PRECOMPILE
    }

    #[must_use]
    pub const fn network_id(&self) -> u32 {
        self.network_id
    }

    #[must_use]
    pub const fn required_confirmations(&self) -> u64 {
        self.required_confirmations
    }

    /// Reads forced-exit eligibility from the custody and anchor precompiles.
    ///
    /// # Errors
    ///
    /// Returns the endpoint failure or undecodable precompile answer.
    pub fn eligibility(&self) -> Result<ExitEligibility, ExitError> {
        let latest = self.view(ANCHOR_PRECOMPILE, &SELECTOR_LATEST_FINALIZED)?;
        let Some((batch_word, present)) = two_words(&latest) else {
            return Err(contract("latestFinalized: expected two words"));
        };
        if !flag(&present, "latestFinalized")? {
            return Ok(ExitEligibility::NoFinalisedCheckpoint);
        }
        let batch_number = word_quantity(&batch_word, "latestFinalized")?;
        let eligible = decode_bool(&self.view(CUSTODY_PRECOMPILE, &exit_eligible_calldata())?)
            .map_err(|error| contract(&format!("exitEligible: {error:?}")))?;
        if !eligible {
            return Ok(ExitEligibility::NetworkOperatingNormally { batch_number });
        }
        let state_root = self
            .finalised_state_root(batch_number)?
            .ok_or_else(|| contract("finalizedStateRoot: latest batch has no finalized root"))?;
        Ok(ExitEligibility::Eligible {
            batch_number,
            state_root,
        })
    }

    /// Constructs the exact forced-exit calls only after the witness proves
    /// the whole balance under the latest finalized state root, the account
    /// authority signed the recipient and the nullifier is unused.
    ///
    /// # Errors
    ///
    /// Returns the first eligibility, material, proof, authority, nullifier or
    /// endpoint refusal.
    pub fn construct_claim(&self, evidence: &ExitEvidence) -> Result<ExitClaim, ExitError> {
        validate_fields(evidence)?;
        let material = evidence
            .material
            .clone()
            .validated()
            .map_err(|_| ExitError::Refused(ExitRefusal::Material("witness")))?;
        let eligibility = self.eligibility()?;
        let ExitEligibility::Eligible {
            batch_number,
            state_root,
        } = eligibility
        else {
            return Err(ExitError::Refused(ExitRefusal::NotEligible { eligibility }));
        };
        if material.batch_number != batch_number {
            return Err(ExitError::Refused(ExitRefusal::StaleBatch {
                latest: batch_number,
                supplied: material.batch_number,
            }));
        }
        verify_exit_balance(evidence, self.network_id, state_root)?;
        let withdrawal_id = exit_withdrawal_id(
            self.network_id,
            &material.account,
            &material.asset_id,
            &state_root,
        );
        let nullifier = withdrawal_nullifier(
            self.network_id,
            &withdrawal_id,
            &material.account,
            &material.asset_id,
            evidence.finalised_balance,
            &state_root,
        );
        let status =
            decode_status(&self.view(CUSTODY_PRECOMPILE, &nullifier_status_calldata(nullifier))?)
                .map_err(|error| contract(&format!("nullifierStatus: {error:?}")))?;
        match status {
            0 => {}
            1 => return Err(ExitError::Refused(ExitRefusal::Held { nullifier })),
            2 => return Err(ExitError::Refused(ExitRefusal::AlreadyExited { nullifier })),
            3 => {
                return Err(ExitError::Refused(ExitRefusal::ClaimCancelled {
                    nullifier,
                }))
            }
            other => {
                return Err(contract(&format!(
                    "nullifierStatus: unknown status {other}"
                )))
            }
        }
        let chain_id = self.chain_id()?;
        Ok(ExitClaim {
            contract: CUSTODY_PRECOMPILE,
            calldata: material.request_calldata(),
            execute_calldata: material.execute_calldata(),
            claim_id: exit_claim_id(chain_id, nullifier),
            batch_number,
            state_root,
            withdrawal_id,
            nullifier,
            account: material.account,
            asset_id: material.asset_id,
            finalised_balance: evidence.finalised_balance,
            recipient: material.recipient,
        })
    }

    /// Reads the stored claim of a constructed exit: `None` until it is queued.
    ///
    /// # Errors
    ///
    /// Returns endpoint failures, an undecodable record, or a record that does
    /// not bind to the constructed exit.
    pub fn claim_record(&self, claim: &ExitClaim) -> Result<Option<CustodyClaim>, ExitError> {
        let record =
            decode_claim(&self.view(CUSTODY_PRECOMPILE, &get_claim_calldata(claim.claim_id))?)
                .map_err(|error| contract(&format!("getClaim: {error:?}")))?;
        if record.status == 0 {
            return Ok(None);
        }
        if record.claim_id != claim.claim_id
            || record.kind != 2
            || record.nullifier != claim.nullifier
            || record.withdrawal_id != claim.withdrawal_id
            || record.account != claim.account
            || record.asset_id != claim.asset_id
            || record.recipient != claim.recipient
            || record.amount != claim.finalised_balance
            || record.batch_number != claim.batch_number
            || record.anchor != claim.state_root
        {
            return Err(contract("getClaim: stored claim does not bind to the exit"));
        }
        Ok(Some(record))
    }

    /// Binds the `EmergencyExitExecuted` log of a final execute transaction to
    /// the constructed exit.
    ///
    /// # Errors
    ///
    /// Refuses a missing, duplicate, malformed or foreign event.
    pub fn verify_executed(
        claim: &ExitClaim,
        logs: &[LogRecord],
    ) -> Result<EmergencyExitExecuted, ExitError> {
        let mut matches = logs.iter().filter(|log| {
            log.address == CUSTODY_PRECOMPILE
                && log.topics.first() == Some(&EMERGENCY_EXIT_EXECUTED_TOPIC)
        });
        let log = matches.next().ok_or(ExitError::MissingEvent)?;
        if matches.next().is_some() {
            return Err(ExitError::DuplicateEvent);
        }
        let event = decode_emergency_exit_executed(log)
            .map_err(|error| contract(&format!("EmergencyExitExecuted: {error:?}")))?;
        if event.claim_id != claim.claim_id
            || event.nullifier != claim.nullifier
            || event.anchor != claim.state_root
            || event.account != claim.account
            || event.asset_id != claim.asset_id
            || event.recipient != claim.recipient
            || event.amount != claim.finalised_balance
        {
            return Err(ExitError::EventMismatch);
        }
        Ok(event)
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

    fn finalised_state_root(&self, batch_number: u64) -> Result<Option<[u8; 32]>, ExitError> {
        let mut data = SELECTOR_FINALIZED_STATE_ROOT.to_vec();
        let mut word = [0_u8; WORD];
        word[24..].copy_from_slice(&batch_number.to_be_bytes());
        data.extend_from_slice(&word);
        let answer = self.view(ANCHOR_PRECOMPILE, &data)?;
        let Some((root, present)) = two_words(&answer) else {
            return Err(contract("finalizedStateRoot: expected two words"));
        };
        Ok(flag(&present, "finalizedStateRoot")?.then_some(root))
    }

    fn chain_id(&self) -> Result<u64, ExitError> {
        let value = self
            .client
            .agreed_call("eth_chainId", &[], self.minimum_endpoint_agreement)
            .map_err(ExitError::Endpoint)?;
        let Json::Text(text) = &value else {
            return Err(contract("eth_chainId: expected a quantity"));
        };
        text.strip_prefix("0x")
            .filter(|digits| !digits.is_empty() && digits.len() <= 16)
            .and_then(|digits| u64::from_str_radix(digits, 16).ok())
            .ok_or_else(|| contract("eth_chainId: malformed quantity"))
    }

    fn view(&self, target: EvmAddress, data: &[u8]) -> Result<Vec<u8>, ExitError> {
        self.client
            .agreed_contract_call(target, data, self.minimum_endpoint_agreement)
            .map_err(ExitError::Endpoint)
    }
}

fn contract(detail: &str) -> ExitError {
    ExitError::Contract {
        detail: detail.to_owned(),
    }
}

fn two_words(bytes: &[u8]) -> Option<([u8; 32], [u8; 32])> {
    if bytes.len() != 2 * WORD {
        return None;
    }
    let mut first = [0_u8; WORD];
    let mut second = [0_u8; WORD];
    first.copy_from_slice(&bytes[..WORD]);
    second.copy_from_slice(&bytes[WORD..]);
    Some((first, second))
}

fn word_quantity(word: &[u8; 32], what: &str) -> Result<u64, ExitError> {
    if word[..24].iter().any(|byte| *byte != 0) {
        return Err(contract(&format!("{what}: quantity exceeds u64")));
    }
    let mut bytes = [0_u8; 8];
    bytes.copy_from_slice(&word[24..]);
    Ok(u64::from_be_bytes(bytes))
}

fn flag(word: &[u8; 32], what: &str) -> Result<bool, ExitError> {
    match word_quantity(word, what)? {
        0 => Ok(false),
        1 => Ok(true),
        other => Err(contract(&format!(
            "{what}: expected a boolean, got {other}"
        ))),
    }
}

fn validate_fields(evidence: &ExitEvidence) -> Result<(), ExitError> {
    let material = &evidence.material;
    let refusal = if material.account == [0; 32] {
        ExitRefusal::EmptyAccount
    } else if material.asset_id == [0; 32] {
        ExitRefusal::EmptyAsset
    } else if evidence.finalised_balance == 0 {
        ExitRefusal::ZeroBalance
    } else if material.recipient.bytes() == [0; 20] {
        ExitRefusal::ZeroRecipient
    } else {
        return Ok(());
    };
    Err(ExitError::Refused(refusal))
}

/// Proves the account's whole balance under `state_root` and the account
/// authority's signature over the recipient, exactly as the custody module's
/// `verify.ExitBalance` does.
///
/// # Errors
///
/// Refuses a witness that does not prove the declared account, asset and
/// balance, or a recipient the account authority did not sign.
pub fn verify_exit_balance(
    evidence: &ExitEvidence,
    network_id: u32,
    state_root: [u8; 32],
) -> Result<(), ExitError> {
    validate_fields(evidence)?;
    let material = &evidence.material;
    let refused = || ExitError::Refused(ExitRefusal::NativeBalanceNotProven);
    let witness = StateWitness::decode(&material.witness)
        .map_err(|_| ExitError::Refused(ExitRefusal::Material("witness")))?;
    witness.verify(state_root).map_err(|_| refused())?;
    if witness.module_id != 0
        || witness.key.len() != 33
        || witness.key[0] != 4
        || witness.key[1..] != material.account
        || witness.account_path.is_none()
    {
        return Err(refused());
    }
    let value = &witness.value;
    let name_length = usize::from(u16::from_be_bytes(
        value
            .get(..2)
            .ok_or_else(refused)?
            .try_into()
            .map_err(|_| refused())?,
    ));
    if !(1..=512).contains(&name_length) || value.len() != 103 + name_length {
        return Err(refused());
    }
    let at = 2 + name_length;
    if value[at] != 1
        || value[at + 49] != 1
        || value[at + 66] > 1
        || value[at + 67] > 1
        || value[at + 100] > 1
        || value[at + 17..at + 49] != material.asset_id
        || value[at + 1..at + 17] != evidence.finalised_balance.to_be_bytes()
    {
        return Err(refused());
    }
    if value[at + 100] != 1 || network_id == 0 {
        return Err(ExitError::Refused(ExitRefusal::RecipientNotAuthorized));
    }
    let key: [u8; 32] = value[at + 68..at + 100].try_into().map_err(|_| refused())?;
    let authority = ed25519_dalek::VerifyingKey::from_bytes(&key)
        .map_err(|_| ExitError::Refused(ExitRefusal::RecipientNotAuthorized))?;
    let message = exit_recipient_message(
        network_id,
        &material.account,
        &material.asset_id,
        material.recipient,
        &state_root,
    );
    authority
        .verify_strict(
            &message,
            &ed25519_dalek::Signature::from_bytes(&material.recipient_signature),
        )
        .map_err(|_| ExitError::Refused(ExitRefusal::RecipientNotAuthorized))
}
