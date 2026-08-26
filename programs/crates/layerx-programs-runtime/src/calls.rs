//! Program-to-program composition. A call graph is the deterministic record of
//! one activity's nested program invocations, bounded by declared depth,
//! fan-out, edge and visit rules, closed against reentrancy, and committed or
//! discarded as a single unit together with every storage write and every
//! 402LXP transfer request the graph produced.

use core::fmt::{self, Display};
use std::collections::BTreeMap;
use std::rc::Rc;

use crate::abi::response::{CallResponse, ResponseRefusal};
use crate::abi::{
    Abi, AbiCommit, AbiError, AuthorizationContext, CallFrameId, Capability, MAX_CALL_INPUT_BYTES,
};
use crate::entrypoint::{self, EntrypointRefusal};
use crate::execute::{ExecutionFault, ABI_VERSION};
use crate::fault::{ProgramFailure, RefusalClass, RefusalReason, CANDIDATE_REFUSAL_SENTINEL};
use crate::host::RuntimeState;
use crate::limits::{DeclaredLimit, LimitsRefusal};
use crate::meter::{MeterRefusal, ResourceKind};
use crate::storage::{PrincipalId, ProgramId};
use crate::validate::{AbiRevision, ValidatedModule};

/// The declared upper bound on program-to-program nesting below the activity's
/// entry program.
pub const DEFAULT_MAX_COMPOSITION_DEPTH: u32 = 8;

/// The declared upper bound on the total number of edges in one call graph.
pub const DEFAULT_MAX_CALL_GRAPH_EDGES: u32 = 64;

/// The declared upper bound on outgoing calls made from a single frame.
pub const DEFAULT_MAX_CALL_FANOUT: u32 = 16;

/// The declared upper bound on how often one program may be entered inside a
/// single call graph.
pub const DEFAULT_MAX_PROGRAM_VISITS: u32 = 8;

/// The export a composable program provides as its call entry point. It takes
/// the input pointer and length and returns a non-negative result code.
pub const CALL_ENTRY_EXPORT: &str = "layerx_call";

/// The export a composable program provides to reserve a bounded input region
/// in its own linear memory. It takes a length and returns a pointer.
pub const CALL_RESERVE_EXPORT: &str = "layerx_reserve";

/// Fuel charged to the calling frame for admitting one program-to-program call.
pub const CALL_ADMISSION_FUEL: u64 = 1_024;

/// Fuel charged to the calling frame for each byte of call input copied into
/// the callee's linear memory.
pub const CALL_INPUT_FUEL_PER_BYTE: u64 = 1;

const GRAPH_DOMAIN: &[u8] = b"LayerX/programs/call-graph/v1\0";

/// The declared composition rules enforced on every call graph.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompositionRules {
    depth: u32,
    edges: u32,
    fanout: u32,
    visits: u32,
}

impl CompositionRules {
    /// Constructs composition rules, refusing any zero bound.
    ///
    /// # Errors
    ///
    /// Returns [`LimitsRefusal::ZeroLimit`] naming the offending rule when any
    /// bound is zero, because a zero bound would refuse every composition.
    pub const fn new(
        max_depth: u32,
        max_edges: u32,
        max_fanout: u32,
        max_program_visits: u32,
    ) -> Result<Self, LimitsRefusal> {
        if max_depth == 0 {
            return Err(LimitsRefusal::ZeroLimit {
                limit: DeclaredLimit::CompositionDepth,
            });
        }
        if max_edges == 0 {
            return Err(LimitsRefusal::ZeroLimit {
                limit: DeclaredLimit::CallGraphEdges,
            });
        }
        if max_fanout == 0 {
            return Err(LimitsRefusal::ZeroLimit {
                limit: DeclaredLimit::CallFanout,
            });
        }
        if max_program_visits == 0 {
            return Err(LimitsRefusal::ZeroLimit {
                limit: DeclaredLimit::ProgramVisits,
            });
        }
        Ok(Self {
            depth: max_depth,
            edges: max_edges,
            fanout: max_fanout,
            visits: max_program_visits,
        })
    }

    /// Returns the declared production composition rules.
    #[must_use]
    pub const fn declared() -> Self {
        Self {
            depth: DEFAULT_MAX_COMPOSITION_DEPTH,
            edges: DEFAULT_MAX_CALL_GRAPH_EDGES,
            fanout: DEFAULT_MAX_CALL_FANOUT,
            visits: DEFAULT_MAX_PROGRAM_VISITS,
        }
    }

    /// Returns the declared maximum nesting depth below the entry program.
    #[must_use]
    pub const fn max_depth(&self) -> u32 {
        self.depth
    }

    /// Returns the declared maximum number of edges in one call graph.
    #[must_use]
    pub const fn max_edges(&self) -> u32 {
        self.edges
    }

    /// Returns the declared maximum number of calls made from one frame.
    #[must_use]
    pub const fn max_fanout(&self) -> u32 {
        self.fanout
    }

    /// Returns the declared maximum number of entries into one program.
    #[must_use]
    pub const fn max_program_visits(&self) -> u32 {
        self.visits
    }
}

impl Default for CompositionRules {
    fn default() -> Self {
        Self::declared()
    }
}

/// One program active on the call stack of an activity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallFrame {
    id: CallFrameId,
    program: ProgramId,
    principal: PrincipalId,
    depth: u32,
    calls: u32,
}

impl CallFrame {
    /// Returns the host-fixed identity of this call frame.
    #[must_use]
    pub const fn id(&self) -> CallFrameId {
        self.id
    }
    /// Returns the program executing in this frame.
    #[must_use]
    pub const fn program(&self) -> ProgramId {
        self.program
    }

    /// Returns the invoking principal, identical for every frame of one graph.
    #[must_use]
    pub const fn principal(&self) -> PrincipalId {
        self.principal
    }

    /// Returns the nesting depth of this frame, zero for the entry program.
    #[must_use]
    pub const fn depth(&self) -> u32 {
        self.depth
    }

    /// Returns the number of outgoing calls this frame has already made.
    #[must_use]
    pub const fn calls(&self) -> u32 {
        self.calls
    }
}

/// One recorded program-to-program edge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallEdge {
    caller_frame: CallFrameId,
    callee_frame: CallFrameId,
    caller: ProgramId,
    callee: ProgramId,
    principal: PrincipalId,
    depth: u32,
}

impl CallEdge {
    /// Returns the frame that issued this call.
    #[must_use]
    pub const fn caller_frame(&self) -> CallFrameId {
        self.caller_frame
    }

    /// Returns the frame entered by this call.
    #[must_use]
    pub const fn callee_frame(&self) -> CallFrameId {
        self.callee_frame
    }
    /// Returns the program that made the call.
    #[must_use]
    pub const fn caller(&self) -> ProgramId {
        self.caller
    }

    /// Returns the program that was entered.
    #[must_use]
    pub const fn callee(&self) -> ProgramId {
        self.callee
    }

    /// Returns the invoking principal carried unchanged across the edge.
    #[must_use]
    pub const fn principal(&self) -> PrincipalId {
        self.principal
    }

    /// Returns the depth of the entered frame.
    #[must_use]
    pub const fn depth(&self) -> u32 {
        self.depth
    }
}

/// The deterministic composition record of one activity: the frames currently
/// active, every edge taken, and how often each program was entered.
///
/// Two reentrancy rules hold by construction. A program already active on the
/// stack can never be entered again, so no callee can observe a caller's
/// half-updated state, and no program can be re-entered to spend twice against
/// state it has not yet committed. A program may be entered at most
/// [`CompositionRules::max_program_visits`] times in total, so sequential
/// re-entry cannot be used to grind unbounded work out of one activity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallGraph {
    rules: CompositionRules,
    principal: PrincipalId,
    frames: Vec<CallFrame>,
    edges: Vec<CallEdge>,
    entered: BTreeMap<ProgramId, u32>,
}

impl CallGraph {
    /// Opens a call graph rooted at the activity's entry program.
    #[must_use]
    pub fn root(rules: CompositionRules, program: ProgramId, principal: PrincipalId) -> Self {
        let mut entered = BTreeMap::new();
        entered.insert(program, 1);
        Self {
            rules,
            principal,
            frames: vec![CallFrame {
                id: CallFrameId::root(),
                program,
                principal,
                depth: 0,
                calls: 0,
            }],
            edges: Vec::new(),
            entered,
        }
    }

    /// Returns the declared rules this graph is enforced against.
    #[must_use]
    pub const fn rules(&self) -> CompositionRules {
        self.rules
    }

    /// Returns the invoking principal of the activity.
    #[must_use]
    pub const fn principal(&self) -> PrincipalId {
        self.principal
    }

    /// Returns the frames currently active, entry program first.
    #[must_use]
    pub fn frames(&self) -> &[CallFrame] {
        &self.frames
    }

    /// Returns every edge taken by the activity in execution order.
    #[must_use]
    pub fn edges(&self) -> &[CallEdge] {
        &self.edges
    }

    /// Returns the frame currently executing.
    #[must_use]
    pub fn current(&self) -> Option<CallFrame> {
        self.frames.last().copied()
    }

    /// Returns the immediate caller of the active frame. The root frame has no
    /// caller; absence is not represented by a program identifier.
    #[must_use]
    pub fn immediate_caller(&self) -> Option<ProgramId> {
        self.frames.iter().rev().nth(1).map(|frame| frame.program)
    }

    /// Returns the nesting depth of the frame currently executing.
    #[must_use]
    pub fn depth(&self) -> u32 {
        self.frames.last().map_or(0, |frame| frame.depth)
    }

    /// Returns whether a program is already active on the call stack.
    #[must_use]
    pub fn active(&self, program: ProgramId) -> bool {
        self.frames.iter().any(|frame| frame.program == program)
    }

    /// Returns how often a program has been entered by this activity.
    #[must_use]
    pub fn visits(&self, program: ProgramId) -> u32 {
        self.entered.get(&program).copied().unwrap_or(0)
    }

    /// Encodes the graph into architecture-independent evidence bytes so the
    /// same activity yields the same composition record everywhere.
    #[must_use]
    pub fn canonical_evidence(&self) -> Vec<u8> {
        let mut evidence = Vec::with_capacity(
            GRAPH_DOMAIN
                .len()
                .saturating_add(56)
                .saturating_add(self.edges.len().saturating_mul(68)),
        );
        evidence.extend_from_slice(GRAPH_DOMAIN);
        evidence.extend_from_slice(&self.principal.bytes());
        evidence.extend_from_slice(&self.rules.depth.to_be_bytes());
        evidence.extend_from_slice(&self.rules.edges.to_be_bytes());
        evidence.extend_from_slice(&self.rules.fanout.to_be_bytes());
        evidence.extend_from_slice(&self.rules.visits.to_be_bytes());
        let count = u64::try_from(self.edges.len()).unwrap_or(u64::MAX);
        evidence.extend_from_slice(&count.to_be_bytes());
        for edge in &self.edges {
            evidence.extend_from_slice(&edge.caller.bytes());
            evidence.extend_from_slice(&edge.callee.bytes());
            evidence.extend_from_slice(&edge.depth.to_be_bytes());
        }
        evidence
    }

    pub(crate) fn enter(&mut self, callee: ProgramId) -> Result<(), CompositionRefusal> {
        let origin = self
            .frames
            .last()
            .copied()
            .ok_or(CompositionRefusal::NotComposable)?;
        if self.active(callee) {
            return Err(CompositionRefusal::Reentrancy { program: callee });
        }
        let depth = origin.depth.saturating_add(1);
        if depth > self.rules.depth {
            return Err(CompositionRefusal::DepthExceeded {
                limit: self.rules.depth,
                attempted: depth,
            });
        }
        let calls = origin.calls.saturating_add(1);
        if calls > self.rules.fanout {
            return Err(CompositionRefusal::FanoutExceeded {
                limit: self.rules.fanout,
                attempted: calls,
            });
        }
        let edges = u32::try_from(self.edges.len())
            .unwrap_or(u32::MAX)
            .saturating_add(1);
        if edges > self.rules.edges {
            return Err(CompositionRefusal::EdgesExceeded {
                limit: self.rules.edges,
                attempted: edges,
            });
        }
        let visits = self.visits(callee).saturating_add(1);
        if visits > self.rules.visits {
            return Err(CompositionRefusal::VisitsExceeded {
                program: callee,
                limit: self.rules.visits,
                attempted: visits,
            });
        }
        if let Some(frame) = self.frames.last_mut() {
            frame.calls = calls;
        }
        let frame_id = origin
            .id
            .child(calls)
            .map_err(|_| CompositionRefusal::FanoutExceeded {
                limit: self.rules.fanout,
                attempted: calls,
            })?;
        self.entered.insert(callee, visits);
        self.edges.push(CallEdge {
            caller_frame: origin.id,
            callee_frame: frame_id,
            caller: origin.program,
            callee,
            principal: self.principal,
            depth,
        });
        self.frames.push(CallFrame {
            id: frame_id,
            program: callee,
            principal: self.principal,
            depth,
            calls: 0,
        });
        Ok(())
    }

    pub(crate) fn leave(&mut self) {
        if self.frames.len() > 1 {
            self.frames.pop();
        }
    }
}

/// Deployed-code boundary consulted to enter a callee. It hands out validated
/// modules only; it carries no authority of its own.
pub trait ProgramResolver: fmt::Debug {
    /// Claims this resolver for one authenticated activity. Raw qualification
    /// catalogs accept any request; evidence-backed resolvers override this to
    /// enforce their exact activity binding and affine use.
    fn authorize_activity(
        &self,
        _binding: Option<crate::ActivityBudgetBinding>,
    ) -> Result<(), CompositionRefusal> {
        Ok(())
    }

    /// Returns the validated module deployed under a program identifier.
    fn program_module(&self, program: ProgramId) -> Option<&ValidatedModule>;
}

/// Caller-assembled validated modules for qualification and in-memory
/// execution. This catalog carries no deployment receipt or journal proof and
/// must not be treated as production deployment authority.
#[derive(Debug, Default)]
pub struct ProgramCatalog {
    modules: BTreeMap<ProgramId, ValidatedModule>,
}

impl ProgramCatalog {
    /// Creates an empty catalog. An empty catalog resolves nothing, so every
    /// attempted call fails typed.
    #[must_use]
    pub fn new() -> Self {
        Self {
            modules: BTreeMap::new(),
        }
    }

    /// Publishes one caller-supplied validated module under its program
    /// identifier, returning the module it replaced. This raw insertion does
    /// not verify deployment state.
    pub fn insert(
        &mut self,
        program: ProgramId,
        module: ValidatedModule,
    ) -> Option<ValidatedModule> {
        self.modules.insert(program, module)
    }

    /// Returns the number of callable programs.
    #[must_use]
    pub fn len(&self) -> usize {
        self.modules.len()
    }

    /// Returns whether no program is callable.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }

    /// Returns whether a program identifier resolves.
    #[must_use]
    pub fn contains(&self, program: ProgramId) -> bool {
        self.modules.contains_key(&program)
    }
}

impl ProgramResolver for ProgramCatalog {
    fn program_module(&self, program: ProgramId) -> Option<&ValidatedModule> {
        self.modules.get(&program)
    }
}

/// The composition surface handed to one authorized execution: which programs
/// are reachable and under which declared rules.
#[derive(Clone, Debug)]
pub struct CompositionContext {
    resolver: Rc<dyn ProgramResolver>,
    rules: CompositionRules,
}

impl CompositionContext {
    /// Builds a context over an explicit resolver.
    #[must_use]
    pub fn new(resolver: Rc<dyn ProgramResolver>, rules: CompositionRules) -> Self {
        Self { resolver, rules }
    }

    /// Builds a context over an owned catalog of validated modules.
    #[must_use]
    pub fn catalog(catalog: ProgramCatalog, rules: CompositionRules) -> Self {
        Self {
            resolver: Rc::new(catalog),
            rules,
        }
    }

    /// Builds a context that resolves no callee, refusing every attempted
    /// program-to-program call with a typed result.
    #[must_use]
    pub fn isolated() -> Self {
        Self {
            resolver: Rc::new(ProgramCatalog::new()),
            rules: CompositionRules::declared(),
        }
    }

    /// Returns the declared rules enforced on graphs built from this context.
    #[must_use]
    pub const fn rules(&self) -> CompositionRules {
        self.rules
    }

    pub(crate) fn claim_resolver(
        &self,
        binding: Option<crate::ActivityBudgetBinding>,
    ) -> Result<Rc<dyn ProgramResolver>, CompositionRefusal> {
        self.resolver.authorize_activity(binding)?;
        Ok(Rc::clone(&self.resolver))
    }
}

impl Default for CompositionContext {
    fn default() -> Self {
        Self::isolated()
    }
}

/// Closed composition refusal taxonomy. Every variant aborts the whole
/// activity, so no partial call graph, storage write or transfer request can
/// survive a refused leg.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompositionRefusal {
    /// The execution carries no authorization context or no composition state.
    NotComposable,
    /// A production deployment snapshot was used outside its authenticated
    /// activity or more than once.
    ActivityEvidenceRequired,
    /// The admitted activity differs from the deployment snapshot binding.
    ActivityEvidenceMismatch,
    /// The affine deployment snapshot was already consumed by an activity.
    ActivityEvidenceReused,
    /// A call graph attempted to cross ABI revisions.
    WrongVersion {
        expected: AbiRevision,
        actual: AbiRevision,
    },
    /// The callee identifier resolves to no deployed module.
    UnknownProgram {
        /// The unresolved callee.
        program: ProgramId,
    },
    /// The callee is already active on the call stack.
    Reentrancy {
        /// The program that would have been re-entered.
        program: ProgramId,
    },
    /// The graph would nest deeper than the declared rule allows.
    DepthExceeded {
        /// The declared depth rule.
        limit: u32,
        /// The depth the call would have reached.
        attempted: u32,
    },
    /// The graph would carry more edges than the declared rule allows.
    EdgesExceeded {
        /// The declared edge rule.
        limit: u32,
        /// The edge index the call would have taken.
        attempted: u32,
    },
    /// One frame would make more calls than the declared rule allows.
    FanoutExceeded {
        /// The declared fan-out rule.
        limit: u32,
        /// The outgoing call index the frame would have reached.
        attempted: u32,
    },
    /// One program would be entered more often than the declared rule allows.
    VisitsExceeded {
        /// The program that would have been entered again.
        program: ProgramId,
        /// The declared visit rule.
        limit: u32,
        /// The visit index the call would have reached.
        attempted: u32,
    },
    /// The callee exports no composition entry point.
    MissingEntry,
    /// The callee exports no input reservation function.
    MissingAllocator,
    /// The callee exports no linear memory to receive the call input.
    MissingMemory,
    /// The callee refused to reserve a region for the call input.
    AllocationRefused {
        /// The value the callee returned instead of a pointer.
        code: i32,
    },
    /// The call input exceeds the version-one ABI bound.
    InputTooLarge {
        /// The refused input length.
        bytes: usize,
        /// The declared input bound.
        limit: usize,
    },
    /// The callee returned a negative result code.
    GuestRefused {
        /// The program that refused.
        program: ProgramId,
        /// The negative result code the callee returned.
        code: i32,
    },
    /// Candidate program refusal with host-authenticated leaf identity.
    Program(ProgramFailure),
    /// The call was refused by the capability ABI, including every attempt to
    /// widen authority across an edge.
    Authority(AbiError),
    /// The callee faulted.
    Fault(ExecutionFault),
    /// The call graph exhausted a metered resource.
    Resource(MeterRefusal),
    /// Successful-response transport was refused.
    Response(ResponseRefusal),
}

impl Display for CompositionRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotComposable => {
                formatter.write_str("execution carries no composition authority")
            }
            Self::ActivityEvidenceRequired => {
                formatter.write_str("execution lacks current deployment evidence")
            }
            Self::ActivityEvidenceMismatch => {
                formatter.write_str("deployment evidence belongs to a different activity")
            }
            Self::ActivityEvidenceReused => {
                formatter.write_str("deployment evidence was already consumed")
            }
            Self::WrongVersion { expected, actual } => write!(
                formatter,
                "composition ABI revision {actual:?} differs from root {expected:?}"
            ),
            Self::UnknownProgram { .. } => formatter.write_str("callee program is not deployed"),
            Self::Reentrancy { .. } => {
                formatter.write_str("callee is already active on the call stack")
            }
            Self::DepthExceeded { limit, attempted } => write!(
                formatter,
                "composition depth rule {limit} exceeded by attempted depth {attempted}"
            ),
            Self::EdgesExceeded { limit, attempted } => write!(
                formatter,
                "call graph edge rule {limit} exceeded by attempted edge {attempted}"
            ),
            Self::FanoutExceeded { limit, attempted } => write!(
                formatter,
                "call fan-out rule {limit} exceeded by attempted call {attempted}"
            ),
            Self::VisitsExceeded {
                limit, attempted, ..
            } => write!(
                formatter,
                "program visit rule {limit} exceeded by attempted visit {attempted}"
            ),
            Self::MissingEntry => formatter.write_str("callee exports no composition entry point"),
            Self::MissingAllocator => {
                formatter.write_str("callee exports no composition input reservation")
            }
            Self::MissingMemory => formatter.write_str("callee exports no linear memory"),
            Self::AllocationRefused { code } => {
                write!(
                    formatter,
                    "callee refused the input reservation with {code}"
                )
            }
            Self::InputTooLarge { bytes, limit } => write!(
                formatter,
                "call input of {bytes} bytes exceeds the ABI bound {limit}"
            ),
            Self::GuestRefused { code, .. } => {
                write!(formatter, "callee refused the call with {code}")
            }
            Self::Program(failure) => write!(
                formatter,
                "program {:?} refused with class {:?}",
                failure.program(),
                failure.class()
            ),
            Self::Authority(error) => write!(formatter, "composition authority refusal: {error}"),
            Self::Fault(fault) => write!(formatter, "callee fault: {fault}"),
            Self::Resource(refusal) => write!(formatter, "composition resource refusal: {refusal}"),
            Self::Response(refusal) => write!(formatter, "composition response refusal: {refusal}"),
        }
    }
}

impl std::error::Error for CompositionRefusal {}

impl From<AbiError> for CompositionRefusal {
    fn from(value: AbiError) -> Self {
        Self::Authority(value)
    }
}

impl From<MeterRefusal> for CompositionRefusal {
    fn from(value: MeterRefusal) -> Self {
        Self::Resource(value)
    }
}

#[derive(Debug)]
pub(crate) struct Composition {
    resolver: Rc<dyn ProgramResolver>,
    graph: CallGraph,
    revision: AbiRevision,
}

impl Composition {
    pub(crate) fn new(
        resolver: Rc<dyn ProgramResolver>,
        graph: CallGraph,
        revision: AbiRevision,
    ) -> Self {
        Self {
            resolver,
            graph,
            revision,
        }
    }

    pub(crate) fn resolver(&self) -> Rc<dyn ProgramResolver> {
        Rc::clone(&self.resolver)
    }

    pub(crate) const fn graph(&self) -> &CallGraph {
        &self.graph
    }

    pub(crate) fn graph_mut(&mut self) -> &mut CallGraph {
        &mut self.graph
    }

    pub(crate) const fn revision(&self) -> AbiRevision {
        self.revision
    }

    pub(crate) fn set_graph(&mut self, graph: CallGraph) {
        self.graph = graph;
    }

    pub(crate) fn into_graph(self) -> CallGraph {
        self.graph
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NestedOutcome {
    pub(crate) code: i32,
    pub(crate) response: CallResponse,
    pub(crate) subtree_fuel: u64,
}

pub(crate) struct PendingNestedOutcome {
    code: i32,
    pub(crate) response: CallResponse,
    subtree_fuel: u64,
    meter: crate::meter::Meter,
    committed: AbiCommit,
    graph: CallGraph,
}

/// Charges the calling frame for admitting one call of the given input size.
#[must_use]
pub fn call_admission_fuel(input_bytes: usize) -> u64 {
    let bytes = u64::try_from(input_bytes).unwrap_or(u64::MAX);
    CALL_ADMISSION_FUEL.saturating_add(bytes.saturating_mul(CALL_INPUT_FUEL_PER_BYTE))
}

#[allow(clippy::too_many_lines)]
pub(crate) fn execute_nested_call(
    state: &mut RuntimeState,
    consumed: u64,
    callee: ProgramId,
    input: &[u8],
    requested: Vec<Capability>,
) -> Result<NestedOutcome, CompositionRefusal> {
    let pending = execute_nested(state, consumed, callee, input, requested, None)?;
    adopt_nested_call(state, pending)
}

pub(crate) fn execute_nested_call_response(
    state: &mut RuntimeState,
    consumed: u64,
    callee: ProgramId,
    input: &[u8],
    requested: Vec<Capability>,
    response_capacity: usize,
) -> Result<PendingNestedOutcome, CompositionRefusal> {
    execute_nested(
        state,
        consumed,
        callee,
        input,
        requested,
        Some(response_capacity),
    )
}

#[allow(clippy::too_many_lines)]
fn execute_nested(
    state: &mut RuntimeState,
    consumed: u64,
    callee: ProgramId,
    input: &[u8],
    requested: Vec<Capability>,
    response_capacity: Option<usize>,
) -> Result<PendingNestedOutcome, CompositionRefusal> {
    if input.len() > MAX_CALL_INPUT_BYTES {
        return Err(CompositionRefusal::InputTooLarge {
            bytes: input.len(),
            limit: MAX_CALL_INPUT_BYTES,
        });
    }
    let resolver = state
        .composition()
        .ok_or(CompositionRefusal::NotComposable)?
        .resolver();
    let module = resolver
        .program_module(callee)
        .ok_or(CompositionRefusal::UnknownProgram { program: callee })?;
    let expected = state
        .composition()
        .ok_or(CompositionRefusal::NotComposable)?
        .revision();
    let actual = module.abi_revision();
    if actual != expected {
        return Err(CompositionRefusal::WrongVersion { expected, actual });
    }
    if state.meter().is_activity() {
        module
            .preflight_entrypoint(CALL_ENTRY_EXPORT, input.is_empty())
            .map_err(|refusal| preflight_entry_refusal(callee, refusal))?;
    }
    let carried = state.meter().cpu_carried();
    let protocol_context = state.protocol_context();
    let mut admitted_graph = state
        .composition()
        .ok_or(CompositionRefusal::NotComposable)?
        .graph()
        .clone();
    admitted_graph.enter(callee)?;
    let (principal, capabilities, storage, receipts, callee_frame) = {
        let abi = state.abi_mut().ok_or(CompositionRefusal::NotComposable)?;
        let callee_frame = admitted_graph
            .current()
            .ok_or(CompositionRefusal::NotComposable)?
            .id();
        let capabilities = abi.stage_call(callee, input, requested, callee_frame)?;
        (
            abi.principal(),
            capabilities,
            abi.storage_snapshot(),
            abi.verified_receipts(),
            callee_frame,
        )
    };
    state
        .composition_mut()
        .ok_or(CompositionRefusal::NotComposable)?
        .set_graph(admitted_graph);
    let mut child_meter = state.meter().clone();
    child_meter.carry_cpu(consumed)?;
    child_meter.record_cpu(0);
    let activity_meter = child_meter.is_activity();
    if child_meter.cpu_remaining() == 0 {
        return Err(CompositionRefusal::Resource(MeterRefusal::BudgetExceeded {
            resource: ResourceKind::Cpu,
            limit: child_meter.cpu_budget(),
            attempted: child_meter.cpu_budget().saturating_add(1),
        }));
    }
    let authorization = AuthorizationContext::nested(principal, capabilities, callee_frame);
    let child_abi = Abi::nested(ABI_VERSION, callee, authorization, storage, receipts)?;
    let child_graph = state
        .composition()
        .ok_or(CompositionRefusal::NotComposable)?
        .graph()
        .clone();
    let child_composition = Composition::new(Rc::clone(&resolver), child_graph, expected);
    let mut instance = if expected == AbiRevision::CandidateV2 {
        let retained = module
            .instantiate_composed_response_context_retained(
                child_meter,
                child_abi,
                child_composition,
                response_capacity.unwrap_or(0),
                protocol_context,
            )
            .map_err(response_refusal)?;
        match retained {
            Ok(instance) => instance,
            Err(error) => {
                let (fault, returned) = *error;
                let refusal = if activity_meter {
                    returned
                        .meter()
                        .exhaustion()
                        .map(CompositionRefusal::Resource)
                } else {
                    None
                }
                .or_else(|| returned.refusal().cloned())
                .or_else(|| returned.failure().cloned().map(CompositionRefusal::Program))
                .unwrap_or_else(|| {
                    if candidate_runtime_fault(&fault) {
                        CompositionRefusal::Program(runtime_failure(callee))
                    } else {
                        instantiation_refusal(fault, returned.meter().exhaustion())
                    }
                });
                retain_failed_nested(state, returned, carried, consumed)?;
                return Err(refusal);
            }
        }
    } else if activity_meter {
        match module.instantiate_composed_retained(child_meter, child_abi, child_composition) {
            Ok(instance) => instance,
            Err(error) => {
                let (fault, returned) = *error;
                let refusal = if let Some(exhausted) = returned.meter().exhaustion() {
                    CompositionRefusal::Resource(exhausted)
                } else if let Some(refusal) = returned.refusal() {
                    refusal.clone()
                } else if let Some(failure) = returned.failure() {
                    CompositionRefusal::Program(failure.clone())
                } else {
                    instantiation_refusal(fault, returned.meter().exhaustion())
                };
                retain_failed_nested(state, returned, carried, consumed)?;
                return Err(refusal);
            }
        }
    } else {
        module
            .instantiate_composed(child_meter, child_abi, child_composition)
            .map_err(|(fault, exhausted)| instantiation_refusal(fault, exhausted))?
    };
    let code = match entrypoint::invoke(&mut instance, CALL_ENTRY_EXPORT, input) {
        Ok(code) => code,
        Err(refusal) => {
            let refusal = entry_refusal(&instance, callee, refusal);
            retain_failed_nested(state, instance.into_state(), carried, consumed)?;
            return Err(refusal);
        }
    };
    let published_refusal = instance.state().refusal().cloned().or_else(|| {
        instance.state().failure().map(|_| {
            CompositionRefusal::Response(ResponseRefusal::CodeMismatch {
                published: CANDIDATE_REFUSAL_SENTINEL,
                returned: code,
            })
        })
    });
    if let Some(refusal) = published_refusal {
        retain_failed_nested(state, instance.into_state(), carried, consumed)?;
        return Err(refusal);
    }
    let response = instance
        .state()
        .finalize_response(code)
        .map_err(response_refusal)?;
    let (returned_meter, returned_abi, returned_composition) = instance.into_state().into_parts();
    let committed = returned_abi
        .ok_or(CompositionRefusal::NotComposable)?
        .commit();
    let returned_graph = returned_composition
        .ok_or(CompositionRefusal::NotComposable)?
        .into_graph();
    let subtree_fuel = reconciled_subtree_fuel(returned_meter.cpu_total(), carried, consumed)?;
    Ok(PendingNestedOutcome {
        code,
        response,
        subtree_fuel,
        meter: returned_meter,
        committed,
        graph: returned_graph,
    })
}

fn retain_failed_nested(
    state: &mut RuntimeState,
    mut returned: RuntimeState,
    carried: u64,
    consumed: u64,
) -> Result<(), CompositionRefusal> {
    let (active_memory, active_tables) = state.meter().active_frame_resources();
    let propagated_graph = returned.take_failure_graph();
    let (mut returned_meter, _, returned_composition) = returned.into_parts();
    let failed_graph =
        propagated_graph.or_else(|| returned_composition.map(Composition::into_graph));
    let subtree_fuel = reconciled_subtree_fuel(returned_meter.cpu_total(), carried, consumed)?;
    returned_meter.restore_cpu_carry(carried);
    returned_meter.restore_active_frame_resources(active_memory, active_tables);
    state.set_meter(returned_meter);
    state.set_failure_subtree_fuel(subtree_fuel);
    if let Some(graph) = failed_graph {
        state.set_failure_graph(graph);
    }
    Ok(())
}

fn reconciled_subtree_fuel(
    returned_total: u64,
    carried: u64,
    consumed: u64,
) -> Result<u64, CompositionRefusal> {
    let parent_total = carried.checked_add(consumed).ok_or({
        CompositionRefusal::Resource(MeterRefusal::CounterOverflow {
            resource: ResourceKind::Cpu,
        })
    })?;
    returned_total.checked_sub(parent_total).ok_or({
        CompositionRefusal::Resource(MeterRefusal::CounterOverflow {
            resource: ResourceKind::Cpu,
        })
    })
}

pub(crate) fn adopt_nested_call(
    state: &mut RuntimeState,
    pending: PendingNestedOutcome,
) -> Result<NestedOutcome, CompositionRefusal> {
    let carried = state.meter().cpu_carried();
    let (active_memory, active_tables) = state.meter().active_frame_resources();
    let mut absorbed = pending.meter;
    absorbed.restore_cpu_carry(carried);
    absorbed.restore_active_frame_resources(active_memory, active_tables);
    state.set_meter(absorbed);
    {
        let abi = state.abi_mut().ok_or(CompositionRefusal::NotComposable)?;
        abi.adopt_storage(pending.committed.storage);
        abi.absorb(pending.committed.effects);
    }
    {
        let composition = state
            .composition_mut()
            .ok_or(CompositionRefusal::NotComposable)?;
        composition.set_graph(pending.graph);
        composition.graph_mut().leave();
    }
    Ok(NestedOutcome {
        code: pending.code,
        response: pending.response,
        subtree_fuel: pending.subtree_fuel,
    })
}

fn response_refusal(refusal: ResponseRefusal) -> CompositionRefusal {
    match refusal {
        ResponseRefusal::Meter(refusal) => CompositionRefusal::Resource(refusal),
        other => CompositionRefusal::Response(other),
    }
}

fn preflight_entry_refusal(program: ProgramId, refusal: EntrypointRefusal) -> CompositionRefusal {
    match refusal {
        EntrypointRefusal::InputTooLarge { bytes, limit } => {
            CompositionRefusal::InputTooLarge { bytes, limit }
        }
        EntrypointRefusal::MissingAllocator => CompositionRefusal::MissingAllocator,
        EntrypointRefusal::MissingMemory => CompositionRefusal::MissingMemory,
        EntrypointRefusal::MissingEntry => CompositionRefusal::MissingEntry,
        EntrypointRefusal::AllocationRefused { code } => {
            CompositionRefusal::AllocationRefused { code }
        }
        EntrypointRefusal::GuestRefused { code } => {
            CompositionRefusal::GuestRefused { program, code }
        }
        EntrypointRefusal::Fault(fault) => CompositionRefusal::Fault(fault),
        EntrypointRefusal::Resource(refusal) => CompositionRefusal::Resource(refusal),
    }
}

fn entry_refusal(
    instance: &crate::execute::ProgramInstance,
    program: ProgramId,
    refusal: EntrypointRefusal,
) -> CompositionRefusal {
    if let Some(refusal) = instance.state().refusal() {
        return refusal.clone();
    }
    match refusal {
        EntrypointRefusal::InputTooLarge { bytes, limit } => {
            CompositionRefusal::InputTooLarge { bytes, limit }
        }
        EntrypointRefusal::MissingAllocator => CompositionRefusal::MissingAllocator,
        EntrypointRefusal::MissingMemory => CompositionRefusal::MissingMemory,
        EntrypointRefusal::MissingEntry => CompositionRefusal::MissingEntry,
        EntrypointRefusal::AllocationRefused { .. }
            if instance.meter().is_activity()
                && instance.state().composition().is_some_and(|composition| {
                    composition.revision() == AbiRevision::CandidateV2
                }) =>
        {
            legacy_failure(program)
        }
        EntrypointRefusal::AllocationRefused { code } => {
            CompositionRefusal::AllocationRefused { code }
        }
        EntrypointRefusal::GuestRefused { code }
            if instance
                .state()
                .composition()
                .is_some_and(|composition| composition.revision() == AbiRevision::CandidateV2) =>
        {
            match instance.state().failure().cloned() {
                Some(failure) if code == CANDIDATE_REFUSAL_SENTINEL => {
                    CompositionRefusal::Program(failure)
                }
                Some(_) => CompositionRefusal::Response(ResponseRefusal::CodeMismatch {
                    published: CANDIDATE_REFUSAL_SENTINEL,
                    returned: code,
                }),
                None if code == CANDIDATE_REFUSAL_SENTINEL => {
                    CompositionRefusal::Response(ResponseRefusal::InvalidPublication)
                }
                None => legacy_failure(program),
            }
        }
        EntrypointRefusal::GuestRefused { code } => {
            CompositionRefusal::GuestRefused { program, code }
        }
        EntrypointRefusal::Fault(fault)
            if instance
                .state()
                .composition()
                .is_some_and(|composition| composition.revision() == AbiRevision::CandidateV2)
                && candidate_runtime_fault(&fault) =>
        {
            instance.state().failure().cloned().map_or_else(
                || CompositionRefusal::Program(runtime_failure(program)),
                CompositionRefusal::Program,
            )
        }
        EntrypointRefusal::Fault(fault) => CompositionRefusal::Fault(fault),
        EntrypointRefusal::Resource(_)
            if instance
                .state()
                .composition()
                .is_some_and(|composition| composition.revision() == AbiRevision::CandidateV2)
                && instance.state().failure().is_some() =>
        {
            CompositionRefusal::Program(
                instance
                    .state()
                    .failure()
                    .cloned()
                    .unwrap_or_else(|| unreachable!("guarded candidate failure")),
            )
        }
        EntrypointRefusal::Resource(refusal) => CompositionRefusal::Resource(refusal),
    }
}

fn legacy_failure(program: ProgramId) -> CompositionRefusal {
    CompositionRefusal::Program(ProgramFailure::authenticated(
        program,
        RefusalClass::Legacy,
        RefusalReason::empty(),
    ))
}

fn runtime_failure(program: ProgramId) -> ProgramFailure {
    ProgramFailure::authenticated(program, RefusalClass::RuntimeFault, RefusalReason::empty())
}

fn candidate_runtime_fault(fault: &ExecutionFault) -> bool {
    !matches!(
        fault,
        ExecutionFault::EngineFault { .. }
            | ExecutionFault::UnknownExport { .. }
            | ExecutionFault::NotAFunction { .. }
            | ExecutionFault::OutOfFuel
            | ExecutionFault::GrowthLimited
            | ExecutionFault::Resource { .. }
    )
}

fn instantiation_refusal(
    fault: ExecutionFault,
    exhausted: Option<MeterRefusal>,
) -> CompositionRefusal {
    if let Some(refusal) = exhausted {
        return CompositionRefusal::Resource(refusal);
    }
    match fault {
        ExecutionFault::Resource { refusal } => CompositionRefusal::Resource(refusal),
        other => CompositionRefusal::Fault(other),
    }
}

#[cfg(test)]
mod context_tests {
    use super::{CallGraph, CompositionRefusal, CompositionRules};
    use crate::{PrincipalId, ProgramId};

    #[test]
    fn immediate_caller_is_owned_by_each_active_edge() {
        let root = ProgramId::new([1; 32]).expect("root");
        let middle = ProgramId::new([2; 32]).expect("middle");
        let leaf = ProgramId::new([3; 32]).expect("leaf");
        let principal = PrincipalId::new([4; 32]).expect("principal");
        let mut graph = CallGraph::root(CompositionRules::declared(), root, principal);
        assert_eq!(graph.current().map(|frame| frame.program()), Some(root));
        assert_eq!(graph.immediate_caller(), None);

        graph.enter(middle).expect("middle edge");
        assert_eq!(graph.current().map(|frame| frame.program()), Some(middle));
        assert_eq!(graph.immediate_caller(), Some(root));

        graph.enter(leaf).expect("leaf edge");
        assert_eq!(graph.current().map(|frame| frame.program()), Some(leaf));
        assert_eq!(graph.immediate_caller(), Some(middle));
        assert!(matches!(
            graph.enter(root),
            Err(CompositionRefusal::Reentrancy { program }) if program == root
        ));
        assert_eq!(graph.current().map(|frame| frame.program()), Some(leaf));
        assert_eq!(graph.immediate_caller(), Some(middle));

        graph.leave();
        assert_eq!(graph.current().map(|frame| frame.program()), Some(middle));
        assert_eq!(graph.immediate_caller(), Some(root));
    }
}
