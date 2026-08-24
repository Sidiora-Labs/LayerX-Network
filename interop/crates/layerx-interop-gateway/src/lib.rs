#![forbid(unsafe_code)]

pub mod adapter;
pub mod audit;
mod codec;
pub mod error;
pub mod gateway;
pub mod principal;
pub mod redaction;
pub mod server;
pub mod trace;

pub use gateway::GatewayCore;

/// The immutable workspace boundary of the interoperability plane: adapters
/// translate at the edge only, and the plane's payload authorities remain the
/// sole constructors of `LayerX` payload bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InteropWorkspaceManifest {
    /// Interop-plane crates in build order.
    pub crates: [&'static str; 2],
    /// The only contract through which the interop plane reaches `LayerX`.
    pub core_boundary: &'static str,
}

/// Returns the build-time declaration of the interop workspace boundary.
#[must_use]
pub const fn interop_workspace_manifest() -> InteropWorkspaceManifest {
    InteropWorkspaceManifest {
        crates: ["layerx-interop-gateway", "layerx-interop-service"],
        core_boundary: "layerx-agent-api",
    }
}

/// Returns an empty gateway core: no adapters registered, no principal state,
/// genesis-linked audit chains.
#[must_use]
pub fn interop_gateway_core() -> GatewayCore {
    GatewayCore::new()
}

#[cfg(test)]
mod tests {
    use super::{interop_gateway_core, interop_workspace_manifest};

    #[test]
    fn declares_agent_contract_as_the_only_core_boundary() {
        assert_eq!(
            interop_workspace_manifest().core_boundary,
            "layerx-agent-api"
        );
    }

    #[test]
    fn a_fresh_core_holds_no_adapters() {
        assert!(interop_gateway_core().adapter_ids().is_empty());
    }
}
