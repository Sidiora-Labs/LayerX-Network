//! Managed-agent spend surface blocked on canonical protocol budget state.

use std::fmt::{Display, Formatter};

use layerx_types::verify::VerificationLevel;

use crate::store::PrincipalId;

/// Reserved copy-catalog key for the future canonical reconciliation result.
pub const RECONCILIATION_COPY_KEY: &str = "agent.spend.protocol-adopted";
/// Reserved reconciliation note for both shells once canonical state exists.
pub const RECONCILIATION_EXPLANATION: &str =
    "Recent verified activity and the network budget differed. The network figure is shown.";

/// The two native renderers consuming the same Human API agent record.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum AgentShell {
    Mobile,
    Desktop,
}

/// Immutable identity and protocol-account facts for one managed agent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpendProfile {
    pub principal: PrincipalId,
    pub agent_id: [u8; 32],
    pub agent_account: [u8; 32],
    pub budget_id: [u8; 32],
    pub asset: [u8; 32],
}

impl SpendProfile {
    fn validate(&self) -> Result<(), SpendError> {
        if self.agent_id == [0; 32]
            || self.agent_account == [0; 32]
            || self.budget_id == [0; 32]
            || self.asset == [0; 32]
        {
            return Err(SpendError::InvalidProfile);
        }
        Ok(())
    }
}

/// Direction of a visible receipt/protocol discrepancy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReconciliationDirection {
    ProtocolHigher,
    ProtocolLower,
}

/// Honest status of the local receipt calculation against protocol authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpendReconciliationStatus {
    InSync,
    ProtocolAdopted {
        direction: ReconciliationDirection,
        difference: u128,
    },
}

/// Future output schema for current-period spend after canonical integration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentSpendView {
    agent_id: [u8; 32],
    period_start: u64,
    period_end: u64,
    /// Authoritative figure to be shown only after adopting verified protocol
    /// budget state and comparing it with receipt-only accounting.
    spent: u128,
    /// Independently recomputed total from verified current-period receipts.
    receipt_spent: u128,
    limit: u128,
    remaining: u128,
    receipt_count: usize,
    verification_level: VerificationLevel,
    observed_head_sequence: u64,
    reconciliation: SpendReconciliationStatus,
    reconciliation_copy_key: Option<&'static str>,
    reconciliation_explanation: Option<&'static str>,
    evidence_digests: Vec<[u8; 32]>,
}

impl AgentSpendView {
    #[must_use]
    pub const fn agent_id(&self) -> [u8; 32] {
        self.agent_id
    }

    #[must_use]
    pub const fn period_start(&self) -> u64 {
        self.period_start
    }

    #[must_use]
    pub const fn period_end(&self) -> u64 {
        self.period_end
    }

    #[must_use]
    pub const fn spent(&self) -> u128 {
        self.spent
    }

    #[must_use]
    pub const fn receipt_spent(&self) -> u128 {
        self.receipt_spent
    }

    #[must_use]
    pub const fn limit(&self) -> u128 {
        self.limit
    }

    #[must_use]
    pub const fn remaining(&self) -> u128 {
        self.remaining
    }

    #[must_use]
    pub const fn receipt_count(&self) -> usize {
        self.receipt_count
    }

    #[must_use]
    pub const fn verification_level(&self) -> VerificationLevel {
        self.verification_level
    }

    #[must_use]
    pub const fn observed_head_sequence(&self) -> u64 {
        self.observed_head_sequence
    }

    #[must_use]
    pub const fn reconciliation(&self) -> SpendReconciliationStatus {
        self.reconciliation
    }

    #[must_use]
    pub const fn reconciliation_copy_key(&self) -> Option<&'static str> {
        self.reconciliation_copy_key
    }

    #[must_use]
    pub const fn reconciliation_explanation(&self) -> Option<&'static str> {
        self.reconciliation_explanation
    }

    #[must_use]
    pub fn evidence_digests(&self) -> &[[u8; 32]] {
        &self.evidence_digests
    }
}

/// Future shell-tagged projection of the Human API agent-spend contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellAgentSpend {
    shell: AgentShell,
    spend: AgentSpendView,
}

impl ShellAgentSpend {
    #[must_use]
    pub const fn shell(&self) -> AgentShell {
        self.shell
    }

    #[must_use]
    pub const fn spend(&self) -> &AgentSpendView {
        &self.spend
    }
}

/// Future native shell projections derived from one atomic snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentSpendSurfaces {
    mobile: ShellAgentSpend,
    desktop: ShellAgentSpend,
}

impl AgentSpendSurfaces {
    #[must_use]
    pub const fn mobile(&self) -> &ShellAgentSpend {
        &self.mobile
    }

    #[must_use]
    pub const fn desktop(&self) -> &ShellAgentSpend {
        &self.desktop
    }
}

/// Principal-scoped spend service which refuses reads until the daemon can
/// supply an opaque canonical protocol budget record and proof.
pub struct SpendReconciliation {
    profile: SpendProfile,
}

impl SpendReconciliation {
    /// Binds the service to one human principal and one managed-agent account.
    ///
    /// # Errors
    ///
    /// Rejects zero protocol identifiers.
    pub fn new(profile: SpendProfile) -> Result<Self, SpendError> {
        profile.validate()?;
        Ok(Self { profile })
    }

    /// Returns one shell projection. The requesting principal must be the
    /// human owner bound when the service was constructed.
    ///
    /// # Errors
    ///
    /// Refuses cross-principal access and otherwise returns
    /// `ProtocolBudgetStateUnavailable` until the core defines the canonical
    /// budget state record, state key, and proof path.
    pub fn for_shell(
        &self,
        principal: &PrincipalId,
        shell: AgentShell,
    ) -> Result<ShellAgentSpend, SpendError> {
        Ok(ShellAgentSpend {
            shell,
            spend: self.reconcile(principal)?,
        })
    }

    /// Returns both shell projections only after canonical budget-state
    /// integration becomes available.
    ///
    /// # Errors
    ///
    /// Returns the same typed failures as [`Self::for_shell`].
    pub fn both_shells(&self, principal: &PrincipalId) -> Result<AgentSpendSurfaces, SpendError> {
        let spend = self.reconcile(principal)?;
        Ok(AgentSpendSurfaces {
            mobile: ShellAgentSpend {
                shell: AgentShell::Mobile,
                spend: spend.clone(),
            },
            desktop: ShellAgentSpend {
                shell: AgentShell::Desktop,
                spend,
            },
        })
    }

    fn reconcile(&self, principal: &PrincipalId) -> Result<AgentSpendView, SpendError> {
        if principal != &self.profile.principal {
            return Err(SpendError::WrongPrincipal);
        }
        Err(SpendError::ProtocolBudgetStateUnavailable)
    }
}

/// Typed spend read failure. No failure carries a partially trusted view.
#[derive(Debug)]
pub enum SpendError {
    InvalidProfile,
    WrongPrincipal,
    ProtocolBudgetStateUnavailable,
}

impl Display for SpendError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidProfile => formatter.write_str("managed-agent spend profile is invalid"),
            Self::WrongPrincipal => formatter.write_str("agent spend belongs to another principal"),
            Self::ProtocolBudgetStateUnavailable => formatter.write_str(
                "canonical protocol budget state is unavailable; managed-agent spend is blocked",
            ),
        }
    }
}

impl std::error::Error for SpendError {}
