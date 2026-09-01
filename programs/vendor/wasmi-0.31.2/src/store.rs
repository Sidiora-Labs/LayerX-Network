use crate::{
    engine::DedupFuncType,
    externref::{ExternObject, ExternObjectEntity, ExternObjectIdx},
    func::{Trampoline, TrampolineEntity, TrampolineIdx},
    memory::{DataSegment, MemoryError},
    module::InstantiationError,
    table::TableError,
    DataSegmentEntity,
    DataSegmentIdx,
    ElementSegment,
    ElementSegmentEntity,
    ElementSegmentIdx,
    Engine,
    Func,
    FuncEntity,
    FuncIdx,
    FuncType,
    Global,
    GlobalEntity,
    GlobalIdx,
    Instance,
    InstanceEntity,
    InstanceIdx,
    Memory,
    MemoryEntity,
    MemoryIdx,
    ResourceLimiter,
    Table,
    TableEntity,
    TableIdx,
};
use alloc::{boxed::Box, vec::Vec};
use core::{
    fmt::{self, Debug},
    sync::atomic::{AtomicU32, Ordering},
};
use wasmi_arena::{Arena, ArenaIndex, GuardedEntity};
use wasmi_core::TrapCode;
use crate::execution_trace::{ExecutionObserver, ExecutionObserverError, ExecutionSnapshot, ExecutionSupplement, ExecutionTransition, ObservationCharge};

/// A unique store index.
///
/// # Note
///
/// Used to protect against invalid entity indices.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct StoreIdx(u32);

impl ArenaIndex for StoreIdx {
    fn into_usize(self) -> usize {
        self.0 as usize
    }

    fn from_usize(value: usize) -> Self {
        let value = value.try_into().unwrap_or_else(|error| {
            panic!("index {value} is out of bounds as store index: {error}")
        });
        Self(value)
    }
}

impl StoreIdx {
    /// Returns a new unique [`StoreIdx`].
    fn new() -> Self {
        /// A static store index counter.
        static CURRENT_STORE_IDX: AtomicU32 = AtomicU32::new(0);
        let next_idx = CURRENT_STORE_IDX.fetch_add(1, Ordering::AcqRel);
        Self(next_idx)
    }
}

/// A stored entity.
pub type Stored<Idx> = GuardedEntity<StoreIdx, Idx>;

/// A wrapper around an optional `&mut dyn` [`ResourceLimiter`], that exists
/// both to make types a little easier to read and to provide a `Debug` impl so
/// that `#[derive(Debug)]` works on structs that contain it.
pub struct ResourceLimiterRef<'a>(Option<&'a mut (dyn ResourceLimiter)>);
impl<'a> core::fmt::Debug for ResourceLimiterRef<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ResourceLimiterRef(...)")
    }
}

impl<'a> ResourceLimiterRef<'a> {
    pub fn as_resource_limiter(&mut self) -> &mut Option<&'a mut dyn ResourceLimiter> {
        &mut self.0
    }
}

/// A wrapper around a boxed `dyn FnMut(&mut T)` returning a `&mut dyn`
/// [`ResourceLimiter`]; in other words a function that one can call to retrieve
/// a [`ResourceLimiter`] from the [`Store`] object's user data type `T`.
///
/// This wrapper exists both to make types a little easier to read and to
/// provide a `Debug` impl so that `#[derive(Debug)]` works on structs that
/// contain it.
struct ResourceLimiterQuery<T>(Box<dyn FnMut(&mut T) -> &mut (dyn ResourceLimiter) + Send + Sync>);
impl<T> core::fmt::Debug for ResourceLimiterQuery<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ResourceLimiterQuery(...)")
    }
}
struct ExecutionSupplementQuery<T>(Box<dyn FnMut(&mut T, &mut ObservationCharge, u64, u64) -> Result<ExecutionSupplement, ExecutionObserverError> + Send + Sync>);
impl<T> core::fmt::Debug for ExecutionSupplementQuery<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "ExecutionSupplementQuery(...)") }
}

/// The store that owns all data associated to Wasm modules.
#[derive(Debug)]
pub struct Store<T> {
    /// All data that is not associated to `T`.
    ///
    /// # Note
    ///
    /// This is re-exported to the rest of the crate since
    /// it is used directly by the engine's executor.
    pub(crate) inner: StoreInner,
    /// Stored host function trampolines.
    trampolines: Arena<TrampolineIdx, TrampolineEntity<T>>,
    /// User provided host data owned by the [`Store`].
    data: T,
    /// User provided hook to retrieve a [`ResourceLimiter`].
    limiter: Option<ResourceLimiterQuery<T>>,
    execution_supplement: Option<ExecutionSupplementQuery<T>>,
}

/// The inner store that owns all data not associated to the host state.
#[derive(Debug)]
pub struct StoreInner {
    /// The unique store index.
    ///
    /// Used to protect against invalid entity indices.
    store_idx: StoreIdx,
    /// Stored Wasm or host functions.
    funcs: Arena<FuncIdx, FuncEntity>,
    /// Stored linear memories.
    memories: Arena<MemoryIdx, MemoryEntity>,
    /// Stored tables.
    tables: Arena<TableIdx, TableEntity>,
    /// Stored global variables.
    globals: Arena<GlobalIdx, GlobalEntity>,
    /// Stored module instances.
    instances: Arena<InstanceIdx, InstanceEntity>,
    /// Stored data segments.
    datas: Arena<DataSegmentIdx, DataSegmentEntity>,
    /// Stored data segments.
    elems: Arena<ElementSegmentIdx, ElementSegmentEntity>,
    /// Stored external objects for [`ExternRef`] types.
    ///
    /// [`ExternRef`]: [`crate::ExternRef`]
    extern_objects: Arena<ExternObjectIdx, ExternObjectEntity>,
    /// The [`Engine`] in use by the [`Store`].
    ///
    /// Amongst others the [`Engine`] stores the Wasm function definitions.
    engine: Engine,
    /// The fuel of the [`Store`].
    fuel: Fuel,
    /// Protocol-owned deterministic execution observer, disabled by default.
    execution_observer: Option<ExecutionObserver>,
}

impl StoreInner {
    pub(crate) fn execution_boundary_authorized(&self) -> bool {
        self.execution_observer.as_ref().map_or(true, |observer| observer.boundary_authorized)
    }

    pub(crate) fn execution_boundary_needs_capture(&self) -> bool {
        self.execution_observer.as_ref().map_or(false, |observer| {
            observer.pending.is_some() || observer.step_index % observer.interval.max(1) == 0
        })
    }

    pub(crate) fn authorize_unobserved_boundary(&mut self) {
        if let Some(observer) = self.execution_observer.as_mut() {
            observer.boundary_authorized = true;
        }
    }

    pub(crate) fn preflight_execution_boundary(&mut self) -> Result<(), ExecutionObserverError> {
        let Some(observer) = self.execution_observer.as_mut() else { return Ok(()) };
        if observer.retained_snapshots >= observer.maximum_snapshots {
            observer.error = Some(ExecutionObserverError::SnapshotLimitExceeded);
            return Err(ExecutionObserverError::SnapshotLimitExceeded)
        }
        Ok(())
    }

    pub(crate) fn authorize_execution_boundary(&mut self, charge: ObservationCharge) -> Result<(), ExecutionObserverError> {
        let Some(observer) = self.execution_observer.as_mut() else { return Ok(()) };
        let bytes = charge.total_bytes().ok_or(ExecutionObserverError::SnapshotLimitExceeded)?;
        let aggregate_bytes = observer.aggregate_bytes.checked_add(bytes).ok_or(ExecutionObserverError::SnapshotLimitExceeded)?;
        let work = charge.total_work().ok_or(ExecutionObserverError::SnapshotLimitExceeded)?;
        let aggregate_work = observer.aggregate_work.checked_add(work).ok_or(ExecutionObserverError::SnapshotLimitExceeded)?;
        if aggregate_bytes > observer.maximum_bytes || aggregate_work > observer.maximum_work {
            observer.error = Some(ExecutionObserverError::SnapshotLimitExceeded);
            return Err(ExecutionObserverError::SnapshotLimitExceeded)
        }
        observer.aggregate_bytes = aggregate_bytes;
        observer.aggregate_work = aggregate_work;
        observer.boundary_authorized = true;
        Ok(())
    }

    pub(crate) fn enter_execution_boundary(&mut self) -> Result<bool, ExecutionObserverError> {
        let Some(observer) = self.execution_observer.as_mut() else { return Ok(false) };
        observer.boundary_authorized = false;
        observer.enter_boundary()
    }

    pub(crate) fn fail_execution_observer(&mut self, error: ExecutionObserverError) {
        if let Some(observer) = self.execution_observer.as_mut() {
            if observer.error.is_none() {
                observer.error = Some(error);
            }
        }
    }
    pub(crate) fn push_execution_snapshot(&mut self, snapshot: ExecutionSnapshot) -> Result<(), ExecutionObserverError> {
        let observer = self.execution_observer.as_mut().expect("enabled observer must exist");
        observer.retained_snapshots = observer.retained_snapshots.checked_add(1)
            .ok_or(ExecutionObserverError::SnapshotLimitExceeded)?;
        let snapshot = alloc::sync::Arc::new(snapshot);
        if let Some(pre) = observer.pending.take() {
            let memory_expansion_bytes = snapshot.linear_memory.len().checked_sub(pre.linear_memory.len())
                .and_then(|bytes| u64::try_from(bytes).ok())
                .ok_or_else(|| {
                    observer.error = Some(ExecutionObserverError::UnsupportedState);
                    ExecutionObserverError::UnsupportedState
                })?;
            observer.transitions.push(ExecutionTransition {
                pre,
                post: alloc::sync::Arc::clone(&snapshot),
                memory_expansion_bytes,
            });
        }
        if observer.sampled_current {
            observer.pending = Some(snapshot);
        }
        Ok(())
    }
    pub(crate) fn execution_step_index(&self) -> u64 {
        self.execution_observer.as_ref().map_or(0, |observer| observer.step_index)
    }
    pub(crate) fn execution_supplement(&self) -> ExecutionSupplement {
        self.execution_observer.as_ref().map_or_else(ExecutionSupplement::default, |observer| observer.supplement.clone())
    }
    fn execution_reached_instances(
        &self,
        root: crate::Instance,
    ) -> Result<Vec<crate::Instance>, ExecutionObserverError> {
        let mut instances = alloc::vec![root];
        let mut cursor = 0_usize;
        while cursor < instances.len() {
            let instance = self.resolve_instance(&instances[cursor]);
            let mut function_index = 0_u32;
            while let Some(function) = instance.get_func(function_index) {
                if let crate::FuncEntity::Wasm(entity) = self.resolve_func(&function) {
                    let reached = *entity.instance();
                    if !instances.iter().any(|candidate| candidate == &reached) {
                        instances.push(reached);
                    }
                }
                function_index = function_index.checked_add(1).ok_or(ExecutionObserverError::UnsupportedState)?;
            }
            cursor = cursor.checked_add(1).ok_or(ExecutionObserverError::SnapshotLimitExceeded)?;
        }
        Ok(instances)
    }

    pub(crate) fn measure_execution_instance_states(
        &self,
        root: crate::Instance,
    ) -> Result<u64, ExecutionObserverError> {
        use core::mem::size_of;
        use crate::execution_trace::{
            ExecutionDataSegment,
            ExecutionElementSegment,
            ExecutionGlobal,
            ExecutionInstanceState,
            ExecutionMemory,
            ExecutionTable,
        };
        fn allocation<T>(count: usize) -> Result<u64, ExecutionObserverError> {
            count.checked_mul(size_of::<T>())
                .and_then(|bytes| u64::try_from(bytes).ok())
                .ok_or(ExecutionObserverError::SnapshotLimitExceeded)
        }
        let instances = self.execution_reached_instances(root)?;
        let mut bytes = allocation::<ExecutionInstanceState>(instances.len())?;
        let mut function_scan_work = 0_u64;
        let mut reference_scan_work = 0_u64;
        for handle in &instances {
            let instance = self.resolve_instance(handle);
            let mut function_count = 0_u32;
            while instance.get_func(function_count).is_some() {
                function_count = function_count.checked_add(1).ok_or(ExecutionObserverError::SnapshotLimitExceeded)?;
            }
            function_scan_work = function_scan_work.checked_add(u64::from(function_count))
                .ok_or(ExecutionObserverError::SnapshotLimitExceeded)?;
            let mut count = 0_usize;
            while let Some(memory) = instance.get_memory(u32::try_from(count).map_err(|_| ExecutionObserverError::UnsupportedState)?) {
                bytes = bytes.checked_add(u64::try_from(self.resolve_memory(&memory).data().len()).map_err(|_| ExecutionObserverError::SnapshotLimitExceeded)?)
                    .ok_or(ExecutionObserverError::SnapshotLimitExceeded)?;
                count = count.checked_add(1).ok_or(ExecutionObserverError::SnapshotLimitExceeded)?;
            }
            bytes = bytes.checked_add(allocation::<ExecutionMemory>(count)?).ok_or(ExecutionObserverError::SnapshotLimitExceeded)?;
            count = 0;
            while instance.get_global(u32::try_from(count).map_err(|_| ExecutionObserverError::UnsupportedState)?).is_some() {
                count = count.checked_add(1).ok_or(ExecutionObserverError::SnapshotLimitExceeded)?;
            }
            bytes = bytes.checked_add(allocation::<ExecutionGlobal>(count)?).ok_or(ExecutionObserverError::SnapshotLimitExceeded)?;
            count = 0;
            while let Some(table) = instance.get_table(u32::try_from(count).map_err(|_| ExecutionObserverError::UnsupportedState)?) {
                let table = self.resolve_table(&table);
                if table.ty().element() != crate::core::ValueType::FuncRef { return Err(ExecutionObserverError::UnsupportedState) }
                reference_scan_work = reference_scan_work.checked_add(u64::try_from(table.elements().len()).map_err(|_| ExecutionObserverError::SnapshotLimitExceeded)?)
                    .ok_or(ExecutionObserverError::SnapshotLimitExceeded)?;
                bytes = bytes.checked_add(allocation::<Option<crate::execution_trace::ExecutionFunctionRef>>(table.elements().len())?)
                    .ok_or(ExecutionObserverError::SnapshotLimitExceeded)?;
                count = count.checked_add(1).ok_or(ExecutionObserverError::SnapshotLimitExceeded)?;
            }
            bytes = bytes.checked_add(allocation::<ExecutionTable>(count)?).ok_or(ExecutionObserverError::SnapshotLimitExceeded)?;
            count = 0;
            while let Some(segment) = instance.get_data_segment(u32::try_from(count).map_err(|_| ExecutionObserverError::UnsupportedState)?) {
                bytes = bytes.checked_add(u64::try_from(self.resolve_data_segment(&segment).bytes().len()).map_err(|_| ExecutionObserverError::SnapshotLimitExceeded)?)
                    .ok_or(ExecutionObserverError::SnapshotLimitExceeded)?;
                count = count.checked_add(1).ok_or(ExecutionObserverError::SnapshotLimitExceeded)?;
            }
            bytes = bytes.checked_add(allocation::<ExecutionDataSegment>(count)?).ok_or(ExecutionObserverError::SnapshotLimitExceeded)?;
            count = 0;
            while let Some(segment) = instance.get_element_segment(u32::try_from(count).map_err(|_| ExecutionObserverError::UnsupportedState)?) {
                let segment = self.resolve_element_segment(&segment);
                if segment.ty() != crate::core::ValueType::FuncRef { return Err(ExecutionObserverError::UnsupportedState) }
                reference_scan_work = reference_scan_work.checked_add(u64::try_from(segment.items().len()).map_err(|_| ExecutionObserverError::SnapshotLimitExceeded)?)
                    .ok_or(ExecutionObserverError::SnapshotLimitExceeded)?;
                bytes = bytes.checked_add(allocation::<Option<crate::execution_trace::ExecutionFunctionRef>>(segment.items().len())?)
                    .ok_or(ExecutionObserverError::SnapshotLimitExceeded)?;
                count = count.checked_add(1).ok_or(ExecutionObserverError::SnapshotLimitExceeded)?;
            }
            bytes = bytes.checked_add(allocation::<ExecutionElementSegment>(count)?).ok_or(ExecutionObserverError::SnapshotLimitExceeded)?;
        }
        let resolution_work = reference_scan_work.checked_mul(function_scan_work)
            .and_then(|work| work.checked_add(function_scan_work))
            .and_then(|work| work.checked_mul(8))
            .ok_or(ExecutionObserverError::SnapshotLimitExceeded)?;
        bytes = bytes.checked_add(resolution_work).ok_or(ExecutionObserverError::SnapshotLimitExceeded)?;
        Ok(bytes)
    }

    pub(crate) fn measure_execution_instance_canonical_bytes(
        &self,
        root: crate::Instance,
    ) -> Result<u64, ExecutionObserverError> {
        fn add(total: &mut u64, amount: u64) -> Result<(), ExecutionObserverError> {
            *total = total.checked_add(amount).ok_or(ExecutionObserverError::SnapshotLimitExceeded)?;
            Ok(())
        }
        fn len(value: usize) -> Result<u64, ExecutionObserverError> {
            u32::try_from(value).map(u64::from).map_err(|_| ExecutionObserverError::UnsupportedState)
        }
        let instances = self.execution_reached_instances(root)?;
        let _ = u32::try_from(instances.len()).map_err(|_| ExecutionObserverError::UnsupportedState)?;
        let mut bytes = 4_u64;
        for handle in &instances {
            let instance = self.resolve_instance(handle);
            add(&mut bytes, 8)?;
            let mut index = 0_u32;
            while let Some(memory) = instance.get_memory(index) {
                let memory = self.resolve_memory(&memory);
                add(&mut bytes, 13 + if memory.ty().maximum_pages().is_some() { 4 } else { 0 })?;
                add(&mut bytes, len(memory.data().len())?)?;
                index = index.checked_add(1).ok_or(ExecutionObserverError::UnsupportedState)?;
            }
            add(&mut bytes, 4)?;
            index = 0;
            while let Some(global) = instance.get_global(index) {
                let global = self.resolve_global(&global);
                add(&mut bytes, match global.ty().content() {
                    crate::core::ValueType::I32 => 10,
                    crate::core::ValueType::I64 => 14,
                    _ => return Err(ExecutionObserverError::UnsupportedState),
                })?;
                index = index.checked_add(1).ok_or(ExecutionObserverError::UnsupportedState)?;
            }
            add(&mut bytes, 4)?;
            index = 0;
            while let Some(table) = instance.get_table(index) {
                let table = self.resolve_table(&table);
                if table.ty().element() != crate::core::ValueType::FuncRef { return Err(ExecutionObserverError::UnsupportedState) }
                add(&mut bytes, 13 + if table.ty().maximum().is_some() { 4 } else { 0 })?;
                for &raw in table.elements() {
                    add(&mut bytes, if crate::FuncRef::from(raw).is_null() { 1 } else { 9 })?;
                }
                index = index.checked_add(1).ok_or(ExecutionObserverError::UnsupportedState)?;
            }
            add(&mut bytes, 4)?;
            index = 0;
            while let Some(segment) = instance.get_data_segment(index) {
                let segment = self.resolve_data_segment(&segment);
                add(&mut bytes, 9)?;
                add(&mut bytes, len(segment.bytes().len())?)?;
                index = index.checked_add(1).ok_or(ExecutionObserverError::UnsupportedState)?;
            }
            add(&mut bytes, 4)?;
            index = 0;
            while let Some(segment) = instance.get_element_segment(index) {
                let segment = self.resolve_element_segment(&segment);
                if segment.ty() != crate::core::ValueType::FuncRef { return Err(ExecutionObserverError::UnsupportedState) }
                add(&mut bytes, 9)?;
                for expression in segment.items() {
                    let raw = expression.eval_with_context(
                        |global_index| self.resolve_global(&instance.get_global(global_index).expect("validated global index")).get(),
                        |function_index| crate::FuncRef::new(instance.get_func(function_index).expect("validated function index")),
                    ).ok_or(ExecutionObserverError::UnsupportedState)?;
                    add(&mut bytes, if crate::FuncRef::from(raw).is_null() { 1 } else { 9 })?;
                }
                index = index.checked_add(1).ok_or(ExecutionObserverError::UnsupportedState)?;
            }
        }
        Ok(bytes)
    }

    pub(crate) fn capture_execution_instance_states(
        &self,
        root: crate::Instance,
        maximum_bytes: u64,
    ) -> Result<Vec<crate::execution_trace::ExecutionInstanceState>, ExecutionObserverError> {
        use crate::execution_trace::{
            ExecutionDataSegment,
            ExecutionElementSegment,
            ExecutionFunctionRef,
            ExecutionInstanceState,
            ExecutionMemory,
            ExecutionTable,
        };

        fn retain(bytes: &mut u64, additional: u64, maximum: u64) -> Result<(), ExecutionObserverError> {
            *bytes = bytes.checked_add(additional).ok_or(ExecutionObserverError::SnapshotLimitExceeded)?;
            if *bytes > maximum {
                return Err(ExecutionObserverError::SnapshotLimitExceeded)
            }
            Ok(())
        }

        let measured = self.measure_execution_instance_states(root)?;
        if measured > maximum_bytes { return Err(ExecutionObserverError::SnapshotLimitExceeded) }
        let mut retained = 0_u64;
        let instances = self.execution_reached_instances(root)?;

        let canonical_ref = |function: crate::Func| -> Result<ExecutionFunctionRef, ExecutionObserverError> {
            for (instance_index, instance) in instances.iter().enumerate() {
                let entity = self.resolve_instance(instance);
                let mut function_index = 0_u32;
                while let Some(candidate) = entity.get_func(function_index) {
                    if candidate.as_inner() == function.as_inner() {
                        return Ok(ExecutionFunctionRef {
                            instance_index: u32::try_from(instance_index).map_err(|_| ExecutionObserverError::UnsupportedState)?,
                            function_index,
                        })
                    }
                    function_index = function_index.checked_add(1).ok_or(ExecutionObserverError::UnsupportedState)?;
                }
            }
            Err(ExecutionObserverError::UnsupportedState)
        };

        let mut states = Vec::with_capacity(instances.len());
        for (instance_index, handle) in instances.iter().enumerate() {
            let instance = self.resolve_instance(handle);
            let mut state = ExecutionInstanceState {
                instance_index: u32::try_from(instance_index).map_err(|_| ExecutionObserverError::UnsupportedState)?,
                memories: Vec::new(),
                globals: Vec::new(),
                tables: Vec::new(),
                data_segments: Vec::new(),
                element_segments: Vec::new(),
            };
            retain(&mut retained, 16, maximum_bytes)?;

            let mut memory_index = 0_u32;
            while let Some(memory) = instance.get_memory(memory_index) {
                let memory = self.resolve_memory(&memory);
                let ty = memory.ty();
                retain(&mut retained, 16, maximum_bytes)?;
                retain(&mut retained, u64::try_from(memory.data().len()).map_err(|_| ExecutionObserverError::SnapshotLimitExceeded)?, maximum_bytes)?;
                state.memories.push(ExecutionMemory {
                    memory_index,
                    initial_pages: u32::from(ty.initial_pages()),
                    maximum_pages: ty.maximum_pages().map(u32::from),
                    bytes: memory.data().to_vec(),
                });
                memory_index = memory_index.checked_add(1).ok_or(ExecutionObserverError::UnsupportedState)?;
            }

            let mut global_index = 0_u32;
            while let Some(global) = instance.get_global(global_index) {
                let global = self.resolve_global(&global);
                let value_type = match global.ty().content() {
                    crate::core::ValueType::I32 => crate::execution_trace::ExecutionValueType::I32,
                    crate::core::ValueType::I64 => crate::execution_trace::ExecutionValueType::I64,
                    _ => return Err(ExecutionObserverError::UnsupportedState),
                };
                retain(&mut retained, match value_type {
                    crate::execution_trace::ExecutionValueType::I32 => 10,
                    crate::execution_trace::ExecutionValueType::I64 => 14,
                }, maximum_bytes)?;
                state.globals.push(crate::execution_trace::ExecutionGlobal {
                    global_index,
                    mutable: global.ty().mutability().is_mut(),
                    value: crate::execution_trace::ExecutionValue {
                        value_type,
                        bits: u64::from(global.get_untyped()),
                    },
                });
                global_index = global_index.checked_add(1).ok_or(ExecutionObserverError::UnsupportedState)?;
            }

            let mut table_index = 0_u32;
            while let Some(table) = instance.get_table(table_index) {
                let table = self.resolve_table(&table);
                if table.ty().element() != crate::core::ValueType::FuncRef {
                    return Err(ExecutionObserverError::UnsupportedState)
                }
                retain(&mut retained, 16, maximum_bytes)?;
                let mut elements = Vec::with_capacity(table.elements().len());
                for &raw in table.elements() {
                    retain(&mut retained, 9, maximum_bytes)?;
                    let reference = crate::FuncRef::from(raw);
                    elements.push(reference.func().copied().map(&canonical_ref).transpose()?);
                }
                let ty = table.ty();
                state.tables.push(ExecutionTable {
                    table_index,
                    minimum: ty.minimum(),
                    maximum: ty.maximum(),
                    elements,
                });
                table_index = table_index.checked_add(1).ok_or(ExecutionObserverError::UnsupportedState)?;
            }

            let mut segment_index = 0_u32;
            while let Some(segment) = instance.get_data_segment(segment_index) {
                let segment = self.resolve_data_segment(&segment);
                retain(&mut retained, 9, maximum_bytes)?;
                retain(&mut retained, u64::try_from(segment.bytes().len()).map_err(|_| ExecutionObserverError::SnapshotLimitExceeded)?, maximum_bytes)?;
                state.data_segments.push(ExecutionDataSegment {
                    segment_index,
                    dropped: segment.is_dropped(),
                    bytes: segment.bytes().to_vec(),
                });
                segment_index = segment_index.checked_add(1).ok_or(ExecutionObserverError::UnsupportedState)?;
            }

            let mut segment_index = 0_u32;
            while let Some(segment) = instance.get_element_segment(segment_index) {
                let segment = self.resolve_element_segment(&segment);
                if segment.ty() != crate::core::ValueType::FuncRef {
                    return Err(ExecutionObserverError::UnsupportedState)
                }
                retain(&mut retained, 9, maximum_bytes)?;
                let mut elements = Vec::with_capacity(segment.items().len());
                for expression in segment.items() {
                    retain(&mut retained, 9, maximum_bytes)?;
                    let raw = expression.eval_with_context(
                        |index| self.resolve_global(&instance.get_global(index).expect("validated global index")).get(),
                        |index| crate::FuncRef::new(instance.get_func(index).expect("validated function index")),
                    ).ok_or(ExecutionObserverError::UnsupportedState)?;
                    let reference = crate::FuncRef::from(raw);
                    elements.push(reference.func().copied().map(&canonical_ref).transpose()?);
                }
                state.element_segments.push(ExecutionElementSegment {
                    segment_index,
                    dropped: segment.is_dropped(),
                    elements,
                });
                segment_index = segment_index.checked_add(1).ok_or(ExecutionObserverError::UnsupportedState)?;
            }
            states.push(state);
        }
        Ok(states)
    }
    pub(crate) fn terminal_observation_charge(&self, instance: crate::Instance, result_count: usize) -> Result<Option<ObservationCharge>, ExecutionObserverError> {
        let Some(pre) = self.execution_observer.as_ref().and_then(|observer| observer.pending.as_ref()) else {
            return Ok(None)
        };
        let observer = self.execution_observer.as_ref().expect("observer exists");
        if observer.retained_snapshots >= observer.maximum_snapshots {
            return Err(ExecutionObserverError::SnapshotLimitExceeded)
        }
        let result_types = pre.value_stack.get(pre.value_stack.len().checked_sub(result_count)
            .ok_or(ExecutionObserverError::UnsupportedState)?..)
            .ok_or(ExecutionObserverError::UnsupportedState)?;
        let value_bytes = result_types.iter().try_fold(4_u64, |bytes, value| {
            bytes.checked_add(match value.value_type {
                crate::execution_trace::ExecutionValueType::I32 => 5,
                crate::execution_trace::ExecutionValueType::I64 => 9,
            }).ok_or(ExecutionObserverError::SnapshotLimitExceeded)
        })?;
        let root_instance = instance;
        let instance = self.resolve_instance(&root_instance);
        let memory_bytes = instance.get_memory(0).map_or(0, |memory| self.resolve_memory(&memory).data().len());
        if instance.get_memory(1).is_some() { return Err(ExecutionObserverError::UnsupportedState) }
        let mut global_count = 0_usize;
        let mut global_bytes = 4_u64;
        while let Some(global) = instance.get_global(u32::try_from(global_count).map_err(|_| ExecutionObserverError::UnsupportedState)?) {
            let entity = self.resolve_global(&global);
            let value_bytes = match entity.ty().content() {
                crate::core::ValueType::I32 => 5,
                crate::core::ValueType::I64 => 9,
                _ => return Err(ExecutionObserverError::UnsupportedState),
            };
            global_bytes = global_bytes.checked_add(5 + value_bytes).ok_or(ExecutionObserverError::SnapshotLimitExceeded)?;
            global_count = global_count.checked_add(1).ok_or(ExecutionObserverError::SnapshotLimitExceeded)?;
        }
        let instance_state_bytes = self.measure_execution_instance_states(root_instance)?;
        let arbitration_engine_canonical_bytes = self.measure_execution_instance_canonical_bytes(root_instance)?;
        Ok(Some(ObservationCharge {
            collect: true,
            value_bytes,
            frame_bytes: 4,
            local_bytes: 0,
            global_bytes,
            memory_bytes: 4_u64.checked_add(u64::try_from(memory_bytes).map_err(|_| ExecutionObserverError::SnapshotLimitExceeded)?).ok_or(ExecutionObserverError::SnapshotLimitExceeded)?,
            instance_state_bytes,
            arbitration_engine_canonical_bytes,
            host_state_bytes: pre.supplement.arbitration_host_state_bytes,
            storage_overlay_bytes: pre.supplement.storage_overlay.iter().try_fold(0_u64, |total, (key, value)| {
                let value_len = value.as_ref().map_or(0, Vec::len);
                let bytes = key.len().checked_add(value_len).and_then(|bytes| bytes.checked_add(1))
                    .and_then(|bytes| u64::try_from(bytes).ok()).ok_or(ExecutionObserverError::SnapshotLimitExceeded)?;
                total.checked_add(bytes).ok_or(ExecutionObserverError::SnapshotLimitExceeded)
            })?,
            instruction_bytes: 4,
            retained_instruction_bytes: 2,
        }))
    }
    pub(crate) fn finalize_return_transition(
        &mut self,
        instance: crate::Instance,
        values: &[wasmi_core::UntypedValue],
    ) -> Result<(), ExecutionObserverError> {
        let Some(observer) = self.execution_observer.as_mut() else { return Ok(()) };
        let Some(pre) = observer.pending.take() else { return Ok(()) };
        let supplement = observer.supplement.clone();
        let mut post = ExecutionSnapshot {
            step_index: pre.step_index.checked_add(1).ok_or(ExecutionObserverError::StepCounterOverflow)?,
            program_counter: u64::MAX,
            value_stack: Vec::new(),
            call_frames: Vec::new(),
            linear_memory: Vec::new(),
            globals: Vec::new(),
            arbitration_instances: Vec::new(),
            control_stack: Vec::new(),
            canonical_instruction: alloc::vec![0xFF, 0xFF],
            instruction_fuel: 0,
            memory_expansion_bytes: 0,
            supplement,
        };
        let types = pre.value_stack.get(pre.value_stack.len().checked_sub(values.len())
            .ok_or(ExecutionObserverError::UnsupportedState)?..)
            .ok_or(ExecutionObserverError::UnsupportedState)?;
        post.value_stack = types.iter().zip(values).map(|(value, raw)| crate::execution_trace::ExecutionValue {
            value_type: value.value_type,
            bits: u64::from(*raw),
        }).collect();
        let root_instance = instance;
        let instance = self.resolve_instance(&root_instance);
        let memory = instance.get_memory(0);
        let mut global_handles = alloc::vec::Vec::new();
        let mut global_index = 0_u32;
        while let Some(global) = instance.get_global(global_index) {
            global_handles.push(global);
            global_index = global_index.checked_add(1).ok_or(ExecutionObserverError::UnsupportedState)?;
        }
        if instance.get_memory(1).is_some() {
            return Err(ExecutionObserverError::UnsupportedState)
        }
        if let Some(memory) = memory {
            post.linear_memory.extend_from_slice(self.resolve_memory(&memory).data());
        }
        for (global_index, global) in global_handles.into_iter().enumerate() {
            let entity = self.resolve_global(&global);
            let value_type = match entity.ty().content() {
                crate::core::ValueType::I32 => crate::execution_trace::ExecutionValueType::I32,
                crate::core::ValueType::I64 => crate::execution_trace::ExecutionValueType::I64,
                _ => return Err(ExecutionObserverError::UnsupportedState),
            };
            post.globals.push(crate::execution_trace::ExecutionGlobal {
                global_index: u32::try_from(global_index).map_err(|_| ExecutionObserverError::UnsupportedState)?,
                mutable: entity.ty().mutability().is_mut(),
                value: crate::execution_trace::ExecutionValue { value_type, bits: u64::from(entity.get_untyped()) },
            });
        }
        let instance_state_bytes = self.measure_execution_instance_states(root_instance)?;
        post.arbitration_instances = self.capture_execution_instance_states(root_instance, instance_state_bytes)?;
        let memory_expansion_bytes = post.linear_memory.len().checked_sub(pre.linear_memory.len())
            .and_then(|bytes| u64::try_from(bytes).ok())
            .ok_or(ExecutionObserverError::UnsupportedState)?;
        let observer = self.execution_observer.as_mut().expect("observer exists");
        observer.retained_snapshots = observer.retained_snapshots.checked_add(1)
            .ok_or(ExecutionObserverError::SnapshotLimitExceeded)?;
        observer.transitions.push(ExecutionTransition {
                pre,
                post: alloc::sync::Arc::new(post),
                memory_expansion_bytes,
            });
        Ok(())
    }

    pub(crate) fn refuse_trapped_transition(&mut self) {
        if let Some(observer) = self.execution_observer.as_mut() {
            observer.pending = None;
            if observer.error.is_none() {
                observer.error = Some(ExecutionObserverError::UnsupportedState);
            }
        }
    }
}

#[test]
fn test_store_is_send_sync() {
    const _: () = {
        #[allow(clippy::extra_unused_type_parameters)]
        fn assert_send<T: Send>() {}
        #[allow(clippy::extra_unused_type_parameters)]
        fn assert_sync<T: Sync>() {}
        let _ = assert_send::<Store<()>>;
        let _ = assert_sync::<Store<()>>;
    };
}

/// An error that may be encountered when operating on the [`Store`].
#[derive(Debug, Clone)]
pub enum FuelError {
    /// Raised when trying to use any of the `fuel` methods while fuel metering is disabled.
    FuelMeteringDisabled,
    /// Raised when trying to consume more fuel than is available in the [`Store`].
    OutOfFuel,
}

impl fmt::Display for FuelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FuelMeteringDisabled => write!(f, "fuel metering is disabled"),
            Self::OutOfFuel => write!(f, "all fuel consumed"),
        }
    }
}

impl FuelError {
    /// Returns an error indicating that fuel metering has been disabled.
    ///
    /// # Note
    ///
    /// This method exists to indicate that this execution path is cold.
    #[cold]
    pub fn fuel_metering_disabled() -> Self {
        Self::FuelMeteringDisabled
    }

    /// Returns an error indicating that too much fuel has been consumed.
    ///
    /// # Note
    ///
    /// This method exists to indicate that this execution path is cold.
    #[cold]
    pub fn out_of_fuel() -> Self {
        Self::OutOfFuel
    }
}

/// The remaining and consumed fuel counters.
#[derive(Debug, Default, Copy, Clone)]
pub struct Fuel {
    /// The remaining fuel.
    remaining: u64,
    /// The total amount of fuel so far.
    total: u64,
}

impl Fuel {
    /// Adds `delta` quantity of fuel to the remaining [`Fuel`].
    ///
    /// # Panics
    ///
    /// If this overflows the [`Fuel`] counter.
    pub fn add_fuel(&mut self, delta: u64) {
        self.total = self.total.checked_add(delta).unwrap_or_else(|| {
            panic!(
                "encountered total fuel overflow: fuel = {}, delta = {delta}",
                self.total
            )
        });
        // No need to check as well since `self.total >= self.remaining`.
        self.remaining = self.remaining.wrapping_add(delta);
    }

    /// Returns the amount of [`Fuel`] consumed by executions of the [`Store`] so far.
    pub fn fuel_consumed(&self) -> u64 {
        self.total.wrapping_sub(self.remaining)
    }

    /// Returns `Ok` if enough fuel is remaining to satisfy `delta` fuel consumption.
    ///
    /// Returns a [`TrapCode::OutOfFuel`] error otherwise.
    pub fn sufficient_fuel(&self, delta: u64) -> Result<(), TrapCode> {
        self.remaining
            .checked_sub(delta)
            .map(|_| ())
            .ok_or(TrapCode::OutOfFuel)
    }

    /// Synthetically consumes an amount of [`Fuel`] for the [`Store`].
    ///
    /// Returns the remaining amount of [`Fuel`] after this operation.
    pub fn consume_fuel(&mut self, delta: u64) -> Result<u64, TrapCode> {
        self.remaining = self
            .remaining
            .checked_sub(delta)
            .ok_or(TrapCode::OutOfFuel)?;
        Ok(self.remaining)
    }
}

impl StoreInner {
    /// Creates a new [`StoreInner`] for the given [`Engine`].
    pub fn new(engine: &Engine) -> Self {
        StoreInner {
            engine: engine.clone(),
            store_idx: StoreIdx::new(),
            funcs: Arena::new(),
            memories: Arena::new(),
            tables: Arena::new(),
            globals: Arena::new(),
            instances: Arena::new(),
            datas: Arena::new(),
            elems: Arena::new(),
            extern_objects: Arena::new(),
            fuel: Fuel::default(),
            execution_observer: None,
        }
    }

    /// Returns the [`Engine`] that this store is associated with.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Returns a shared reference to the [`Fuel`] counters.
    pub fn fuel(&self) -> &Fuel {
        &self.fuel
    }

    /// Returns an exclusive reference to the [`Fuel`] counters.
    pub fn fuel_mut(&mut self) -> &mut Fuel {
        &mut self.fuel
    }

    /// Wraps an entitiy `Idx` (index type) as a [`Stored<Idx>`] type.
    ///
    /// # Note
    ///
    /// [`Stored<Idx>`] associates an `Idx` type with the internal store index.
    /// This way wrapped indices cannot be misused with incorrect [`Store`] instances.
    fn wrap_stored<Idx>(&self, entity_idx: Idx) -> Stored<Idx> {
        Stored::new(self.store_idx, entity_idx)
    }

    /// Unwraps the given [`Stored<Idx>`] reference and returns the `Idx`.
    ///
    /// # Panics
    ///
    /// If the [`Stored<Idx>`] does not originate from this [`Store`].
    fn unwrap_stored<Idx>(&self, stored: &Stored<Idx>) -> Idx
    where
        Idx: ArenaIndex + Debug,
    {
        stored.entity_index(self.store_idx).unwrap_or_else(|| {
            panic!(
                "entity reference ({:?}) does not belong to store {:?}",
                stored, self.store_idx,
            )
        })
    }

    /// Allocates a new [`GlobalEntity`] and returns a [`Global`] reference to it.
    pub fn alloc_global(&mut self, global: GlobalEntity) -> Global {
        let global = self.globals.alloc(global);
        Global::from_inner(self.wrap_stored(global))
    }

    /// Allocates a new [`TableEntity`] and returns a [`Table`] reference to it.
    pub fn alloc_table(&mut self, table: TableEntity) -> Table {
        let table = self.tables.alloc(table);
        Table::from_inner(self.wrap_stored(table))
    }

    /// Allocates a new [`MemoryEntity`] and returns a [`Memory`] reference to it.
    pub fn alloc_memory(&mut self, memory: MemoryEntity) -> Memory {
        let memory = self.memories.alloc(memory);
        Memory::from_inner(self.wrap_stored(memory))
    }

    /// Allocates a new [`DataSegmentEntity`] and returns a [`DataSegment`] reference to it.
    pub fn alloc_data_segment(&mut self, segment: DataSegmentEntity) -> DataSegment {
        let segment = self.datas.alloc(segment);
        DataSegment::from_inner(self.wrap_stored(segment))
    }

    /// Allocates a new [`ElementSegmentEntity`] and returns a [`ElementSegment`] reference to it.
    pub(super) fn alloc_element_segment(
        &mut self,
        segment: ElementSegmentEntity,
    ) -> ElementSegment {
        let segment = self.elems.alloc(segment);
        ElementSegment::from_inner(self.wrap_stored(segment))
    }

    /// Allocates a new [`ExternObjectEntity`] and returns a [`ExternObject`] reference to it.
    pub(super) fn alloc_extern_object(&mut self, object: ExternObjectEntity) -> ExternObject {
        let object = self.extern_objects.alloc(object);
        ExternObject::from_inner(self.wrap_stored(object))
    }

    /// Allocates a new uninitialized [`InstanceEntity`] and returns an [`Instance`] reference to it.
    ///
    /// # Note
    ///
    /// - This will create an uninitialized dummy [`InstanceEntity`] as a place holder
    ///   for the returned [`Instance`]. Using this uninitialized [`Instance`] will result
    ///   in a runtime panic.
    /// - The returned [`Instance`] must later be initialized via the [`StoreInner::initialize_instance`]
    ///   method. Afterwards the [`Instance`] may be used.
    pub fn alloc_instance(&mut self) -> Instance {
        let instance = self.instances.alloc(InstanceEntity::uninitialized());
        Instance::from_inner(self.wrap_stored(instance))
    }

    /// Initializes the [`Instance`] using the given [`InstanceEntity`].
    ///
    /// # Note
    ///
    /// After this operation the [`Instance`] is initialized and can be used.
    ///
    /// # Panics
    ///
    /// - If the [`Instance`] does not belong to the [`Store`].
    /// - If the [`Instance`] is unknown to the [`Store`].
    /// - If the [`Instance`] has already been initialized.
    /// - If the given [`InstanceEntity`] is itself not initialized, yet.
    pub fn initialize_instance(&mut self, instance: Instance, init: InstanceEntity) {
        assert!(
            init.is_initialized(),
            "encountered an uninitialized new instance entity: {init:?}",
        );
        let idx = self.unwrap_stored(instance.as_inner());
        let uninit = self
            .instances
            .get_mut(idx)
            .unwrap_or_else(|| panic!("missing entity for the given instance: {instance:?}"));
        assert!(
            !uninit.is_initialized(),
            "encountered an already initialized instance: {uninit:?}",
        );
        *uninit = init;
    }

    /// Returns a shared reference to the entity indexed by the given `idx`.
    ///
    /// # Panics
    ///
    /// - If the indexed entity does not originate from this [`Store`].
    /// - If the entity index cannot be resolved to its entity.
    fn resolve<'a, Idx, Entity>(
        &self,
        idx: &Stored<Idx>,
        entities: &'a Arena<Idx, Entity>,
    ) -> &'a Entity
    where
        Idx: ArenaIndex + Debug,
    {
        let idx = self.unwrap_stored(idx);
        entities
            .get(idx)
            .unwrap_or_else(|| panic!("failed to resolve stored entity: {idx:?}"))
    }

    /// Returns an exclusive reference to the entity indexed by the given `idx`.
    ///
    /// # Note
    ///
    /// Due to borrow checking issues this method takes an already unwrapped
    /// `Idx` unlike the [`StoreInner::resolve`] method.
    ///
    /// # Panics
    ///
    /// - If the entity index cannot be resolved to its entity.
    fn resolve_mut<Idx, Entity>(idx: Idx, entities: &mut Arena<Idx, Entity>) -> &mut Entity
    where
        Idx: ArenaIndex + Debug,
    {
        entities
            .get_mut(idx)
            .unwrap_or_else(|| panic!("failed to resolve stored entity: {idx:?}"))
    }

    /// Returns the [`FuncType`] associated to the given [`DedupFuncType`].
    ///
    /// # Panics
    ///
    /// - If the [`DedupFuncType`] does not originate from this [`Store`].
    /// - If the [`DedupFuncType`] cannot be resolved to its entity.
    pub fn resolve_func_type(&self, func_type: &DedupFuncType) -> FuncType {
        self.resolve_func_type_with(func_type, FuncType::clone)
    }

    /// Calls `f` on the [`FuncType`] associated to the given [`DedupFuncType`] and returns the result.
    ///
    /// # Panics
    ///
    /// - If the [`DedupFuncType`] does not originate from this [`Store`].
    /// - If the [`DedupFuncType`] cannot be resolved to its entity.
    pub fn resolve_func_type_with<R>(
        &self,
        func_type: &DedupFuncType,
        f: impl FnOnce(&FuncType) -> R,
    ) -> R {
        self.engine.resolve_func_type(func_type, f)
    }

    /// Returns a shared reference to the [`GlobalEntity`] associated to the given [`Global`].
    ///
    /// # Panics
    ///
    /// - If the [`Global`] does not originate from this [`Store`].
    /// - If the [`Global`] cannot be resolved to its entity.
    pub fn resolve_global(&self, global: &Global) -> &GlobalEntity {
        self.resolve(global.as_inner(), &self.globals)
    }

    /// Returns an exclusive reference to the [`GlobalEntity`] associated to the given [`Global`].
    ///
    /// # Panics
    ///
    /// - If the [`Global`] does not originate from this [`Store`].
    /// - If the [`Global`] cannot be resolved to its entity.
    pub fn resolve_global_mut(&mut self, global: &Global) -> &mut GlobalEntity {
        let idx = self.unwrap_stored(global.as_inner());
        Self::resolve_mut(idx, &mut self.globals)
    }

    /// Returns a shared reference to the [`TableEntity`] associated to the given [`Table`].
    ///
    /// # Panics
    ///
    /// - If the [`Table`] does not originate from this [`Store`].
    /// - If the [`Table`] cannot be resolved to its entity.
    pub fn resolve_table(&self, table: &Table) -> &TableEntity {
        self.resolve(table.as_inner(), &self.tables)
    }

    /// Returns an exclusive reference to the [`TableEntity`] associated to the given [`Table`].
    ///
    /// # Panics
    ///
    /// - If the [`Table`] does not originate from this [`Store`].
    /// - If the [`Table`] cannot be resolved to its entity.
    pub fn resolve_table_mut(&mut self, table: &Table) -> &mut TableEntity {
        let idx = self.unwrap_stored(table.as_inner());
        Self::resolve_mut(idx, &mut self.tables)
    }

    /// Returns an exclusive reference to the [`TableEntity`] associated to the given [`Table`].
    ///
    /// # Panics
    ///
    /// - If the [`Table`] does not originate from this [`Store`].
    /// - If the [`Table`] cannot be resolved to its entity.
    pub fn resolve_table_pair_mut(
        &mut self,
        fst: &Table,
        snd: &Table,
    ) -> (&mut TableEntity, &mut TableEntity) {
        let fst = self.unwrap_stored(fst.as_inner());
        let snd = self.unwrap_stored(snd.as_inner());
        self.tables.get_pair_mut(fst, snd).unwrap_or_else(|| {
            panic!("failed to resolve stored pair of entities: {fst:?} and {snd:?}")
        })
    }

    /// Returns a triple of:
    ///
    /// - An exclusive reference to the [`TableEntity`] associated to the given [`Table`].
    /// - A shared reference to the [`ElementSegmentEntity`] associated to the given [`ElementSegment`].
    ///
    /// # Note
    ///
    /// This method exists to properly handle use cases where
    /// otherwise the Rust borrow-checker would not accept.
    ///
    /// # Panics
    ///
    /// - If the [`Table`] does not originate from this [`Store`].
    /// - If the [`Table`] cannot be resolved to its entity.
    /// - If the [`ElementSegment`] does not originate from this [`Store`].
    /// - If the [`ElementSegment`] cannot be resolved to its entity.
    pub(super) fn resolve_table_element(
        &mut self,
        table: &Table,
        segment: &ElementSegment,
    ) -> (&mut TableEntity, &ElementSegmentEntity) {
        let table_idx = self.unwrap_stored(table.as_inner());
        let elem_idx = segment.as_inner();
        let elem = self.resolve(elem_idx, &self.elems);
        let table = Self::resolve_mut(table_idx, &mut self.tables);
        (table, elem)
    }

    /// Returns a triple of:
    ///
    /// - A shared reference to the [`InstanceEntity`] associated to the given [`Instance`].
    /// - An exclusive reference to the [`TableEntity`] associated to the given [`Table`].
    /// - A shared reference to the [`ElementSegmentEntity`] associated to the given [`ElementSegment`].
    ///
    /// # Note
    ///
    /// This method exists to properly handle use cases where
    /// otherwise the Rust borrow-checker would not accept.
    ///
    /// # Panics
    ///
    /// - If the [`Instance`] does not originate from this [`Store`].
    /// - If the [`Instance`] cannot be resolved to its entity.
    /// - If the [`Table`] does not originate from this [`Store`].
    /// - If the [`Table`] cannot be resolved to its entity.
    /// - If the [`ElementSegment`] does not originate from this [`Store`].
    /// - If the [`ElementSegment`] cannot be resolved to its entity.
    pub(super) fn resolve_instance_table_element(
        &mut self,
        instance: &Instance,
        table: &Table,
        segment: &ElementSegment,
    ) -> (&InstanceEntity, &mut TableEntity, &ElementSegmentEntity) {
        let mem_idx = self.unwrap_stored(table.as_inner());
        let data_idx = segment.as_inner();
        let instance_idx = instance.as_inner();
        let instance = self.resolve(instance_idx, &self.instances);
        let data = self.resolve(data_idx, &self.elems);
        let mem = Self::resolve_mut(mem_idx, &mut self.tables);
        (instance, mem, data)
    }

    /// Returns a shared reference to the [`ElementSegmentEntity`] associated to the given [`ElementSegment`].
    ///
    /// # Panics
    ///
    /// - If the [`ElementSegment`] does not originate from this [`Store`].
    /// - If the [`ElementSegment`] cannot be resolved to its entity.
    #[allow(unused)] // Note: We allow this unused API to exist to uphold code symmetry.
    pub fn resolve_element_segment(&self, segment: &ElementSegment) -> &ElementSegmentEntity {
        self.resolve(segment.as_inner(), &self.elems)
    }

    /// Returns an exclusive reference to the [`ElementSegmentEntity`] associated to the given [`ElementSegment`].
    ///
    /// # Panics
    ///
    /// - If the [`ElementSegment`] does not originate from this [`Store`].
    /// - If the [`ElementSegment`] cannot be resolved to its entity.
    pub fn resolve_element_segment_mut(
        &mut self,
        segment: &ElementSegment,
    ) -> &mut ElementSegmentEntity {
        let idx = self.unwrap_stored(segment.as_inner());
        Self::resolve_mut(idx, &mut self.elems)
    }

    /// Returns a shared reference to the [`MemoryEntity`] associated to the given [`Memory`].
    ///
    /// # Panics
    ///
    /// - If the [`Memory`] does not originate from this [`Store`].
    /// - If the [`Memory`] cannot be resolved to its entity.
    pub fn resolve_memory(&self, memory: &Memory) -> &MemoryEntity {
        self.resolve(memory.as_inner(), &self.memories)
    }

    /// Returns an exclusive reference to the [`MemoryEntity`] associated to the given [`Memory`].
    ///
    /// # Panics
    ///
    /// - If the [`Memory`] does not originate from this [`Store`].
    /// - If the [`Memory`] cannot be resolved to its entity.
    pub fn resolve_memory_mut(&mut self, memory: &Memory) -> &mut MemoryEntity {
        let idx = self.unwrap_stored(memory.as_inner());
        Self::resolve_mut(idx, &mut self.memories)
    }

    /// Returns a pair of:
    ///
    /// - An exclusive reference to the [`MemoryEntity`] associated to the given [`Memory`].
    /// - A shared reference to the [`DataSegmentEntity`] associated to the given [`DataSegment`].
    ///
    /// # Note
    ///
    /// This method exists to properly handle use cases where
    /// otherwise the Rust borrow-checker would not accept.
    ///
    /// # Panics
    ///
    /// - If the [`Memory`] does not originate from this [`Store`].
    /// - If the [`Memory`] cannot be resolved to its entity.
    /// - If the [`DataSegment`] does not originate from this [`Store`].
    /// - If the [`DataSegment`] cannot be resolved to its entity.
    pub(super) fn resolve_memory_mut_and_data_segment(
        &mut self,
        memory: &Memory,
        segment: &DataSegment,
    ) -> (&mut MemoryEntity, &DataSegmentEntity) {
        let mem_idx = self.unwrap_stored(memory.as_inner());
        let data_idx = segment.as_inner();
        let data = self.resolve(data_idx, &self.datas);
        let mem = Self::resolve_mut(mem_idx, &mut self.memories);
        (mem, data)
    }

    /// Returns a shared reference to the [`DataSegmentEntity`] associated to the given [`DataSegment`].
    ///
    /// # Panics
    ///
    /// - If the [`DataSegment`] does not originate from this [`Store`].
    /// - If the [`DataSegment`] cannot be resolved to its entity.
    #[allow(unused)] // Note: We allow this unused API to exist to uphold code symmetry.
    pub fn resolve_data_segment(&self, segment: &DataSegment) -> &DataSegmentEntity {
        self.resolve(segment.as_inner(), &self.datas)
    }

    /// Returns an exclusive reference to the [`DataSegmentEntity`] associated to the given [`DataSegment`].
    ///
    /// # Panics
    ///
    /// - If the [`DataSegment`] does not originate from this [`Store`].
    /// - If the [`DataSegment`] cannot be resolved to its entity.
    pub fn resolve_data_segment_mut(&mut self, segment: &DataSegment) -> &mut DataSegmentEntity {
        let idx = self.unwrap_stored(segment.as_inner());
        Self::resolve_mut(idx, &mut self.datas)
    }

    /// Returns a shared reference to the [`InstanceEntity`] associated to the given [`Instance`].
    ///
    /// # Panics
    ///
    /// - If the [`Instance`] does not originate from this [`Store`].
    /// - If the [`Instance`] cannot be resolved to its entity.
    pub fn resolve_instance(&self, instance: &Instance) -> &InstanceEntity {
        self.resolve(instance.as_inner(), &self.instances)
    }

    /// Returns a shared reference to the [`ExternObjectEntity`] associated to the given [`ExternObject`].
    ///
    /// # Panics
    ///
    /// - If the [`ExternObject`] does not originate from this [`Store`].
    /// - If the [`ExternObject`] cannot be resolved to its entity.
    pub fn resolve_external_object(&self, object: &ExternObject) -> &ExternObjectEntity {
        self.resolve(object.as_inner(), &self.extern_objects)
    }

    /// Allocates a new Wasm or host [`FuncEntity`] and returns a [`Func`] reference to it.
    pub fn alloc_func(&mut self, func: FuncEntity) -> Func {
        let idx = self.funcs.alloc(func);
        Func::from_inner(self.wrap_stored(idx))
    }

    /// Returns a shared reference to the associated entity of the Wasm or host function.
    ///
    /// # Panics
    ///
    /// - If the [`Func`] does not originate from this [`Store`].
    /// - If the [`Func`] cannot be resolved to its entity.
    pub fn resolve_func(&self, func: &Func) -> &FuncEntity {
        let entity_index = self.unwrap_stored(func.as_inner());
        self.funcs.get(entity_index).unwrap_or_else(|| {
            panic!("failed to resolve stored Wasm or host function: {entity_index:?}")
        })
    }
}

impl<T> Store<T> {
    /// Creates a new store.
    pub fn new(engine: &Engine, data: T) -> Self {
        Self {
            inner: StoreInner::new(engine),
            trampolines: Arena::new(),
            data,
            limiter: None,
            execution_supplement: None,
        }
    }

    /// Returns the [`Engine`] that this store is associated with.
    pub fn engine(&self) -> &Engine {
        self.inner.engine()
    }

    /// Returns a shared reference to the user provided data owned by this [`Store`].
    pub fn data(&self) -> &T {
        &self.data
    }

    /// Returns an exclusive reference to the user provided data owned by this [`Store`].
    pub fn data_mut(&mut self) -> &mut T {
        &mut self.data
    }

    /// Consumes `self` and returns its user provided data.
    pub fn into_data(self) -> T {
        self.data
    }

    /// Enables bounded deterministic snapshots at the declared source-step interval.
    /// Existing observations are discarded so a configuration belongs to one execution.
    pub fn enable_execution_observer(&mut self, interval: u64, maximum_snapshots: usize) {
        self.enable_execution_observer_with_limits(interval, maximum_snapshots, 64 * 1024 * 1024, 64 * 1024 * 1024)
    }

    /// Enables observations with explicit aggregate allocation and work ceilings.
    pub fn enable_execution_observer_with_limits(
        &mut self,
        interval: u64,
        maximum_snapshots: usize,
        maximum_bytes: u64,
        maximum_work: u64,
    ) {
        self.inner.execution_observer = Some(ExecutionObserver {
            interval,
            maximum_snapshots,
            retained_snapshots: 0,
            step_index: 0,
            transitions: alloc::vec::Vec::new(),
            pending: None,
            error: None,
            supplement: ExecutionSupplement::default(),
            boundary_authorized: false,
            aggregate_bytes: 0,
            aggregate_work: 0,
            maximum_bytes,
            maximum_work,
            sampled_current: false,
        });
    }

    pub fn set_execution_supplement(
        &mut self,
        query: impl FnMut(&mut T, &mut ObservationCharge, u64, u64) -> Result<ExecutionSupplement, ExecutionObserverError> + Send + Sync + 'static,
    ) {
        self.execution_supplement = Some(ExecutionSupplementQuery(Box::new(query)));
    }

    pub(crate) fn refresh_execution_supplement(&mut self, mut charge: ObservationCharge) -> Result<(), ExecutionObserverError> {
        if self.inner.execution_observer.is_none() { return Ok(()) }
        let (remaining_bytes, remaining_work) = self.inner.execution_observer.as_ref()
            .map(|observer| (
                observer.maximum_bytes.saturating_sub(observer.aggregate_bytes),
                observer.maximum_work.saturating_sub(observer.aggregate_work),
            )).expect("observer exists");
        let supplement = match match self.execution_supplement.as_mut() {
            Some(query) => (query.0)(&mut self.data, &mut charge, remaining_bytes, remaining_work),
            None => Ok(ExecutionSupplement::default()),
        } {
            Ok(supplement) => supplement,
            Err(_) => {
                self.inner.fail_execution_observer(ExecutionObserverError::SupplementRejected);
                return Err(ExecutionObserverError::SupplementRejected)
            }
        };
        self.inner.execution_observer.as_mut().expect("observer exists").supplement = supplement;
        self.inner.authorize_execution_boundary(charge)?;
        Ok(())
    }

    /// Returns the fail-closed observer error produced by the last execution.
    pub fn execution_observer_error(&self) -> Option<ExecutionObserverError> {
        self.inner
            .execution_observer
            .as_ref()
            .and_then(|observer| observer.error)
    }

    /// Returns the number of snapshots retained by the active observer.
    pub fn execution_observer_retained_snapshots(&self) -> Option<usize> {
        self.inner
            .execution_observer
            .as_ref()
            .map(|observer| observer.retained_snapshots)
    }

    /// Returns retained and maximum snapshot counts for refusal classification.
    pub fn execution_observer_snapshot_counts(&self) -> Option<(usize, usize)> {
        self.inner
            .execution_observer
            .as_ref()
            .map(|observer| (observer.retained_snapshots, observer.maximum_snapshots))
    }

    pub fn take_execution_transitions(&mut self) -> Vec<ExecutionTransition> {
        self.inner.execution_observer.as_mut().map_or_else(Vec::new, |observer| core::mem::take(&mut observer.transitions))
    }

    /// Installs a function into the [`Store`] that will be called with the user
    /// data type `T` to retrieve a [`ResourceLimiter`] any time a limited,
    /// growable resource such as a linear memory or table is grown.
    pub fn limiter(
        &mut self,
        limiter: impl FnMut(&mut T) -> &mut (dyn ResourceLimiter) + Send + Sync + 'static,
    ) {
        self.limiter = Some(ResourceLimiterQuery(Box::new(limiter)))
    }

    pub(crate) fn check_new_instances_limit(
        &mut self,
        num_new_instances: usize,
    ) -> Result<(), InstantiationError> {
        let (inner, mut limiter) = self.store_inner_and_resource_limiter_ref();
        if let Some(limiter) = limiter.as_resource_limiter() {
            if inner.instances.len().saturating_add(num_new_instances) > limiter.instances() {
                return Err(InstantiationError::TooManyInstances);
            }
        }
        Ok(())
    }

    pub(crate) fn check_new_memories_limit(
        &mut self,
        num_new_memories: usize,
    ) -> Result<(), MemoryError> {
        let (inner, mut limiter) = self.store_inner_and_resource_limiter_ref();
        if let Some(limiter) = limiter.as_resource_limiter() {
            if inner.memories.len().saturating_add(num_new_memories) > limiter.memories() {
                return Err(MemoryError::TooManyMemories);
            }
        }
        Ok(())
    }

    pub(crate) fn check_new_tables_limit(
        &mut self,
        num_new_tables: usize,
    ) -> Result<(), TableError> {
        let (inner, mut limiter) = self.store_inner_and_resource_limiter_ref();
        if let Some(limiter) = limiter.as_resource_limiter() {
            if inner.tables.len().saturating_add(num_new_tables) > limiter.tables() {
                return Err(TableError::TooManyTables);
            }
        }
        Ok(())
    }

    pub(crate) fn store_inner_and_resource_limiter_ref(
        &mut self,
    ) -> (&mut StoreInner, ResourceLimiterRef) {
        let resource_limiter = ResourceLimiterRef(match &mut self.limiter {
            Some(q) => Some(q.0(&mut self.data)),
            None => None,
        });
        (&mut self.inner, resource_limiter)
    }

    /// Returns `true` if fuel metering has been enabled.
    fn is_fuel_metering_enabled(&self) -> bool {
        self.engine().config().get_consume_fuel()
    }

    /// Returns `Ok` if fuel metering has been enabled.
    ///
    /// Otherwise returns the respective [`FuelError`].
    fn check_fuel_metering_enabled(&self) -> Result<(), FuelError> {
        if !self.is_fuel_metering_enabled() {
            return Err(FuelError::fuel_metering_disabled());
        }
        Ok(())
    }

    /// Adds `delta` quantity of fuel to the remaining fuel.
    ///
    /// # Panics
    ///
    /// If this overflows the remaining fuel counter.
    ///
    /// # Errors
    ///
    /// If fuel metering is disabled.
    pub fn add_fuel(&mut self, delta: u64) -> Result<(), FuelError> {
        self.check_fuel_metering_enabled()?;
        self.inner.fuel.add_fuel(delta);
        Ok(())
    }

    /// Returns the amount of fuel consumed by executions of the [`Store`] so far.
    ///
    /// Returns `None` if fuel metering is disabled.
    pub fn fuel_consumed(&self) -> Option<u64> {
        self.check_fuel_metering_enabled().ok()?;
        Some(self.inner.fuel.fuel_consumed())
    }

    /// Synthetically consumes an amount of fuel for the [`Store`].
    ///
    /// Returns the remaining amount of fuel after this operation.
    ///
    /// # Panics
    ///
    /// If this overflows the consumed fuel counter.
    ///
    /// # Errors
    ///
    /// - If fuel metering is disabled.
    /// - If more fuel is consumed than available.
    pub fn consume_fuel(&mut self, delta: u64) -> Result<u64, FuelError> {
        self.check_fuel_metering_enabled()?;
        self.inner
            .fuel
            .consume_fuel(delta)
            .map_err(|_error| FuelError::out_of_fuel())
    }

    /// Allocates a new [`TrampolineEntity`] and returns a [`Trampoline`] reference to it.
    pub(super) fn alloc_trampoline(&mut self, func: TrampolineEntity<T>) -> Trampoline {
        let idx = self.trampolines.alloc(func);
        Trampoline::from_inner(self.inner.wrap_stored(idx))
    }

    /// Returns an exclusive reference to the [`MemoryEntity`] associated to the given [`Memory`]
    /// and an exclusive reference to the user provided host state.
    ///
    /// # Note
    ///
    /// This method exists to properly handle use cases where
    /// otherwise the Rust borrow-checker would not accept.
    ///
    /// # Panics
    ///
    /// - If the [`Memory`] does not originate from this [`Store`].
    /// - If the [`Memory`] cannot be resolved to its entity.
    pub(super) fn resolve_memory_and_state_mut(
        &mut self,
        memory: &Memory,
    ) -> (&mut MemoryEntity, &mut T) {
        (self.inner.resolve_memory_mut(memory), &mut self.data)
    }

    /// Returns a shared reference to the associated entity of the host function trampoline.
    ///
    /// # Panics
    ///
    /// - If the [`Trampoline`] does not originate from this [`Store`].
    /// - If the [`Trampoline`] cannot be resolved to its entity.
    pub(super) fn resolve_trampoline(&self, func: &Trampoline) -> &TrampolineEntity<T> {
        let entity_index = self.inner.unwrap_stored(func.as_inner());
        self.trampolines
            .get(entity_index)
            .unwrap_or_else(|| panic!("failed to resolve stored host function: {entity_index:?}"))
    }
}

/// A trait used to get shared access to a [`Store`] in `wasmi`.
pub trait AsContext {
    /// The user state associated with the [`Store`], aka the `T` in `Store<T>`.
    type UserState;

    /// Returns the store context that this type provides access to.
    fn as_context(&self) -> StoreContext<Self::UserState>;
}

/// A trait used to get exclusive access to a [`Store`] in `wasmi`.
pub trait AsContextMut: AsContext {
    /// Returns the store context that this type provides access to.
    fn as_context_mut(&mut self) -> StoreContextMut<Self::UserState>;
}

/// A temporary handle to a [`&Store<T>`][`Store`].
///
/// This type is suitable for [`AsContext`] trait bounds on methods if desired.
/// For more information, see [`Store`].
#[derive(Debug, Copy, Clone)]
#[repr(transparent)]
pub struct StoreContext<'a, T> {
    pub(crate) store: &'a Store<T>,
}

impl<'a, T> StoreContext<'a, T> {
    /// Returns the underlying [`Engine`] this store is connected to.
    pub fn engine(&self) -> &Engine {
        self.store.engine()
    }

    /// Access the underlying data owned by this store.
    ///
    /// Same as [`Store::data`].
    pub fn data(&self) -> &T {
        self.store.data()
    }
}

impl<'a, T: AsContext> From<&'a T> for StoreContext<'a, T::UserState> {
    #[inline]
    fn from(ctx: &'a T) -> Self {
        ctx.as_context()
    }
}

impl<'a, T: AsContext> From<&'a mut T> for StoreContext<'a, T::UserState> {
    #[inline]
    fn from(ctx: &'a mut T) -> Self {
        T::as_context(ctx)
    }
}

impl<'a, T: AsContextMut> From<&'a mut T> for StoreContextMut<'a, T::UserState> {
    #[inline]
    fn from(ctx: &'a mut T) -> Self {
        ctx.as_context_mut()
    }
}

/// A temporary handle to a [`&mut Store<T>`][`Store`].
///
/// This type is suitable for [`AsContextMut`] or [`AsContext`] trait bounds on methods if desired.
/// For more information, see [`Store`].
#[derive(Debug)]
#[repr(transparent)]
pub struct StoreContextMut<'a, T> {
    pub(crate) store: &'a mut Store<T>,
}

impl<'a, T> StoreContextMut<'a, T> {
    /// Returns the underlying [`Engine`] this store is connected to.
    pub fn engine(&self) -> &Engine {
        self.store.engine()
    }

    /// Access the underlying data owned by this store.
    ///
    /// Same as [`Store::data`].
    pub fn data(&self) -> &T {
        self.store.data()
    }

    /// Access the underlying data owned by this store.
    ///
    /// Same as [`Store::data_mut`].
    pub fn data_mut(&mut self) -> &mut T {
        self.store.data_mut()
    }
}

impl<T> AsContext for &'_ T
where
    T: AsContext,
{
    type UserState = T::UserState;

    #[inline]
    fn as_context(&self) -> StoreContext<'_, T::UserState> {
        T::as_context(*self)
    }
}

impl<T> AsContext for &'_ mut T
where
    T: AsContext,
{
    type UserState = T::UserState;

    #[inline]
    fn as_context(&self) -> StoreContext<'_, T::UserState> {
        T::as_context(*self)
    }
}

impl<T> AsContextMut for &'_ mut T
where
    T: AsContextMut,
{
    #[inline]
    fn as_context_mut(&mut self) -> StoreContextMut<'_, T::UserState> {
        T::as_context_mut(*self)
    }
}

impl<T> AsContext for StoreContext<'_, T> {
    type UserState = T;

    #[inline]
    fn as_context(&self) -> StoreContext<'_, Self::UserState> {
        StoreContext { store: self.store }
    }
}

impl<T> AsContext for StoreContextMut<'_, T> {
    type UserState = T;

    #[inline]
    fn as_context(&self) -> StoreContext<'_, Self::UserState> {
        StoreContext { store: self.store }
    }
}

impl<T> AsContextMut for StoreContextMut<'_, T> {
    #[inline]
    fn as_context_mut(&mut self) -> StoreContextMut<'_, Self::UserState> {
        StoreContextMut {
            store: &mut *self.store,
        }
    }
}

impl<T> AsContext for Store<T> {
    type UserState = T;

    #[inline]
    fn as_context(&self) -> StoreContext<'_, Self::UserState> {
        StoreContext { store: self }
    }
}

impl<T> AsContextMut for Store<T> {
    #[inline]
    fn as_context_mut(&mut self) -> StoreContextMut<'_, Self::UserState> {
        StoreContextMut { store: self }
    }
}
