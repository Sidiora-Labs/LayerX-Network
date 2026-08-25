use std::time::Duration;

use crate::client::{
    ClientConfigError, EndpointError, PaxeerClient, TransactionHash, TransactionInclusion,
    TransactionView,
};
use crate::rpc::{EndpointConfig, EndpointFailure, EndpointTransport};

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
    Unreachable { error: EndpointError },
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
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FinalityReport {
    pub transaction: TransactionHash,
    pub stage: FinalityStage,
    pub signal: ChainSignal,
    pub endpoint: EndpointSignal,
    pub progress: ConfirmationProgress,
    pub displacements: u64,
    pub polls: u64,
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
        let client = PaxeerClient::new(config.endpoints.clone())
            .map_err(TrackerConfigError::Endpoints)?;
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
            Ok((head, stage)) => {
                let unchanged = self
                    .last_observation
                    .is_some_and(|(last_head, last_stage)| last_head == head && last_stage == stage);
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
    ) -> Result<(u64, FinalityStage), EndpointError> {
        let (observation, mut dissent) = self
            .client
            .agreed_finality_observation(self.transaction, self.minimum_endpoint_agreement)?;
        failovers.append(&mut dissent);
        let head = observation.head;
        match observation.transaction {
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
                    let confirmations = head
                        .saturating_sub(included.block.number)
                        .saturating_add(1);
                    let stage = if confirmations >= self.required_confirmations {
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
                    };
                    Ok((head, stage))
                } else {
                    let lost = self.displace(Some(included)).unwrap_or(included);
                    Ok((
                        head,
                        FinalityStage::Displaced {
                            lost,
                            head,
                            requeued: false,
                        },
                    ))
                }
            }
            TransactionView::Pending => {
                let stage = match self.displace(None) {
                    Some(lost) => FinalityStage::Displaced {
                        lost,
                        head,
                        requeued: true,
                    },
                    None => FinalityStage::Pooled { head },
                };
                Ok((head, stage))
            }
            TransactionView::Unknown => {
                let stage = match self.displace(None) {
                    Some(lost) => FinalityStage::Displaced {
                        lost,
                        head,
                        requeued: false,
                    },
                    None => FinalityStage::Missing { head },
                };
                Ok((head, stage))
            }
        }
    }

    fn displace(&mut self, reference: Option<TransactionInclusion>) -> Option<TransactionInclusion> {
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
