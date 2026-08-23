//! Durable program deprecation and exit-only wind-down enforcement.

use core::fmt::{self, Display};

use layerx_programs_runtime::ProgramId;

use crate::{
    LifecycleReceipt, LifecycleTransition, ProgramLifecycle, Registry, RegistryError,
    WindDownPolicy, WindDownStateAccess,
};

/// One program-owned value account observed at the transition sequence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValueAccount {
    pub account_id: [u8; 32],
    pub balance: u128,
    pub exit_destination: [u8; 32],
    pub exit_authority: [u8; 32],
    pub exit_receipt_digest: [u8; 32],
    pub exit_limit: u128,
    pub exit_not_after: u64,
}

/// Authority-bearing deprecation or tombstone activity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeprecationRequest {
    pub program: ProgramId,
    pub expected: ProgramLifecycle,
    pub target: ProgramLifecycle,
    pub authority: [u8; 32],
    pub effective_sequence: u64,
    pub wind_down: WindDownPolicy,
    pub value_accounts: Vec<ValueAccount>,
}

/// Read-only wind-down state exposed after a lifecycle transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindDownView {
    pub lifecycle: ProgramLifecycle,
    pub policy: WindDownPolicy,
    pub live_value_accounts: Vec<ValueAccount>,
    pub transition_history: Vec<LifecycleReceipt>,
}

impl WindDownView {
    /// Returns the stable deprecation-status label so every surface that shows
    /// a program - agent layer, explorer, CLI - renders the same honest state.
    #[must_use]
    pub const fn status_label(&self) -> &'static str {
        match self.lifecycle {
            ProgramLifecycle::Active => "active",
            ProgramLifecycle::Deprecated => "deprecated",
            ProgramLifecycle::Tombstoned => "tombstoned",
        }
    }

    /// Reports whether the program has entered a declared wind-down, so a
    /// display can flag deprecated and tombstoned programs distinctly from
    /// active ones.
    #[must_use]
    pub const fn is_wound_down(&self) -> bool {
        matches!(
            self.lifecycle,
            ProgramLifecycle::Deprecated | ProgramLifecycle::Tombstoned
        )
    }

    /// Returns the value still held under the program's accounts at the
    /// wind-down point, each of which the accepted transition proved has an
    /// authorized exit route.
    #[must_use]
    pub fn reachable_value(&self) -> u128 {
        self.live_value_accounts
            .iter()
            .map(|account| account.balance)
            .sum()
    }
}

/// Typed refusal which prevents deprecation from stranding program value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeprecationRefusal {
    InvalidTransition,
    DeadlineElapsed,
    DuplicateAccount,
    InvalidAccount,
    ValueWouldBeStranded { account_id: [u8; 32] },
    Registry(RegistryError),
}

impl Display for DeprecationRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTransition => formatter.write_str("invalid program lifecycle transition"),
            Self::DeadlineElapsed => formatter.write_str("wind-down deadline has elapsed"),
            Self::DuplicateAccount => formatter.write_str("duplicate program value account"),
            Self::InvalidAccount => formatter.write_str("invalid program value account"),
            Self::ValueWouldBeStranded { account_id } => {
                write!(
                    formatter,
                    "program value account {account_id:02x?} has no authorized exit"
                )
            }
            Self::Registry(error) => write!(formatter, "registry refusal: {error}"),
        }
    }
}

impl std::error::Error for DeprecationRefusal {}

impl From<RegistryError> for DeprecationRefusal {
    fn from(value: RegistryError) -> Self {
        Self::Registry(value)
    }
}

/// Deprecation coordinator retaining exit-only state independently of callable
/// program code.
#[derive(Debug, Default)]
pub struct Deprecation {
    wind_downs: Vec<(ProgramId, WindDownPolicy, Vec<ValueAccount>)>,
}

impl Deprecation {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            wind_downs: Vec::new(),
        }
    }

    /// Applies a deprecation or tombstone only after every live account has a
    /// concrete authorized exit route.
    ///
    /// # Errors
    ///
    /// Refuses malformed transitions, elapsed wind-downs, duplicate accounts,
    /// stranded value, and registry state conflicts.
    pub fn transition(
        &mut self,
        registry: &mut Registry,
        request: &DeprecationRequest,
    ) -> Result<LifecycleReceipt, DeprecationRefusal> {
        validate_transition(request)?;
        let live: Vec<ValueAccount> = request
            .value_accounts
            .iter()
            .copied()
            .filter(|account| account.balance != 0)
            .collect();
        let live_count =
            u32::try_from(live.len()).map_err(|_| DeprecationRefusal::InvalidAccount)?;
        let receipt = registry.transition_lifecycle(LifecycleTransition {
            program: request.program,
            expected: request.expected,
            target: request.target,
            authority: request.authority,
            effective_sequence: request.effective_sequence,
            wind_down: request.wind_down,
            live_value_accounts: live_count,
        })?;
        match self
            .wind_downs
            .iter_mut()
            .find(|record| record.0 == request.program)
        {
            Some(record) => {
                record.1 = request.wind_down;
                record.2 = live;
            }
            None => self
                .wind_downs
                .push((request.program, request.wind_down, live)),
        }
        Ok(receipt)
    }

    /// Returns retained read-only state and complete registry transition history.
    ///
    /// # Errors
    ///
    /// Refuses programs with no accepted wind-down transition.
    pub fn read(
        &self,
        registry: &Registry,
        program: ProgramId,
    ) -> Result<WindDownView, DeprecationRefusal> {
        let record = self
            .wind_downs
            .iter()
            .find(|record| record.0 == program)
            .ok_or(DeprecationRefusal::InvalidTransition)?;
        let entry = registry
            .entry_for_wind_down(program)
            .map_err(DeprecationRefusal::Registry)?;
        Ok(WindDownView {
            lifecycle: entry.lifecycle,
            policy: record.1,
            live_value_accounts: record.2.clone(),
            transition_history: entry.lifecycle_history.clone(),
        })
    }

    /// Reconstructs both the registry lifecycle and the retained wind-down
    /// state deterministically from the durable, append-only deprecation
    /// activity log, so a deprecated or tombstoned program's history stays
    /// readable and replayable forever - including after a restart that keeps
    /// only the journalled activities and none of this coordinator's memory.
    ///
    /// Activities are applied in canonical protocol order - by program, then
    /// effective sequence - and each is revalidated through the same
    /// stranded-value guard as a live transition, so replay can never admit a
    /// path that a live deprecation would have refused.
    ///
    /// # Errors
    ///
    /// Refuses the same malformed transitions, elapsed wind-downs, duplicate
    /// accounts, stranded value and registry conflicts as [`Self::transition`],
    /// surfacing the first offending activity in the log.
    pub fn replay(
        &mut self,
        registry: &mut Registry,
        log: &[DeprecationRequest],
    ) -> Result<Vec<LifecycleReceipt>, DeprecationRefusal> {
        let mut ordered: Vec<&DeprecationRequest> = log.iter().collect();
        ordered.sort_by_key(|request| (request.program.bytes(), request.effective_sequence));
        let mut receipts = Vec::with_capacity(ordered.len());
        for request in ordered {
            receipts.push(self.transition(registry, request)?);
        }
        Ok(receipts)
    }
}

fn validate_transition(request: &DeprecationRequest) -> Result<(), DeprecationRefusal> {
    let valid_edge = matches!(
        (request.expected, request.target),
        (ProgramLifecycle::Active, ProgramLifecycle::Deprecated)
            | (ProgramLifecycle::Deprecated, ProgramLifecycle::Tombstoned)
    );
    if !valid_edge || request.wind_down.state_access != WindDownStateAccess::ReadOnly {
        return Err(DeprecationRefusal::InvalidTransition);
    }
    if request.effective_sequence >= request.wind_down.deadline {
        return Err(DeprecationRefusal::DeadlineElapsed);
    }
    let mut accounts = request.value_accounts.clone();
    accounts.sort_by_key(|account| account.account_id);
    for pair in accounts.windows(2) {
        if pair[0].account_id == pair[1].account_id {
            return Err(DeprecationRefusal::DuplicateAccount);
        }
    }
    for account in &accounts {
        if account.account_id == [0; 32] {
            return Err(DeprecationRefusal::InvalidAccount);
        }
        if account.balance != 0
            && (account.exit_destination == [0; 32]
                || account.exit_authority == [0; 32]
                || account.exit_receipt_digest == [0; 32]
                || account.exit_limit < account.balance
                || account.exit_not_after < request.wind_down.deadline)
        {
            return Err(DeprecationRefusal::ValueWouldBeStranded {
                account_id: account.account_id,
            });
        }
    }
    Ok(())
}
