use std::time::Duration;

use crate::{
    ChainSignal, EndpointError, EndpointFailure, EndpointSignal, ExecutionOutcome, FinalityReport,
    FinalityStage, FinalityTracker, TransactionHash,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DelayExpectation {
    pub poll_cadence: Duration,
    pub delayed_after: Duration,
    pub stalled_for: Duration,
    pub next_observation_within: Duration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EndpointStatus {
    Serving,
    Degraded { failovers: Vec<EndpointFailure> },
    Failed { error: EndpointError },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChainStatus {
    Progressing,
    Congested { expectation: DelayExpectation },
    FinalityDelayed { expectation: DelayExpectation },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContractStatus {
    NotObserved,
    Accepted,
    Refused,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BoundaryHealth {
    Ready,
    Degraded,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundaryStatus {
    pub transaction: TransactionHash,
    pub stage: FinalityStage,
    pub endpoint: EndpointStatus,
    pub chain: ChainStatus,
    pub contract: ContractStatus,
    pub health: BoundaryHealth,
}

impl BoundaryStatus {
    #[must_use]
    pub fn from_report(report: &FinalityReport, poll_cadence: Duration) -> Self {
        let endpoint = match report.endpoint() {
            EndpointSignal::Serving => EndpointStatus::Serving,
            EndpointSignal::Degraded { failovers } => EndpointStatus::Degraded { failovers },
            EndpointSignal::Unreachable { error } => EndpointStatus::Failed { error },
        };
        let chain = match report.signal() {
            ChainSignal::Progressing | ChainSignal::Unreachable { .. } => ChainStatus::Progressing,
            ChainSignal::Delayed {
                stalled_for,
                delayed_after,
                ..
            } => {
                let expectation = DelayExpectation {
                    poll_cadence,
                    delayed_after,
                    stalled_for,
                    next_observation_within: poll_cadence,
                };
                if matches!(
                    report.stage(),
                    FinalityStage::Missing { .. } | FinalityStage::Pooled { .. }
                ) {
                    ChainStatus::Congested { expectation }
                } else {
                    ChainStatus::FinalityDelayed { expectation }
                }
            }
        };
        let contract = match current_execution(report.stage()) {
            None => ContractStatus::NotObserved,
            Some(ExecutionOutcome::Succeeded) => ContractStatus::Accepted,
            Some(ExecutionOutcome::Reverted) => ContractStatus::Refused,
        };
        let health = if matches!(endpoint, EndpointStatus::Failed { .. }) {
            BoundaryHealth::Unavailable
        } else if matches!(endpoint, EndpointStatus::Degraded { .. })
            || !matches!(chain, ChainStatus::Progressing)
        {
            BoundaryHealth::Degraded
        } else {
            BoundaryHealth::Ready
        };
        Self {
            transaction: report.transaction(),
            stage: report.stage(),
            endpoint,
            chain,
            contract,
            health,
        }
    }
}

impl FinalityTracker {
    #[must_use]
    pub fn boundary_status(&self) -> BoundaryStatus {
        BoundaryStatus::from_report(self.latest(), self.poll_cadence())
    }
}

const fn current_execution(stage: FinalityStage) -> Option<ExecutionOutcome> {
    match stage {
        FinalityStage::Confirming { inclusion, .. } | FinalityStage::Final { inclusion, .. } => {
            Some(inclusion.execution)
        }
        FinalityStage::Announced
        | FinalityStage::Missing { .. }
        | FinalityStage::Pooled { .. }
        | FinalityStage::Displaced { .. } => None,
    }
}
