//! Construction boundary for MCP deployments that cannot emit write invocations.

use std::path::Path;

use layerx_agentd::capability::CapabilityId;
use layerx_agentd::session::SessionCredential;
use layerx_agentd::session_control::SessionControl;

use crate::server::{
    CapabilityDeclaration, DaemonInvocation, DeploymentMode, InvocationOutcome, ScopeBinding,
    Server, ServerError, ToolDefinition, ToolKind,
};

/// Opaque read-only MCP server. Its inner full server is never exposed.
pub struct ReadOnly {
    server: Server,
}

impl ReadOnly {
    /// Binds a read-only server from daemon-owned records.
    ///
    /// # Errors
    ///
    /// Refuses invalid authority or a scope set containing no reachable read tool.
    pub fn bind(
        control: SessionControl,
        credential: SessionCredential,
        capability_id: CapabilityId,
        core_sequence: u64,
        audit_root: impl AsRef<Path>,
    ) -> Result<Self, ServerError> {
        Server::bind_for_mode(
            control,
            credential,
            capability_id,
            core_sequence,
            audit_root,
            DeploymentMode::ReadOnly,
        )
        .map(|server| Self { server })
    }

    #[must_use]
    pub const fn binding(&self) -> &ScopeBinding {
        self.server.binding()
    }

    #[must_use]
    pub fn tools(&self) -> &[ToolDefinition] {
        self.server.tools()
    }

    #[must_use]
    pub fn capability_declaration(&self) -> CapabilityDeclaration {
        self.server.capability_declaration()
    }

    /// Runs a read executor only between route authorization and completion reauthorization.
    ///
    /// # Errors
    ///
    /// Refuses every absent or non-read definition before an invocation can be emitted, and a
    /// closed or revoked bound session at the choke point.
    pub fn execute_authorized<T, F>(
        &mut self,
        core_sequence: u64,
        name: &str,
        arguments: Vec<u8>,
        executor: F,
    ) -> Result<T, ServerError>
    where
        F: FnOnce(&DaemonInvocation) -> (T, InvocationOutcome),
    {
        if self
            .server
            .tool(name)
            .is_none_or(|tool| tool.kind != ToolKind::Read || tool.mutation != "none")
        {
            return Err(ServerError::ToolAbsent);
        }
        self.server
            .execute_read(core_sequence, name, arguments, executor)
    }

    #[must_use]
    pub const fn audit_entries(&self) -> u64 {
        self.server.audit_entries()
    }
}
