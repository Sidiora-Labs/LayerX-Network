use std::time::Duration;

use layerx_human_service::health::{
    ComponentHealth, HealthComponent, ServiceHealth, ServiceReadiness,
};
use layerx_paxeer_client::{
    BoundaryHealth, BoundaryStatus, ChainStatus, ContractStatus, DelayExpectation, EndpointError,
    EndpointFailure, EndpointFault, EndpointStatus, FinalityStage, TransactionHash,
};

fn status(endpoint: EndpointStatus, chain: ChainStatus, health: BoundaryHealth) -> BoundaryStatus {
    BoundaryStatus {
        transaction: TransactionHash::new([7; 32]),
        stage: FinalityStage::Missing { head: 41 },
        endpoint,
        chain,
        contract: ContractStatus::NotObserved,
        health,
    }
}

fn ready_components(paxeer: BoundaryStatus) -> ServiceHealth {
    ServiceHealth {
        human_service: ComponentHealth::Ready,
        agent_layer: ComponentHealth::Ready,
        paxeer,
    }
}

#[test]
fn readiness_names_paxeer_as_the_unavailable_component() {
    let error = EndpointError {
        failures: vec![EndpointFailure {
            url: "http://127.0.0.1:1".to_owned(),
            fault: EndpointFault::Connect {
                detail: "connection refused".to_owned(),
            },
        }],
    };
    let health = ready_components(status(
        EndpointStatus::Failed { error },
        ChainStatus::Progressing,
        BoundaryHealth::Unavailable,
    ));

    assert_eq!(
        health.readiness(),
        ServiceReadiness {
            ready: false,
            unavailable: vec![HealthComponent::Paxeer],
        }
    );
}

#[test]
fn degraded_paxeer_timing_remains_ready_but_visible() {
    let health = ready_components(status(
        EndpointStatus::Serving,
        ChainStatus::FinalityDelayed {
            expectation: DelayExpectation {
                poll_cadence: Duration::from_secs(2),
                delayed_after: Duration::from_secs(6),
                stalled_for: Duration::from_secs(6),
                next_observation_within: Duration::from_secs(2),
            },
        },
        BoundaryHealth::Degraded,
    ));

    assert_eq!(
        health.readiness(),
        ServiceReadiness {
            ready: true,
            unavailable: Vec::new(),
        }
    );
    assert_eq!(
        health.paxeer.health,
        layerx_paxeer_client::BoundaryHealth::Degraded
    );
}

#[test]
fn service_and_agent_failures_are_reported_separately() {
    let health = ServiceHealth {
        human_service: ComponentHealth::Unavailable {
            reason: "store unavailable".to_owned(),
        },
        agent_layer: ComponentHealth::Unavailable {
            reason: "agent boundary unavailable".to_owned(),
        },
        paxeer: status(
            EndpointStatus::Serving,
            ChainStatus::Progressing,
            BoundaryHealth::Ready,
        ),
    };

    assert_eq!(
        health.readiness(),
        ServiceReadiness {
            ready: false,
            unavailable: vec![HealthComponent::HumanService, HealthComponent::AgentLayer],
        }
    );
}
