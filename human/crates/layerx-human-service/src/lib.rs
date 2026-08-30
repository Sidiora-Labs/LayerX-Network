#![forbid(unsafe_code)]

pub mod activity;
pub mod agents;
pub mod approvals;
pub mod audit;
pub mod auth;
pub mod binding;
pub mod custody;
pub mod health;
pub mod journeys;
pub mod notify;
pub mod onboarding;
pub mod redaction;
pub mod security;
pub mod server;
pub mod store;
pub mod support;
mod time;
pub mod trace;

/// The immutable workspace boundary consumed by the human control plane.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HumanWorkspaceManifest {
    /// Human-plane crates in build order.
    pub crates: [&'static str; 4],
    /// The only contract through which the human plane reaches `LayerX`.
    pub core_boundary: &'static str,
}

/// Returns the build-time declaration of the human workspace boundary.
#[must_use]
pub const fn human_workspace_manifest() -> HumanWorkspaceManifest {
    HumanWorkspaceManifest {
        crates: [
            "layerx-intents",
            "layerx-human-service",
            "layerx-paxeer-client",
            "layerx-explorer-index",
        ],
        core_boundary: "layerx-agent-api",
    }
}

#[cfg(test)]
mod tests {
    use super::human_workspace_manifest;

    #[test]
    fn declares_agent_contract_as_the_only_core_boundary() {
        assert_eq!(human_workspace_manifest().core_boundary, "layerx-agent-api");
    }
}
