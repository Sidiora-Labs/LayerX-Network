//! Durable program deprecation over receipt-verified protocol balances.

use core::fmt::{self, Display};

use layerx_programs_runtime::ProgramId;

const PROGRAMS_WIND_DOWN_ACTIVITY_TYPE: u32 = 0x0009_0007;
const WIND_DOWN_EXIT_OPERATION: u8 = 4;

use crate::{
    AccountStateError, AccountStateJournal, JournalAccountStateAuthority, LifecycleReceipt,
    LifecycleTransition, ProgramLifecycle, Registry, RegistryError, ValueAccount,
    VerifiedAccountSnapshot, WindDownPolicy, WindDownStateAccess,
};

/// Exit-only route retained independently of ordinary program call admission.
/// The route names no alternative balance primitive: authorization is always
/// reconstructed and consumed inside the existing candidate-v2 402LXP path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExitRoute {
    pub seed: Vec<u8>,
    pub account_id: [u8; 32],
    pub asset_id: [u8; 32],
    pub destination: [u8; 32],
}

/// Exact program-authorized exit ready for the ordinary atomic 402LXP path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorizedExit {
    pub account: ValueAccount,
    pub destination: [u8; 32],
    pub protocol_activity: WindDownExitActivity,
}

/// Exact consensus activity which consumes a retained exit route. The payload
/// deliberately contains neither destination nor amount: the Programs module
/// loads the durable destination and transfers the full current account
/// balance inside the same atomic transition which verifies it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindDownExitActivity {
    pub activity_type: u32,
    pub payload: Vec<u8>,
}

impl WindDownExitActivity {
    fn new(program: ProgramId, account_id: [u8; 32]) -> Self {
        let mut payload = Vec::with_capacity(65);
        payload.extend_from_slice(&program.bytes());
        payload.push(WIND_DOWN_EXIT_OPERATION);
        payload.extend_from_slice(&account_id);
        Self {
            activity_type: PROGRAMS_WIND_DOWN_ACTIVITY_TYPE,
            payload,
        }
    }
}

/// Authority-bearing deprecation or tombstone activity. The account snapshot
/// is protocol evidence, not caller-declared bookkeeping, and is retained in
/// the append-only activity so historical replay rechecks the same roots.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeprecationRequest {
    pub program: ProgramId,
    pub expected: ProgramLifecycle,
    pub target: ProgramLifecycle,
    pub authority: [u8; 32],
    pub effective_sequence: u64,
    pub wind_down: WindDownPolicy,
    pub exits: Vec<ExitRoute>,
    pub account_snapshot: VerifiedAccountSnapshot,
}

/// Frozen ABI-one lifecycle record. ABI one could not register program-owned
/// accounts, so its only sound compatibility path is an explicitly empty
/// program-account set; caller-declared legacy balances are never imported.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LegacyDeprecationRequest {
    pub program: ProgramId,
    pub expected: ProgramLifecycle,
    pub target: ProgramLifecycle,
    pub authority: [u8; 32],
    pub effective_sequence: u64,
    pub wind_down: WindDownPolicy,
}

/// Read-only wind-down state with balances resolved at the supplied verified
/// account-tree head rather than copied from the deprecation activity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindDownView {
    pub lifecycle: ProgramLifecycle,
    pub policy: WindDownPolicy,
    pub value_accounts: Vec<ValueAccount>,
    pub transition_history: Vec<LifecycleReceipt>,
}

impl WindDownView {
    #[must_use]
    pub const fn status_label(&self) -> &'static str {
        match self.lifecycle {
            ProgramLifecycle::Active => "active",
            ProgramLifecycle::Deprecated => "deprecated",
            ProgramLifecycle::Tombstoned => "tombstoned",
        }
    }

    #[must_use]
    pub const fn is_wound_down(&self) -> bool {
        matches!(
            self.lifecycle,
            ProgramLifecycle::Deprecated | ProgramLifecycle::Tombstoned
        )
    }

    /// Returns the sum of current proof-backed balances, or `None` rather than
    /// wrapping if distinct asset quantities exceed `u128` when aggregated for
    /// display. Per-asset balances remain available in `value_accounts`.
    #[must_use]
    pub fn reachable_value(&self) -> Option<u128> {
        self.value_accounts
            .iter()
            .try_fold(0_u128, |total, account| total.checked_add(account.balance))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeprecationRefusal {
    InvalidTransition,
    InvalidExitProgram,
    DeadlineElapsed,
    SnapshotSequenceMismatch,
    DuplicateExit,
    MissingExit { account_id: [u8; 32] },
    ExitMismatch { account_id: [u8; 32] },
    ValueWouldBeStranded { account_id: [u8; 32] },
    ExitAmountMismatch,
    LegacyValueStateUnsupported,
    AccountState(AccountStateError),
    Registry(RegistryError),
}

impl Display for DeprecationRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTransition => formatter.write_str("invalid program lifecycle transition"),
            Self::InvalidExitProgram => formatter.write_str("wind-down exit is not owned by the deprecated program"),
            Self::DeadlineElapsed => formatter.write_str("wind-down deadline has elapsed"),
            Self::SnapshotSequenceMismatch => formatter.write_str("account snapshot is not the immediate pre-transition state"),
            Self::DuplicateExit => formatter.write_str("duplicate program value-account exit route"),
            Self::MissingExit { account_id } => write!(formatter, "program value account {account_id:02x?} has no retained exit route"),
            Self::ExitMismatch { account_id } => write!(formatter, "program value account {account_id:02x?} exit route conflicts with its durable binding"),
            Self::ValueWouldBeStranded { account_id } => write!(formatter, "program value account {account_id:02x?} cannot execute its retained 402LXP exit"),
            Self::ExitAmountMismatch => formatter.write_str("exit must move the exact currently verified balance"),
            Self::LegacyValueStateUnsupported => formatter.write_str("legacy program has value-account state which ABI one cannot prove"),
            Self::AccountState(error) => write!(formatter, "account-state refusal: {error}"),
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

impl From<AccountStateError> for DeprecationRefusal {
    fn from(value: AccountStateError) -> Self {
        Self::AccountState(value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct WindDownRecord {
    program: ProgramId,
    policy: WindDownPolicy,
    exits: Vec<ExitRoute>,
}

/// Deprecation coordinator retaining exit routes, never cached balances.
#[derive(Clone, Debug, Default)]
pub struct Deprecation {
    wind_downs: Vec<WindDownRecord>,
}

impl Deprecation {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            wind_downs: Vec::new(),
        }
    }

    /// Applies a lifecycle transition atomically after resolving every durable
    /// derived account against the immediate receipt-backed account-tree head.
    ///
    /// # Errors
    ///
    /// Refuses malformed transitions, stale roots, incomplete account
    /// enumeration, frozen funded accounts and any route which cannot produce
    /// the existing owner-frame `ProgramAuthority`.
    pub fn transition(
        &mut self,
        registry: &mut Registry,
        request: &DeprecationRequest,
        state_authority: &JournalAccountStateAuthority<impl AccountStateJournal>,
    ) -> Result<LifecycleReceipt, DeprecationRefusal> {
        self.transition_with_head(registry, request, state_authority, true)
    }

    fn transition_with_head(
        &mut self,
        registry: &mut Registry,
        request: &DeprecationRequest,
        state_authority: &JournalAccountStateAuthority<impl AccountStateJournal>,
        current: bool,
    ) -> Result<LifecycleReceipt, DeprecationRefusal> {
        validate_transition(request)?;
        let entry = registry.entry_for_wind_down(request.program)?;
        if entry.versions.last().map(|version| version.abi_version) != Some(2) {
            return Err(AccountStateError::LegacyProtocol.into());
        }
        let bindings = registry.value_account_bindings(request.program)?;
        let accounts = if current {
            request
                .account_snapshot
                .resolve_program(request.program, bindings, state_authority)?
        } else {
            request.account_snapshot.resolve_program_historical(
                request.program,
                bindings,
                state_authority,
            )?
        };
        if request
            .account_snapshot
            .freshness
            .observed_sequence
            .checked_add(1)
            != Some(request.effective_sequence)
        {
            return Err(DeprecationRefusal::SnapshotSequenceMismatch);
        }
        let exits = validate_exits(request, bindings, &accounts)?;
        if let Some(existing) = self
            .wind_downs
            .iter()
            .find(|record| record.program == request.program)
        {
            if existing.exits != exits || existing.policy != request.wind_down {
                return Err(DeprecationRefusal::ExitMismatch {
                    account_id: [0; 32],
                });
            }
        }
        let live_count = u32::try_from(
            accounts
                .iter()
                .filter(|account| account.balance != 0)
                .count(),
        )
        .map_err(|_| DeprecationRefusal::InvalidTransition)?;

        let mut candidate_registry = registry.clone();
        let mut candidate = self.clone();
        let receipt = candidate_registry.transition_lifecycle(LifecycleTransition {
            program: request.program,
            expected: request.expected,
            target: request.target,
            authority: request.authority,
            effective_sequence: request.effective_sequence,
            wind_down: request.wind_down,
            live_value_accounts: live_count,
        })?;
        match candidate
            .wind_downs
            .iter_mut()
            .find(|record| record.program == request.program)
        {
            Some(record) => {
                record.policy = request.wind_down;
                record.exits = exits;
            }
            None => candidate.wind_downs.push(WindDownRecord {
                program: request.program,
                policy: request.wind_down,
                exits,
            }),
        }
        *registry = candidate_registry;
        *self = candidate;
        Ok(receipt)
    }

    /// Preserves ABI-one lifecycle history behind an explicit version gate.
    /// ABI one had no program-account registration activity; therefore this
    /// path is admitted only while the durable enumeration is empty.
    ///
    /// # Errors
    ///
    /// Refuses ABI-two programs and any legacy record claiming value state
    /// which cannot be proven through the version-two account tree.
    pub fn transition_legacy(
        &mut self,
        registry: &mut Registry,
        request: LegacyDeprecationRequest,
    ) -> Result<LifecycleReceipt, DeprecationRefusal> {
        validate_legacy_transition(&request)?;
        let entry = registry.entry_for_wind_down(request.program)?;
        if entry.versions.last().map(|version| version.abi_version) != Some(1)
            || !entry.value_accounts.is_empty()
        {
            return Err(DeprecationRefusal::LegacyValueStateUnsupported);
        }
        let mut candidate_registry = registry.clone();
        let mut candidate = self.clone();
        let receipt = candidate_registry.transition_lifecycle(LifecycleTransition {
            program: request.program,
            expected: request.expected,
            target: request.target,
            authority: request.authority,
            effective_sequence: request.effective_sequence,
            wind_down: request.wind_down,
            live_value_accounts: 0,
        })?;
        match candidate
            .wind_downs
            .iter_mut()
            .find(|record| record.program == request.program)
        {
            Some(record) => {
                if record.policy != request.wind_down || !record.exits.is_empty() {
                    return Err(DeprecationRefusal::InvalidTransition);
                }
            }
            None => candidate.wind_downs.push(WindDownRecord {
                program: request.program,
                policy: request.wind_down,
                exits: Vec::new(),
            }),
        }
        *registry = candidate_registry;
        *self = candidate;
        Ok(receipt)
    }

    /// Resolves the current balances of a deprecated or tombstoned program
    /// from a newly verified account-tree snapshot.
    ///
    /// # Errors
    ///
    /// Refuses programs without retained wind-down state or unverified reads.
    pub fn read(
        &self,
        registry: &Registry,
        program: ProgramId,
        snapshot: &VerifiedAccountSnapshot,
        state_authority: &JournalAccountStateAuthority<impl AccountStateJournal>,
    ) -> Result<WindDownView, DeprecationRefusal> {
        let record = self
            .wind_downs
            .iter()
            .find(|record| record.program == program)
            .ok_or(DeprecationRefusal::InvalidTransition)?;
        let entry = registry.entry_for_wind_down(program)?;
        let value_accounts =
            snapshot.resolve_program(program, &entry.value_accounts, state_authority)?;
        Ok(WindDownView {
            lifecycle: entry.lifecycle,
            policy: record.policy,
            value_accounts,
            transition_history: entry.lifecycle_history.clone(),
        })
    }

    /// Reads frozen ABI-one lifecycle history. It is unavailable once a
    /// program has any derived-account binding, preventing an empty balance
    /// presentation from concealing ABI-two value.
    ///
    /// # Errors
    ///
    /// Refuses ABI-two or value-bearing account registries.
    pub fn read_legacy(
        &self,
        registry: &Registry,
        program: ProgramId,
    ) -> Result<WindDownView, DeprecationRefusal> {
        let record = self
            .wind_downs
            .iter()
            .find(|record| record.program == program)
            .ok_or(DeprecationRefusal::InvalidTransition)?;
        let entry = registry.entry_for_wind_down(program)?;
        if entry.versions.last().map(|version| version.abi_version) != Some(1)
            || !entry.value_accounts.is_empty()
            || !record.exits.is_empty()
        {
            return Err(DeprecationRefusal::LegacyValueStateUnsupported);
        }
        Ok(WindDownView {
            lifecycle: entry.lifecycle,
            policy: record.policy,
            value_accounts: Vec::new(),
            transition_history: entry.lifecycle_history.clone(),
        })
    }

    /// Reconstructs the exact exit activity and owner-frame authority for the
    /// full live balance. Consensus consumes the activity through the Programs
    /// module, which reloads the route and current balance in the same atomic
    /// 402LXP transition; the Rust value is a receipt-bound projection only.
    ///
    /// # Errors
    ///
    /// Refuses stale evidence and zero or frozen balances. A funded account's
    /// exit remains reachable after the advisory wind-down deadline.
    pub fn authorize_exit(
        &self,
        registry: &Registry,
        program: ProgramId,
        account_id: [u8; 32],
        snapshot: &VerifiedAccountSnapshot,
        state_authority: &JournalAccountStateAuthority<impl AccountStateJournal>,
    ) -> Result<AuthorizedExit, DeprecationRefusal> {
        let record = self
            .wind_downs
            .iter()
            .find(|record| record.program == program)
            .ok_or(DeprecationRefusal::InvalidTransition)?;
        let entry = registry.entry_for_wind_down(program)?;
        if !matches!(
            entry.lifecycle,
            ProgramLifecycle::Deprecated | ProgramLifecycle::Tombstoned
        ) {
            return Err(DeprecationRefusal::InvalidTransition);
        }
        let accounts = snapshot.resolve_program(program, &entry.value_accounts, state_authority)?;
        let account = accounts
            .into_iter()
            .find(|account| account.account_id == account_id)
            .ok_or(DeprecationRefusal::MissingExit { account_id })?;
        let route = record
            .exits
            .iter()
            .find(|route| route.account_id == account_id)
            .ok_or(DeprecationRefusal::MissingExit { account_id })?;
        if route.asset_id != account.asset_id {
            return Err(DeprecationRefusal::ExitMismatch { account_id });
        }
        if account.balance == 0 {
            return Err(DeprecationRefusal::ExitAmountMismatch);
        }
        if account.frozen {
            return Err(DeprecationRefusal::ValueWouldBeStranded { account_id });
        }
        Ok(AuthorizedExit {
            account,
            destination: route.destination,
            protocol_activity: WindDownExitActivity::new(program, account_id),
        })
    }

    /// Replays the append-only deprecation journal in canonical order while
    /// rechecking every historical receipt and account-tree proof.
    ///
    /// # Errors
    ///
    /// Returns the first live-path refusal encountered during replay.
    pub fn replay(
        &mut self,
        registry: &mut Registry,
        log: &[DeprecationRequest],
        state_authority: &JournalAccountStateAuthority<impl AccountStateJournal>,
    ) -> Result<Vec<LifecycleReceipt>, DeprecationRefusal> {
        let mut ordered: Vec<&DeprecationRequest> = log.iter().collect();
        ordered.sort_by_key(|request| (request.program.bytes(), request.effective_sequence));
        let mut receipts = Vec::with_capacity(ordered.len());
        for request in ordered {
            receipts.push(self.transition_with_head(registry, request, state_authority, false)?);
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
    if request.wind_down.exit_program != request.program.bytes() {
        return Err(DeprecationRefusal::InvalidExitProgram);
    }
    if request.effective_sequence >= request.wind_down.deadline {
        return Err(DeprecationRefusal::DeadlineElapsed);
    }
    Ok(())
}

fn validate_legacy_transition(
    request: &LegacyDeprecationRequest,
) -> Result<(), DeprecationRefusal> {
    let valid_edge = matches!(
        (request.expected, request.target),
        (ProgramLifecycle::Active, ProgramLifecycle::Deprecated)
            | (ProgramLifecycle::Deprecated, ProgramLifecycle::Tombstoned)
    );
    if !valid_edge || request.wind_down.state_access != WindDownStateAccess::ReadOnly {
        return Err(DeprecationRefusal::InvalidTransition);
    }
    if request.wind_down.exit_program != request.program.bytes() {
        return Err(DeprecationRefusal::InvalidExitProgram);
    }
    if request.effective_sequence >= request.wind_down.deadline {
        return Err(DeprecationRefusal::DeadlineElapsed);
    }
    Ok(())
}

fn validate_exits(
    request: &DeprecationRequest,
    bindings: &[crate::ProgramValueAccountBinding],
    accounts: &[ValueAccount],
) -> Result<Vec<ExitRoute>, DeprecationRefusal> {
    if request.exits.len() != bindings.len() || accounts.len() != bindings.len() {
        let missing = bindings
            .iter()
            .find(|binding| {
                !request
                    .exits
                    .iter()
                    .any(|route| route.account_id == binding.account_id)
            })
            .map_or([0; 32], |binding| binding.account_id);
        return Err(DeprecationRefusal::MissingExit {
            account_id: missing,
        });
    }
    let mut exits = request.exits.clone();
    exits.sort_by_key(|route| route.account_id);
    for pair in exits.windows(2) {
        if pair[0].account_id == pair[1].account_id {
            return Err(DeprecationRefusal::DuplicateExit);
        }
    }
    for binding in bindings {
        let route = exits
            .iter()
            .find(|route| route.account_id == binding.account_id)
            .ok_or(DeprecationRefusal::MissingExit {
                account_id: binding.account_id,
            })?;
        let account = accounts
            .iter()
            .find(|account| account.account_id == binding.account_id)
            .ok_or(DeprecationRefusal::MissingExit {
                account_id: binding.account_id,
            })?;
        if route.seed != binding.seed
            || route.asset_id != binding.asset_id
            || route.destination == [0; 32]
        {
            return Err(DeprecationRefusal::ExitMismatch {
                account_id: binding.account_id,
            });
        }
        if account.frozen && account.balance != 0 {
            return Err(DeprecationRefusal::ValueWouldBeStranded {
                account_id: binding.account_id,
            });
        }
        let probe_amount = account.balance.max(1);
        ProgramAuthority::for_owner_frame(
            request.program,
            &route.seed,
            route.account_id,
            route.asset_id,
            route.destination,
            probe_amount,
        )
        .map_err(|_| DeprecationRefusal::ValueWouldBeStranded {
            account_id: binding.account_id,
        })?;
    }
    Ok(exits)
}
