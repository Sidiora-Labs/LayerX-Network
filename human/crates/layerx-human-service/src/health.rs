use layerx_paxeer_client::{BoundaryHealth, BoundaryStatus};

use crate::custody::{Availability, CustodyStatus, KeyReferenceIntegrity, RotationState};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ComponentHealth {
    Ready,
    Degraded { reason: String },
    Unavailable { reason: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HealthComponent {
    HumanService,
    AgentLayer,
    Paxeer,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceHealth {
    pub human_service: ComponentHealth,
    pub agent_layer: ComponentHealth,
    pub paxeer: BoundaryStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceReadiness {
    pub ready: bool,
    pub unavailable: Vec<HealthComponent>,
}

/// Redacted custody details safe for status responses, logs and metrics. The
/// model deliberately carries no provider reference, key identifier, payload
/// or principal value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedactedCustodyHealth {
    pub provider: ComponentHealth,
    pub storage: ComponentHealth,
    pub key_references: ComponentHealth,
    pub rotation: ComponentHealth,
}

impl RedactedCustodyHealth {
    /// Projects internal custody status into fixed, non-secret reason strings.
    #[must_use]
    pub fn from_status(status: CustodyStatus) -> Self {
        Self {
            provider: match status.kms {
                Availability::Available => ComponentHealth::Ready,
                Availability::Unavailable => ComponentHealth::Degraded {
                    reason: "remote custody provider unavailable".to_owned(),
                },
            },
            storage: match status.storage {
                Availability::Available => ComponentHealth::Ready,
                Availability::Unavailable => ComponentHealth::Unavailable {
                    reason: "custody record storage unavailable".to_owned(),
                },
            },
            key_references: match status.key_references {
                KeyReferenceIntegrity::Verified => ComponentHealth::Ready,
                KeyReferenceIntegrity::Failed => ComponentHealth::Unavailable {
                    reason: "custody key-reference integrity failed".to_owned(),
                },
                KeyReferenceIntegrity::Unknown => ComponentHealth::Degraded {
                    reason: "custody key-reference integrity is unknown".to_owned(),
                },
            },
            rotation: match status.rotation {
                RotationState::Stable => ComponentHealth::Ready,
                RotationState::InProgress => ComponentHealth::Degraded {
                    reason: "custody key rotation is in progress".to_owned(),
                },
                RotationState::Failed => ComponentHealth::Degraded {
                    reason: "custody key rotation failed".to_owned(),
                },
                RotationState::Unknown => ComponentHealth::Degraded {
                    reason: "custody key rotation state is unknown".to_owned(),
                },
            },
        }
    }

    fn service_component(&self) -> ComponentHealth {
        for component in [
            &self.storage,
            &self.key_references,
            &self.provider,
            &self.rotation,
        ] {
            if matches!(component, ComponentHealth::Unavailable { .. }) {
                return component.clone();
            }
        }
        for component in [
            &self.provider,
            &self.key_references,
            &self.rotation,
            &self.storage,
        ] {
            if matches!(component, ComponentHealth::Degraded { .. }) {
                return component.clone();
            }
        }
        ComponentHealth::Ready
    }
}

impl ServiceHealth {
    /// Merges custody readiness into the existing human-service component
    /// while retaining a separately consumable redacted custody projection.
    #[must_use]
    pub fn with_custody_status(mut self, status: CustodyStatus) -> Self {
        let custody = RedactedCustodyHealth::from_status(status).service_component();
        self.human_service = merge_component(self.human_service, custody);
        self
    }

    #[must_use]
    pub fn readiness(&self) -> ServiceReadiness {
        let mut unavailable = Vec::new();
        if matches!(self.human_service, ComponentHealth::Unavailable { .. }) {
            unavailable.push(HealthComponent::HumanService);
        }
        if matches!(self.agent_layer, ComponentHealth::Unavailable { .. }) {
            unavailable.push(HealthComponent::AgentLayer);
        }
        if self.paxeer.health == BoundaryHealth::Unavailable {
            unavailable.push(HealthComponent::Paxeer);
        }
        ServiceReadiness {
            ready: unavailable.is_empty(),
            unavailable,
        }
    }
}

fn merge_component(current: ComponentHealth, custody: ComponentHealth) -> ComponentHealth {
    match (&current, &custody) {
        (ComponentHealth::Unavailable { .. }, _)
        | (_, ComponentHealth::Ready)
        | (ComponentHealth::Degraded { .. }, ComponentHealth::Degraded { .. }) => current,
        (_, ComponentHealth::Unavailable { .. })
        | (ComponentHealth::Ready, ComponentHealth::Degraded { .. }) => custody,
    }
}
