//! Deterministic scheduling for program activities with declared access sets.

use std::fmt;
use std::collections::BTreeSet;
use std::num::NonZeroUsize;
use std::thread;

use crate::{AccessDeclaration, AccessSet};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProtocolScheduleEffects {
    accounts: AccessSet,
    identities: BTreeSet<[u8; 32]>,
}

impl ProtocolScheduleEffects {
    pub(crate) fn new(
        accounts: AccessSet,
        identities: impl IntoIterator<Item = [u8; 32]>,
    ) -> Option<Self> {
        let identities: BTreeSet<_> = identities.into_iter().collect();
        if identities.iter().any(|identity| *identity == [0; 32]) {
            return None;
        }
        Some(Self { accounts, identities })
    }

    pub(crate) fn empty() -> Self {
        Self { accounts: AccessSet::empty(), identities: BTreeSet::new() }
    }

    fn conflicts_with(&self, other: &Self) -> bool {
        self.accounts.conflicts_with(&other.accounts)
            || self.identities.iter().any(|identity| other.identities.contains(identity))
    }
}

/// Owned result of authenticating a CALL scheduling projection. The exact
/// payload and admission binding are retained so a prepared worker input
/// cannot be detached from the activity whose capabilities were decoded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PreparedScheduleAccess {
    access: ScheduleAccess,
    canonical_payload: Vec<u8>,
    activity_binding: [u8; 32],
    payer: [u8; 32],
}

impl PreparedScheduleAccess {
    pub(crate) fn from_authenticated_call(
        canonical_payload: &[u8],
        activity_binding: [u8; 32],
        program: crate::ProgramId,
        principal: crate::PrincipalId,
        payer: [u8; 32],
        capabilities: &[u8],
        access_declaration: &[u8],
        protocol_effects: Option<ProtocolScheduleEffects>,
    ) -> Result<Self, crate::AbiError> {
        if canonical_payload.is_empty() || activity_binding == [0; 32] || payer == [0; 32] {
            return Err(crate::AbiError::InvalidEncoding);
        }
        let declaration = AccessDeclaration::canonical_decode(access_declaration)
            .map_err(|_| crate::AbiError::AccessDeclaration)?;
        let reachable = crate::CapabilitySet::admitted_schedule_accesses(
            capabilities,
            program,
            principal,
        )?;
        let Some(protocol_effects) = protocol_effects else {
            return Ok(Self {
                access: ScheduleAccess::conservative_absent(),
                canonical_payload: canonical_payload.to_vec(),
                activity_binding,
                payer,
            });
        };
        Ok(Self {
            access: ScheduleAccess::from_admitted(
                declaration,
                reachable,
                protocol_effects,
            ),
            canonical_payload: canonical_payload.to_vec(),
            activity_binding,
            payer,
        })
    }

    pub(crate) const fn access(&self) -> &ScheduleAccess { &self.access }
    pub(crate) fn canonical_payload(&self) -> &[u8] { &self.canonical_payload }
    pub(crate) const fn activity_binding(&self) -> &[u8; 32] { &self.activity_binding }
    pub(crate) const fn payer(&self) -> &[u8; 32] { &self.payer }
}

/// Conservative default that bounds node-local thread demand without entering
/// protocol semantics. Any positive configured bound produces the same plan.
pub const DEFAULT_MAXIMUM_SCHEDULER_WORKERS: usize = 8;

/// The access information used to place one activity in a dependency level.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScheduleAccess {
    declaration: AccessDeclaration,
    reachable: AccessSet,
    protocol_effects: Option<ProtocolScheduleEffects>,
    conservative: bool,
}

impl ScheduleAccess {
    /// Schedules a caller-committed explicit set. Reachability is irrelevant to
    /// conflicts here because an explicit over-declaration remains binding.
    #[must_use]
    pub fn explicit(accesses: AccessSet) -> Self {
        Self {
            declaration: AccessDeclaration::explicit(accesses.clone()),
            reachable: accesses,
            protocol_effects: Some(ProtocolScheduleEffects::empty()),
            conservative: false,
        }
    }

    /// Safe public representation of an absent declaration when verified
    /// reachability is unavailable: it conflicts with every other activity.
    #[must_use]
    pub const fn conservative_absent() -> Self {
        Self {
            declaration: AccessDeclaration::absent(),
            reachable: AccessSet::empty(),
            protocol_effects: None,
            conservative: true,
        }
    }

    /// Production construction path. The caller must derive `reachable` from
    /// the admitted request's verified capabilities, never activity metadata.
    #[must_use]
    pub(crate) const fn from_admitted(
        declaration: AccessDeclaration,
        reachable: AccessSet,
        protocol_effects: ProtocolScheduleEffects,
    ) -> Self {
        Self {
            declaration,
            reachable,
            protocol_effects: Some(protocol_effects),
            conservative: false,
        }
    }

    #[must_use]
    pub const fn declaration(&self) -> &AccessDeclaration { &self.declaration }

    #[must_use]
    pub const fn reachable(&self) -> &AccessSet { &self.reachable }

    #[must_use]
    pub(crate) fn protocol_effects(&self) -> Option<&ProtocolScheduleEffects> {
        self.protocol_effects.as_ref()
    }

    #[must_use]
    pub fn conflicts_with(&self, other: &Self) -> bool {
        if self.conservative || other.conservative {
            return true;
        }
        let guest_conflict = self.declaration.conflicts_with_resolved(
            &self.reachable,
            &other.declaration,
            &other.reachable,
        );
        let protocol_conflict = match (
            self.protocol_effects.as_ref(),
            other.protocol_effects.as_ref(),
        ) {
            (Some(left), Some(right)) => left.conflicts_with(right),
            _ => true,
        };
        guest_conflict || protocol_conflict
    }
}

/// Canonical predecessor-conflict graph for one already ordered batch.
///
/// Edges always point from a lower canonical activity index to a higher one.
/// This orientation and the monotonic level frontier prevent a later activity
/// from being applied before any earlier canonical activity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConflictGraph {
    predecessors: Vec<Vec<usize>>,
    dependency_levels: Vec<Vec<usize>>,
}

impl ConflictGraph {
    /// Constructs the graph and its unique monotonic canonical-level partition.
    #[must_use]
    pub fn from_accesses(accesses: &[ScheduleAccess]) -> Self {
        let mut predecessors = Vec::with_capacity(accesses.len());
        let mut activity_levels = Vec::with_capacity(accesses.len());
        let mut dependency_levels: Vec<Vec<usize>> = Vec::new();

        for (activity, access) in accesses.iter().enumerate() {
            let mut incoming = Vec::new();
            let mut level = 0usize;
            for predecessor in 0..activity {
                if access.conflicts_with(&accesses[predecessor]) {
                    incoming.push(predecessor);
                    level = level.max(activity_levels[predecessor] + 1);
                }
            }
            if let Some(previous_level) = activity_levels.last() {
                level = level.max(*previous_level);
            }
            if dependency_levels.len() == level {
                dependency_levels.push(Vec::new());
            }
            dependency_levels[level].push(activity);
            predecessors.push(incoming);
            activity_levels.push(level);
        }

        Self { predecessors, dependency_levels }
    }

    #[must_use]
    pub fn len(&self) -> usize { self.predecessors.len() }

    #[must_use]
    pub fn is_empty(&self) -> bool { self.predecessors.is_empty() }

    #[must_use]
    pub fn conflicts(&self, earlier: usize, later: usize) -> bool {
        if earlier >= later || later >= self.predecessors.len() {
            return false;
        }
        self.predecessors[later].binary_search(&earlier).is_ok()
    }

    #[must_use]
    pub fn predecessors(&self, activity: usize) -> Option<&[usize]> {
        self.predecessors.get(activity).map(Vec::as_slice)
    }

    #[must_use]
    pub fn dependency_levels(&self) -> &[Vec<usize>] { &self.dependency_levels }
}

/// Immutable schedule derived solely from canonical batch access information.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchedulePlan {
    graph: ConflictGraph,
}

impl SchedulePlan {
    #[must_use]
    pub const fn graph(&self) -> &ConflictGraph { &self.graph }

    #[must_use]
    pub fn dependency_levels(&self) -> &[Vec<usize>] { self.graph.dependency_levels() }
}

/// Whether execution may use worker threads or deliberately refuses parallelism.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SchedulingStrategy {
    Parallel,
    Serial,
}

/// A deterministic scheduler. Strategy changes execution mechanics, never ordering.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParallelScheduler {
    strategy: SchedulingStrategy,
    maximum_workers: NonZeroUsize,
}

impl ParallelScheduler {
    #[must_use]
    pub fn parallel() -> Self {
        Self {
            strategy: SchedulingStrategy::Parallel,
            maximum_workers: NonZeroUsize::new(DEFAULT_MAXIMUM_SCHEDULER_WORKERS)
                .expect("scheduler worker default is nonzero"),
        }
    }

    /// Selects a worker bound without changing the graph, snapshots, or commit order.
    #[must_use]
    pub const fn parallel_with_workers(maximum_workers: NonZeroUsize) -> Self {
        Self { strategy: SchedulingStrategy::Parallel, maximum_workers }
    }

    /// Safe refusal path for operators that cannot or choose not to parallelise.
    #[must_use]
    pub const fn serial() -> Self {
        Self {
            strategy: SchedulingStrategy::Serial,
            maximum_workers: NonZeroUsize::MIN,
        }
    }

    #[must_use]
    pub const fn strategy(self) -> SchedulingStrategy { self.strategy }

    #[must_use]
    pub const fn maximum_workers(self) -> NonZeroUsize { self.maximum_workers }

    #[must_use]
    pub fn plan(self, accesses: &[ScheduleAccess]) -> SchedulePlan {
        SchedulePlan { graph: ConflictGraph::from_accesses(accesses) }
    }

    /// Executes each dependency level against one immutable snapshot. Results are
    /// applied only to scheduler-owned speculative state between levels, retained,
    /// and committed externally only after every execution and apply succeeds.
    /// Therefore an error drops the speculative state without leaking a partial
    /// commit. The infallible final callback runs in global canonical order. Serial
    /// strategy uses the identical snapshot, apply, and commit protocol.
    pub fn execute_staged<T, S, R, E, Execute, Apply, Commit>(
        self,
        activities: &[T],
        accesses: &[ScheduleAccess],
        mut speculative_state: S,
        execute: Execute,
        mut apply: Apply,
        mut commit: Commit,
    ) -> Result<S, ScheduleError<E>>
    where
        T: Sync,
        S: Clone + Sync,
        R: Send,
        E: Send,
        Execute: Fn(&S, usize, &T) -> Result<R, E> + Sync,
        Apply: FnMut(&mut S, usize, &R) -> Result<(), E>,
        Commit: FnMut(usize, R),
    {
        if activities.len() != accesses.len() {
            return Err(ScheduleError::LengthMismatch {
                activities: activities.len(),
                accesses: accesses.len(),
            });
        }

        let plan = self.plan(accesses);
        let mut completed: Vec<Option<R>> = (0..activities.len()).map(|_| None).collect();
        for level in plan.dependency_levels() {
            let view = speculative_state.clone();
            let staged = match self.strategy {
                SchedulingStrategy::Serial => level
                    .iter()
                    .map(|&index| (index, execute(&view, index, &activities[index])))
                    .collect(),
                SchedulingStrategy::Parallel => execute_level(
                    level,
                    activities,
                    &view,
                    &execute,
                    self.maximum_workers,
                )?,
            };
            for (index, result) in staged {
                let output = result.map_err(|source| ScheduleError::Activity { index, source })?;
                apply(&mut speculative_state, index, &output)
                    .map_err(|source| ScheduleError::Activity { index, source })?;
                completed[index] = Some(output);
            }
        }
        for (index, output) in completed.into_iter().enumerate() {
            let output = output.ok_or(ScheduleError::MissingResult { index })?;
            commit(index, output);
        }
        Ok(speculative_state)
    }
}

impl Default for ParallelScheduler {
    fn default() -> Self { Self::parallel() }
}

fn execute_level<T, S, R, E, Execute>(
    level: &[usize],
    activities: &[T],
    view: &S,
    execute: &Execute,
    maximum_workers: NonZeroUsize,
) -> Result<Vec<(usize, Result<R, E>)>, ScheduleError<E>>
where
    T: Sync,
    S: Sync,
    R: Send,
    E: Send,
    Execute: Fn(&S, usize, &T) -> Result<R, E> + Sync,
{
    let mut staged = Vec::with_capacity(level.len());
    for group in level.chunks(maximum_workers.get()) {
        let mut group_results = thread::scope(|scope| {
            let mut workers = Vec::with_capacity(group.len());
            let mut serial_tail = Vec::new();
            let mut refuse_parallel = false;
            for &index in group {
                if refuse_parallel {
                    serial_tail.push((index, execute(view, index, &activities[index])));
                    continue;
                }
                match thread::Builder::new().spawn_scoped(scope, move || {
                    execute(view, index, &activities[index])
                }) {
                    Ok(worker) => workers.push((index, worker)),
                    Err(_) => {
                        // Host thread availability is not protocol state. Keep
                        // the same snapshot and stage the unspawned suffix here.
                        refuse_parallel = true;
                        serial_tail.push((index, execute(view, index, &activities[index])));
                    }
                }
            }
            let mut results = Vec::with_capacity(workers.len() + serial_tail.len());
            for (index, worker) in workers {
                let result = worker.join().map_err(|_| ScheduleError::WorkerPanicked { index })?;
                results.push((index, result));
            }
            results.extend(serial_tail);
            results.sort_by_key(|(index, _)| *index);
            Ok(results)
        })?;
        staged.append(&mut group_results);
    }
    Ok(staged)
}

/// Failure before or during staged execution.
#[derive(Debug, Eq, PartialEq)]
pub enum ScheduleError<E> {
    LengthMismatch { activities: usize, accesses: usize },
    WorkerPanicked { index: usize },
    MissingResult { index: usize },
    Activity { index: usize, source: E },
}

impl<E: fmt::Display> fmt::Display for ScheduleError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LengthMismatch { activities, accesses } => write!(
                formatter,
                "scheduler received {activities} activities but {accesses} access declarations",
            ),
            Self::WorkerPanicked { index } => write!(formatter, "scheduler worker {index} panicked"),
            Self::MissingResult { index } => write!(formatter, "scheduler produced no result for activity {index}"),
            Self::Activity { index, source } => write!(formatter, "activity {index} failed: {source}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for ScheduleError<E> {}
