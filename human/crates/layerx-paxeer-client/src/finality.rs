use std::time::Duration;

use crate::client::{
    BlockRef, ClientConfigError, EndpointError, FinalityObservation, LogRecord, PaxeerClient,
    QuorumBinding, TransactionHash, TransactionInclusion, TransactionView,
};
use crate::rpc::{EndpointConfig, EndpointFailure, EndpointTransport};

mod finality_wire;

pub(crate) use finality_wire::{decode as decode_wire, encode as encode_wire};

/// Declared tracking configuration: endpoints, depth, cadence and stall bound.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrackerConfig {
    pub endpoints: Vec<EndpointConfig>,
    pub minimum_endpoint_agreement: usize,
    pub required_confirmations: u64,
    pub poll_cadence: Duration,
    pub delayed_after_polls: u64,
}

/// Why the declared tracking configuration was refused.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TrackerConfigError {
    Endpoints(ClientConfigError),
    ZeroRequiredConfirmations,
    ZeroPollCadence,
    ZeroDelayedAfterPolls,
    ZeroEndpointAgreement,
    InsufficientEndpointAgreement,
    ProductionAgreementRequiresTwo,
}

/// The complete staged state matrix of one tracked custody transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FinalityStage {
    Announced,
    Missing {
        head: u64,
    },
    Pooled {
        head: u64,
    },
    Confirming {
        inclusion: TransactionInclusion,
        confirmations: u64,
        required: u64,
    },
    Final {
        inclusion: TransactionInclusion,
        confirmations: u64,
        required: u64,
    },
    Displaced {
        lost: TransactionInclusion,
        head: u64,
        requeued: bool,
    },
}

/// Whether the chain is progressing, chain-side delayed, or unreachable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChainSignal {
    Progressing,
    Delayed {
        stalled_polls: u64,
        threshold: u64,
        stalled_for: Duration,
        delayed_after: Duration,
    },
    Unreachable {
        error: EndpointError,
    },
}

/// How the configured endpoints served this poll: fully, by failover, or not.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EndpointSignal {
    Serving,
    Degraded { failovers: Vec<EndpointFailure> },
    Unreachable { error: EndpointError },
}

/// Confirmation depth against the bridge-required target, at any stage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfirmationProgress {
    pub confirmed: u64,
    pub required: u64,
}

/// One honest poll result: the stage, the signals behind it, and the history.
///
/// ```compile_fail
/// use layerx_paxeer_client::{
///     ChainSignal, ConfirmationProgress, EndpointSignal, FinalityReport, FinalityStage,
///     TransactionHash,
/// };
/// let _forged = FinalityReport {
///     transaction: TransactionHash::new([0; 32]),
///     stage: FinalityStage::Announced,
///     signal: ChainSignal::Progressing,
///     endpoint: EndpointSignal::Serving,
///     progress: ConfirmationProgress { confirmed: 0, required: 1 },
///     displacements: 0,
///     polls: 0,
/// };
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FinalityReport {
    transaction: TransactionHash,
    stage: FinalityStage,
    signal: ChainSignal,
    endpoint: EndpointSignal,
    progress: ConfirmationProgress,
    displacements: u64,
    polls: u64,
    evidence: Option<FinalityEvidence>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FinalityEvidence {
    binding: QuorumBinding,
    chain_id: u64,
    head: u64,
    transaction: TransactionView,
    canonical_block: Option<BlockRef>,
    receipt_logs: Option<Vec<LogRecord>>,
}

impl FinalityEvidence {
    fn from_observation(binding: QuorumBinding, observation: &FinalityObservation) -> Self {
        Self {
            binding,
            chain_id: observation.chain_id,
            head: observation.head,
            transaction: observation.transaction,
            canonical_block: observation.canonical_block,
            receipt_logs: observation.receipt_logs.clone(),
        }
    }

    pub(crate) const fn binding(&self) -> &QuorumBinding {
        &self.binding
    }

    pub(crate) const fn chain_id(&self) -> u64 {
        self.chain_id
    }

    pub(crate) const fn head(&self) -> u64 {
        self.head
    }

    pub(crate) const fn transaction(&self) -> TransactionView {
        self.transaction
    }

    pub(crate) const fn canonical_block(&self) -> Option<BlockRef> {
        self.canonical_block
    }

    pub(crate) fn receipt_logs(&self) -> Option<&[LogRecord]> {
        self.receipt_logs.as_deref()
    }
}

impl FinalityReport {
    #[must_use]
    pub const fn transaction(&self) -> TransactionHash {
        self.transaction
    }

    #[must_use]
    pub const fn stage(&self) -> FinalityStage {
        self.stage
    }

    #[must_use]
    pub fn signal(&self) -> ChainSignal {
        self.signal.clone()
    }

    #[must_use]
    pub fn endpoint(&self) -> EndpointSignal {
        self.endpoint.clone()
    }

    #[must_use]
    pub const fn progress(&self) -> ConfirmationProgress {
        self.progress
    }

    #[must_use]
    pub const fn displacements(&self) -> u64 {
        self.displacements
    }

    #[must_use]
    pub const fn polls(&self) -> u64 {
        self.polls
    }

    pub(crate) const fn evidence(&self) -> Option<&FinalityEvidence> {
        self.evidence.as_ref()
    }
}

/// Tracks one custody transaction from broadcast to bridge-required finality.
#[derive(Debug)]
pub struct FinalityTracker {
    client: PaxeerClient,
    transaction: TransactionHash,
    required_confirmations: u64,
    poll_cadence: Duration,
    delayed_after_polls: u64,
    minimum_endpoint_agreement: usize,
    recorded: Option<TransactionInclusion>,
    lost: Option<TransactionInclusion>,
    last_observation: Option<(u64, FinalityStage)>,
    stalled_polls: u64,
    displacements: u64,
    polls: u64,
    latest: FinalityReport,
}

impl FinalityTracker {
    /// Builds a tracker from declared configuration and the tracked hash.
    ///
    /// # Errors
    ///
    /// Refuses zero confirmation depth, zero cadence, a zero stall bound, or
    /// an invalid endpoint declaration.
    pub fn new(
        config: TrackerConfig,
        transaction: TransactionHash,
    ) -> Result<Self, TrackerConfigError> {
        if config.required_confirmations == 0 {
            return Err(TrackerConfigError::ZeroRequiredConfirmations);
        }
        if config.poll_cadence.is_zero() {
            return Err(TrackerConfigError::ZeroPollCadence);
        }
        if config.delayed_after_polls == 0 {
            return Err(TrackerConfigError::ZeroDelayedAfterPolls);
        }
        let client =
            PaxeerClient::new(config.endpoints.clone()).map_err(TrackerConfigError::Endpoints)?;
        validate_endpoint_agreement(&config.endpoints, config.minimum_endpoint_agreement)?;
        Ok(Self {
            client,
            transaction,
            required_confirmations: config.required_confirmations,
            poll_cadence: config.poll_cadence,
            delayed_after_polls: config.delayed_after_polls,
            minimum_endpoint_agreement: config.minimum_endpoint_agreement,
            recorded: None,
            lost: None,
            last_observation: None,
            stalled_polls: 0,
            displacements: 0,
            polls: 0,
            latest: FinalityReport {
                transaction,
                stage: FinalityStage::Announced,
                signal: ChainSignal::Progressing,
                endpoint: EndpointSignal::Serving,
                progress: ConfirmationProgress {
                    confirmed: 0,
                    required: config.required_confirmations,
                },
                displacements: 0,
                polls: 0,
                evidence: None,
            },
        })
    }

    #[must_use]
    pub const fn transaction(&self) -> TransactionHash {
        self.transaction
    }

    #[must_use]
    pub const fn required_confirmations(&self) -> u64 {
        self.required_confirmations
    }

    #[must_use]
    pub const fn poll_cadence(&self) -> Duration {
        self.poll_cadence
    }

    #[must_use]
    pub const fn latest(&self) -> &FinalityReport {
        &self.latest
    }

    pub fn poll(&mut self) -> FinalityReport {
        self.polls = self.polls.saturating_add(1);
        let mut failovers = Vec::new();
        match self.observe(&mut failovers) {
            Ok((head, stage, evidence)) => {
                let unchanged = self
                    .last_observation
                    .is_some_and(|(last_head, last_stage)| {
                        last_head == head && last_stage == stage
                    });
                if matches!(stage, FinalityStage::Final { .. }) || !unchanged {
                    self.stalled_polls = 0;
                } else {
                    self.stalled_polls = self.stalled_polls.saturating_add(1);
                }
                self.last_observation = Some((head, stage));
                let signal = if self.stalled_polls >= self.delayed_after_polls {
                    ChainSignal::Delayed {
                        stalled_polls: self.stalled_polls,
                        threshold: self.delayed_after_polls,
                        stalled_for: self.cadence_window(self.stalled_polls),
                        delayed_after: self.cadence_window(self.delayed_after_polls),
                    }
                } else {
                    ChainSignal::Progressing
                };
                let endpoint = if failovers.is_empty() {
                    EndpointSignal::Serving
                } else {
                    EndpointSignal::Degraded { failovers }
                };
                self.latest = FinalityReport {
                    transaction: self.transaction,
                    stage,
                    signal,
                    endpoint,
                    progress: self.progress_of(stage),
                    displacements: self.displacements,
                    polls: self.polls,
                    evidence: Some(evidence),
                };
            }
            Err(error) => {
                let stage = self
                    .last_observation
                    .map_or(FinalityStage::Announced, |(_, stage)| stage);
                self.latest = FinalityReport {
                    transaction: self.transaction,
                    stage,
                    signal: ChainSignal::Unreachable {
                        error: error.clone(),
                    },
                    endpoint: EndpointSignal::Unreachable { error },
                    progress: self.progress_of(stage),
                    displacements: self.displacements,
                    polls: self.polls,
                    evidence: None,
                };
            }
        }
        self.latest.clone()
    }

    fn progress_of(&self, stage: FinalityStage) -> ConfirmationProgress {
        let confirmed = match stage {
            FinalityStage::Confirming { confirmations, .. }
            | FinalityStage::Final { confirmations, .. } => confirmations,
            FinalityStage::Announced
            | FinalityStage::Missing { .. }
            | FinalityStage::Pooled { .. }
            | FinalityStage::Displaced { .. } => 0,
        };
        ConfirmationProgress {
            confirmed,
            required: self.required_confirmations,
        }
    }

    fn cadence_window(&self, polls: u64) -> Duration {
        self.poll_cadence
            .saturating_mul(u32::try_from(polls).unwrap_or(u32::MAX))
    }

    fn observe(
        &mut self,
        failovers: &mut Vec<EndpointFailure>,
    ) -> Result<(u64, FinalityStage, FinalityEvidence), EndpointError> {
        let (observation, mut dissent) = self
            .client
            .agreed_finality_observation(self.transaction, self.minimum_endpoint_agreement)?;
        failovers.append(&mut dissent);
        let head = observation.head;
        let evidence = FinalityEvidence::from_observation(
            self.client.quorum_binding(self.minimum_endpoint_agreement),
            &observation,
        );
        let stage = match observation.transaction {
            TransactionView::Included(included) => {
                let canonical = observation.canonical_block;
                if canonical.is_some_and(|block| block.hash == included.block.hash) {
                    if let Some(prior) = self.recorded {
                        if prior.block.hash != included.block.hash {
                            self.displacements = self.displacements.saturating_add(1);
                        }
                    }
                    self.recorded = Some(included);
                    self.lost = None;
                    let confirmations =
                        head.saturating_sub(included.block.number).saturating_add(1);
                    if confirmations >= self.required_confirmations {
                        FinalityStage::Final {
                            inclusion: included,
                            confirmations,
                            required: self.required_confirmations,
                        }
                    } else {
                        FinalityStage::Confirming {
                            inclusion: included,
                            confirmations,
                            required: self.required_confirmations,
                        }
                    }
                } else {
                    let lost = self.displace(Some(included)).unwrap_or(included);
                    FinalityStage::Displaced {
                        lost,
                        head,
                        requeued: false,
                    }
                }
            }
            TransactionView::Pending => match self.displace(None) {
                Some(lost) => FinalityStage::Displaced {
                    lost,
                    head,
                    requeued: true,
                },
                None => FinalityStage::Pooled { head },
            },
            TransactionView::Unknown => match self.displace(None) {
                Some(lost) => FinalityStage::Displaced {
                    lost,
                    head,
                    requeued: false,
                },
                None => FinalityStage::Missing { head },
            },
        };
        Ok((head, stage, evidence))
    }

    fn displace(
        &mut self,
        reference: Option<TransactionInclusion>,
    ) -> Option<TransactionInclusion> {
        if let Some(prior) = self.recorded.take() {
            self.displacements = self.displacements.saturating_add(1);
            self.lost = Some(prior);
        } else if self.lost.is_none() {
            if let Some(fallback) = reference {
                self.displacements = self.displacements.saturating_add(1);
                self.lost = Some(fallback);
            }
        }
        self.lost
    }
}

pub(crate) fn validate_endpoint_agreement(
    endpoints: &[EndpointConfig],
    minimum_endpoint_agreement: usize,
) -> Result<(), TrackerConfigError> {
    if minimum_endpoint_agreement == 0 {
        return Err(TrackerConfigError::ZeroEndpointAgreement);
    }
    if minimum_endpoint_agreement > endpoints.len() {
        return Err(TrackerConfigError::InsufficientEndpointAgreement);
    }
    let production = endpoints
        .iter()
        .any(|endpoint| matches!(&endpoint.transport, EndpointTransport::PinnedTls { .. }));
    if production && minimum_endpoint_agreement < 2 {
        return Err(TrackerConfigError::ProductionAgreementRequiresTwo);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deposit::{
        DepositFailure, DepositProofConfig, DepositProofConfigError, DepositProofVerifier,
        ProofFault,
    };
    use crate::rpc::EndpointFault;
    use crate::status::{BoundaryHealth, BoundaryStatus};

    fn endpoint(port: u16) -> EndpointConfig {
        EndpointConfig {
            url: format!("http://127.0.0.1:{port}"),
            request_timeout: Duration::from_secs(1),
            transport: EndpointTransport::LocalEmulator,
            expected_chain_id: 31_337,
        }
    }

    fn final_report(
        binding: QuorumBinding,
        reported_confirmations: u64,
        reported_required: u64,
        observed_head: u64,
        chain_id: u64,
    ) -> FinalityReport {
        let inclusion = TransactionInclusion {
            block: BlockRef {
                number: 10,
                hash: [3; 32],
            },
            transaction_index: 0,
            execution: crate::client::ExecutionOutcome::Succeeded,
            deployed_contract: None,
        };
        FinalityReport {
            transaction: TransactionHash::new([7; 32]),
            stage: FinalityStage::Final {
                inclusion,
                confirmations: reported_confirmations,
                required: reported_required,
            },
            signal: ChainSignal::Progressing,
            endpoint: EndpointSignal::Serving,
            progress: ConfirmationProgress {
                confirmed: reported_confirmations,
                required: reported_required,
            },
            displacements: 0,
            polls: 1,
            evidence: Some(FinalityEvidence {
                binding,
                chain_id,
                head: observed_head,
                transaction: TransactionView::Included(inclusion),
                canonical_block: Some(inclusion.block),
                receipt_logs: Some(Vec::new()),
            }),
        }
    }

    fn deposit_config(endpoints: &[EndpointConfig]) -> DepositProofConfig {
        let authority = ed25519_dalek::SigningKey::from_bytes(&[9; 32]);
        DepositProofConfig {
            endpoints: endpoints.to_vec(),
            minimum_endpoint_agreement: 2,
            required_confirmations: 12,
            paxeer_chain_id: 31_337,
            paxeer_checkpoint_authority: authority.verifying_key().to_bytes(),
            custody_reference: [10; 32],
            layerx_network_id: 17,
            layerx_protocol_version: 1,
        }
    }

    fn deposit_verifier(endpoints: &[EndpointConfig]) -> DepositProofVerifier {
        DepositProofVerifier::new(deposit_config(endpoints))
            .unwrap_or_else(|error| panic!("deposit verifier: {error:?}"))
    }

    #[test]
    fn unreachable_report_maps_to_unavailable_boundary_health() {
        let error = EndpointError {
            failures: vec![EndpointFailure {
                url: "http://127.0.0.1:1".to_owned(),
                fault: EndpointFault::Connect {
                    detail: "connection refused".to_owned(),
                },
            }],
        };
        let report = FinalityReport {
            transaction: TransactionHash::new([7; 32]),
            stage: FinalityStage::Missing { head: 41 },
            signal: ChainSignal::Unreachable {
                error: error.clone(),
            },
            endpoint: EndpointSignal::Unreachable { error },
            progress: ConfirmationProgress {
                confirmed: 0,
                required: 3,
            },
            displacements: 0,
            polls: 2,
            evidence: None,
        };

        assert_eq!(
            BoundaryStatus::from_report(&report, Duration::from_secs(2)).health,
            BoundaryHealth::Unavailable
        );
    }

    #[test]
    fn deposit_proof_refuses_report_from_weaker_endpoint_quorum() {
        let endpoints = vec![endpoint(18_545), endpoint(18_546)];
        let verifier = deposit_verifier(&endpoints);
        let client =
            PaxeerClient::new(endpoints).unwrap_or_else(|error| panic!("Paxeer client: {error:?}"));
        let report = final_report(client.quorum_binding(1), 12, 12, 21, 31_337);

        assert_eq!(
            verifier.verify_report_policy(&report),
            Err(DepositFailure::ProofUnavailable(
                ProofFault::EvidenceSourceMismatch,
            ))
        );
    }

    #[test]
    fn deposit_proof_config_refuses_zero_or_endpoint_mismatched_chain_identity() {
        let endpoints = vec![endpoint(18_551), endpoint(18_552)];
        let mut zero = deposit_config(&endpoints);
        zero.paxeer_chain_id = 0;
        assert_eq!(
            DepositProofVerifier::new(zero).unwrap_err(),
            DepositProofConfigError::ZeroPaxeerChainId
        );

        let mut mismatched = deposit_config(&endpoints);
        mismatched.paxeer_chain_id = 1;
        assert_eq!(
            DepositProofVerifier::new(mismatched).unwrap_err(),
            DepositProofConfigError::EndpointChainIdMismatch {
                expected: 1,
                found: 31_337,
            }
        );
    }

    #[test]
    fn deposit_proof_refuses_quorum_evidence_from_another_chain() {
        let endpoints = vec![endpoint(18_553), endpoint(18_554)];
        let verifier = deposit_verifier(&endpoints);
        let client =
            PaxeerClient::new(endpoints).unwrap_or_else(|error| panic!("Paxeer client: {error:?}"));
        let report = final_report(client.quorum_binding(2), 12, 12, 21, 1);

        assert_eq!(
            verifier.verify_report_policy(&report),
            Err(DepositFailure::ProofUnavailable(
                ProofFault::FinalityChainIdMismatch {
                    expected: 31_337,
                    found: 1,
                },
            ))
        );
    }

    #[test]
    fn deposit_proof_refuses_report_from_weaker_confirmation_policy() {
        let endpoints = vec![endpoint(18_547), endpoint(18_548)];
        let verifier = deposit_verifier(&endpoints);
        let client =
            PaxeerClient::new(endpoints).unwrap_or_else(|error| panic!("Paxeer client: {error:?}"));
        let report = final_report(client.quorum_binding(2), 12, 1, 21, 31_337);

        assert_eq!(
            verifier.verify_report_policy(&report),
            Err(DepositFailure::ProofUnavailable(
                ProofFault::ConfirmationPolicyMismatch {
                    expected: 12,
                    reported: 1,
                },
            ))
        );
    }

    #[test]
    fn deposit_proof_recomputes_confirmation_depth_from_quorum_evidence() {
        let endpoints = vec![endpoint(18_549), endpoint(18_550)];
        let verifier = deposit_verifier(&endpoints);
        let client =
            PaxeerClient::new(endpoints).unwrap_or_else(|error| panic!("Paxeer client: {error:?}"));
        let report = final_report(client.quorum_binding(2), 12, 12, 10, 31_337);

        assert_eq!(
            verifier.verify_report_policy(&report),
            Err(DepositFailure::ProofUnavailable(
                ProofFault::ConfirmationEvidenceMismatch {
                    reported: 12,
                    observed: 1,
                },
            ))
        );
    }
}
