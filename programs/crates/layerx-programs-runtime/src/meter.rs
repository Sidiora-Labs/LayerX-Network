//! Deterministic resource accounting for guest execution.

use core::fmt::{self, Display};

use wasmi::errors::{MemoryError, TableError};
use wasmi::ResourceLimiter;

/// Default instruction fuel admitted for one execution.
pub const DEFAULT_CPU_FUEL: u64 = 1_000_000;
/// Default peak linear-memory budget in bytes.
pub const DEFAULT_MEMORY_BYTES: u64 = 16 * 1_024 * 1_024;
/// Default storage-read budget in bytes.
pub const DEFAULT_STORAGE_READ_BYTES: u64 = 1_048_576;
/// Default storage-write budget in bytes.
pub const DEFAULT_STORAGE_WRITE_BYTES: u64 = 1_048_576;
/// Default result-value count admitted at the guest boundary.
pub const DEFAULT_OUTPUT_VALUES: u32 = 64;
/// Default successful-response byte budget.
pub const DEFAULT_OUTPUT_BYTES: u64 = 1_048_576;
/// Default table-element limit per execution.
pub const DEFAULT_TABLE_ELEMENTS: u32 = 4_096;
/// Default fee-unit price for one namespace byte held across one batch.
pub const DEFAULT_OCCUPANCY_BYTE_BATCH_PRICE: u64 = 1;

/// One independently enforced deterministic resource class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    /// Interpreter instruction fuel.
    Cpu,
    /// Peak guest linear memory.
    Memory,
    /// Bytes read through the storage ABI.
    StorageRead,
    /// Bytes written through the storage ABI.
    StorageWrite,
    /// One namespace byte held across one protocol batch.
    StorageOccupancy,
    /// Integer values returned across the guest boundary.
    Output,
    /// Successful response bytes copied across a call boundary.
    OutputBytes,
}

impl Display for ResourceKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cpu => write!(formatter, "cpu fuel"),
            Self::Memory => write!(formatter, "memory bytes"),
            Self::StorageRead => write!(formatter, "storage read bytes"),
            Self::StorageWrite => write!(formatter, "storage write bytes"),
            Self::StorageOccupancy => write!(formatter, "storage occupancy byte-batches"),
            Self::Output => write!(formatter, "output values"),
            Self::OutputBytes => write!(formatter, "output bytes"),
        }
    }
}

/// Resource classes available only to caller-declared activity budgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetResourceKind {
    Cpu,
    Memory,
    StorageRead,
    StorageWrite,
    Output,
    OutputBytes,
    Table,
}

/// Receipt-carriable resource refusal for an admitted activity budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetMeterRefusal {
    BudgetExceeded {
        resource: BudgetResourceKind,
        limit: u64,
        attempted: u64,
    },
    CounterOverflow {
        resource: BudgetResourceKind,
    },
}

impl TryFrom<MeterRefusal> for BudgetMeterRefusal {
    type Error = MeterRefusal;

    fn try_from(refusal: MeterRefusal) -> Result<Self, Self::Error> {
        budget_refusal(refusal).ok_or(refusal)
    }
}

/// Exact resource budget applied to one fresh execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceBudget {
    cpu_fuel: u64,
    memory_bytes: u64,
    storage_read_bytes: u64,
    storage_write_bytes: u64,
    output_values: u32,
    output_bytes: u64,
    table_elements: u32,
}

impl ResourceBudget {
    /// Constructs an explicit integer-only execution budget.
    #[must_use]
    pub const fn new(
        cpu_fuel: u64,
        memory_bytes: u64,
        storage_read_bytes: u64,
        storage_write_bytes: u64,
        output_values: u32,
        table_elements: u32,
    ) -> Self {
        Self {
            cpu_fuel,
            memory_bytes,
            storage_read_bytes,
            storage_write_bytes,
            output_values,
            output_bytes: DEFAULT_OUTPUT_BYTES,
            table_elements,
        }
    }

    /// Constructs all seven resource limits without defaulting response bytes.
    #[must_use]
    pub const fn new_complete(
        cpu_fuel: u64,
        memory_bytes: u64,
        storage_read_bytes: u64,
        storage_write_bytes: u64,
        output_values: u32,
        output_bytes: u64,
        table_elements: u32,
    ) -> Self {
        Self {
            cpu_fuel,
            memory_bytes,
            storage_read_bytes,
            storage_write_bytes,
            output_values,
            output_bytes,
            table_elements,
        }
    }

    /// Returns the declared production budget.
    #[must_use]
    pub const fn declared() -> Self {
        Self::new(
            DEFAULT_CPU_FUEL,
            DEFAULT_MEMORY_BYTES,
            DEFAULT_STORAGE_READ_BYTES,
            DEFAULT_STORAGE_WRITE_BYTES,
            DEFAULT_OUTPUT_VALUES,
            DEFAULT_TABLE_ELEMENTS,
        )
    }

    /// Returns the instruction-fuel limit.
    #[must_use]
    pub const fn cpu_fuel(self) -> u64 {
        self.cpu_fuel
    }

    /// Returns the peak linear-memory limit.
    #[must_use]
    pub const fn memory_bytes(self) -> u64 {
        self.memory_bytes
    }

    /// Returns the cumulative storage-read limit.
    #[must_use]
    pub const fn storage_read_bytes(self) -> u64 {
        self.storage_read_bytes
    }

    /// Returns the cumulative storage-write limit.
    #[must_use]
    pub const fn storage_write_bytes(self) -> u64 {
        self.storage_write_bytes
    }

    /// Returns the maximum result-value count.
    #[must_use]
    pub const fn output_values(self) -> u32 {
        self.output_values
    }

    #[must_use]
    pub const fn with_output_bytes(mut self, output_bytes: u64) -> Self {
        self.output_bytes = output_bytes;
        self
    }

    #[must_use]
    pub const fn output_bytes(self) -> u64 {
        self.output_bytes
    }

    /// Returns the peak table-element limit.
    #[must_use]
    pub const fn table_elements(self) -> u32 {
        self.table_elements
    }
}

impl Default for ResourceBudget {
    fn default() -> Self {
        Self::declared()
    }
}

/// Integer fee-unit prices for each metered resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeeSchedule {
    version: u32,
    cpu: u64,
    memory_byte: u64,
    storage_read_byte: u64,
    storage_write_byte: u64,
    output_value: u64,
    output_byte: u64,
    occupancy_byte_batch: u64,
}

impl FeeSchedule {
    /// Constructs an explicit integer fee schedule.
    #[must_use]
    pub const fn new(
        cpu: u64,
        memory_byte: u64,
        storage_read_byte: u64,
        storage_write_byte: u64,
        output_value: u64,
    ) -> Self {
        Self {
            version: 1,
            cpu,
            memory_byte,
            storage_read_byte,
            storage_write_byte,
            output_value,
            output_byte: 1,
            occupancy_byte_batch: DEFAULT_OCCUPANCY_BYTE_BATCH_PRICE,
        }
    }

    /// Constructs the complete governed schedule recorded by the protocol.
    #[must_use]
    pub const fn new_complete(
        version: u32,
        cpu: u64,
        memory_byte: u64,
        storage_read_byte: u64,
        storage_write_byte: u64,
        output_value: u64,
        output_byte: u64,
        occupancy_byte_batch: u64,
    ) -> Self {
        Self {
            version,
            cpu,
            memory_byte,
            storage_read_byte,
            storage_write_byte,
            output_value,
            output_byte,
            occupancy_byte_batch,
        }
    }

    #[must_use]
    pub const fn version(self) -> u32 {
        self.version
    }

    #[must_use]
    pub const fn with_output_byte_price(mut self, output_byte: u64) -> Self {
        self.output_byte = output_byte;
        self
    }

    #[must_use]
    pub const fn with_occupancy_byte_batch_price(mut self, price: u64) -> Self {
        self.occupancy_byte_batch = price;
        self
    }

    /// Returns the runtime's version-one fee-unit schedule.
    #[must_use]
    pub const fn declared() -> Self {
        Self::new(1, 1, 2, 4, 1)
    }

    #[must_use]
    pub const fn cpu_price(self) -> u64 {
        self.cpu
    }

    #[must_use]
    pub const fn memory_byte_price(self) -> u64 {
        self.memory_byte
    }

    #[must_use]
    pub const fn storage_read_byte_price(self) -> u64 {
        self.storage_read_byte
    }

    #[must_use]
    pub const fn storage_write_byte_price(self) -> u64 {
        self.storage_write_byte
    }

    #[must_use]
    pub const fn output_value_price(self) -> u64 {
        self.output_value
    }

    #[must_use]
    pub const fn output_byte_price(self) -> u64 {
        self.output_byte
    }

    #[must_use]
    pub const fn occupancy_byte_batch_price(self) -> u64 {
        self.occupancy_byte_batch
    }
}

impl Default for FeeSchedule {
    fn default() -> Self {
        Self::declared()
    }
}

/// Exact deterministic usage and fee units produced by one execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeteredUsage {
    /// Interpreter fuel consumed.
    pub cpu_fuel: u64,
    /// Peak guest memory admitted in bytes.
    pub memory_bytes: u64,
    /// Storage bytes read.
    pub storage_read_bytes: u64,
    /// Storage bytes written.
    pub storage_write_bytes: u64,
    /// Integer result values returned.
    pub output_values: u32,
    /// Successful response bytes copied across execution boundaries.
    pub output_bytes: u64,
    /// Persistent storage occupied across canonical protocol batch intervals.
    pub occupancy_byte_batches: u128,
    /// Exact occupancy fee, kept distinct from one-off execution fees.
    pub occupancy_fee_units: u128,
    /// Exact units handed to the existing fee mechanism.
    pub fee_units: u128,
}

/// Typed resource refusal with exact limit and attempted use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeterRefusal {
    /// One resource budget was exceeded.
    BudgetExceeded {
        /// Resource whose budget was exceeded.
        resource: ResourceKind,
        /// Configured resource limit.
        limit: u64,
        /// Attempted cumulative or peak use.
        attempted: u64,
    },
    /// Cumulative accounting could not be represented exactly.
    CounterOverflow {
        /// Resource whose cumulative counter overflowed.
        resource: ResourceKind,
    },
    /// Fee-unit multiplication or accumulation exceeded `u128`.
    FeeOverflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MeterMode {
    Legacy,
    Activity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MeterExhaustion {
    Legacy(MeterRefusal),
    Budget(BudgetMeterRefusal),
}

impl Display for MeterRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BudgetExceeded {
                resource,
                limit,
                attempted,
            } => write!(
                formatter,
                "{resource} budget {limit} exceeded by attempted use {attempted}"
            ),
            Self::CounterOverflow { resource } => {
                write!(formatter, "{resource} cumulative accounting overflowed")
            }
            Self::FeeOverflow => write!(formatter, "metered fee units overflowed"),
        }
    }
}

impl std::error::Error for MeterRefusal {}

const fn budget_resource(resource: ResourceKind) -> Option<BudgetResourceKind> {
    match resource {
        ResourceKind::Cpu => Some(BudgetResourceKind::Cpu),
        ResourceKind::Memory => Some(BudgetResourceKind::Memory),
        ResourceKind::StorageRead => Some(BudgetResourceKind::StorageRead),
        ResourceKind::StorageWrite => Some(BudgetResourceKind::StorageWrite),
        ResourceKind::Output => Some(BudgetResourceKind::Output),
        ResourceKind::OutputBytes => Some(BudgetResourceKind::OutputBytes),
        ResourceKind::StorageOccupancy => None,
    }
}

const fn budget_refusal(refusal: MeterRefusal) -> Option<BudgetMeterRefusal> {
    match refusal {
        MeterRefusal::BudgetExceeded {
            resource,
            limit,
            attempted,
        } => match budget_resource(resource) {
            Some(resource) => Some(BudgetMeterRefusal::BudgetExceeded {
                resource,
                limit,
                attempted,
            }),
            None => None,
        },
        MeterRefusal::CounterOverflow { resource } => match budget_resource(resource) {
            Some(resource) => Some(BudgetMeterRefusal::CounterOverflow { resource }),
            None => None,
        },
        MeterRefusal::FeeOverflow => None,
    }
}

const fn project_budget_resource(resource: BudgetResourceKind) -> ResourceKind {
    match resource {
        BudgetResourceKind::Cpu => ResourceKind::Cpu,
        BudgetResourceKind::Memory | BudgetResourceKind::Table => ResourceKind::Memory,
        BudgetResourceKind::StorageRead => ResourceKind::StorageRead,
        BudgetResourceKind::StorageWrite => ResourceKind::StorageWrite,
        BudgetResourceKind::Output => ResourceKind::Output,
        BudgetResourceKind::OutputBytes => ResourceKind::OutputBytes,
    }
}

const fn project_budget_refusal(refusal: BudgetMeterRefusal) -> MeterRefusal {
    match refusal {
        BudgetMeterRefusal::BudgetExceeded {
            resource,
            limit,
            attempted,
        } => MeterRefusal::BudgetExceeded {
            resource: project_budget_resource(resource),
            limit,
            attempted,
        },
        BudgetMeterRefusal::CounterOverflow { resource } => MeterRefusal::CounterOverflow {
            resource: project_budget_resource(resource),
        },
    }
}

/// Per-execution deterministic meter and guest resource limiter.
#[derive(Debug, Clone)]
pub struct Meter {
    budget: ResourceBudget,
    prices: FeeSchedule,
    cpu_fuel: u64,
    cpu_carried: u64,
    memory_bytes: u64,
    active_memory_bytes: u64,
    active_table_elements: u64,
    storage_read_bytes: u64,
    storage_write_bytes: u64,
    output_values: u32,
    output_bytes: u64,
    mode: MeterMode,
    exhausted: Option<MeterExhaustion>,
}

impl Meter {
    /// Creates a fresh meter for one execution.
    #[must_use]
    pub const fn new(budget: ResourceBudget, prices: FeeSchedule) -> Self {
        Self {
            budget,
            prices,
            cpu_fuel: 0,
            cpu_carried: 0,
            memory_bytes: 0,
            active_memory_bytes: 0,
            active_table_elements: 0,
            storage_read_bytes: 0,
            storage_write_bytes: 0,
            output_values: 0,
            output_bytes: 0,
            mode: MeterMode::Legacy,
            exhausted: None,
        }
    }

    /// Creates a meter for an admitted activity with distinct table taxonomy.
    #[must_use]
    pub(crate) const fn new_activity(budget: ResourceBudget, prices: FeeSchedule) -> Self {
        let mut meter = Self::new(budget, prices);
        meter.mode = MeterMode::Activity;
        meter
    }

    pub(crate) const fn is_activity(&self) -> bool {
        matches!(self.mode, MeterMode::Activity)
    }

    /// Creates a fresh meter under the declared production budget and prices.
    #[must_use]
    pub const fn declared() -> Self {
        Self::new(ResourceBudget::declared(), FeeSchedule::declared())
    }

    /// Returns the declared instruction-fuel budget of the whole call graph.
    #[must_use]
    pub const fn cpu_budget(&self) -> u64 {
        self.budget.cpu_fuel
    }

    /// Returns the fuel already consumed by frames outside the store this
    /// meter is about to drive.
    #[must_use]
    pub const fn cpu_carried(&self) -> u64 {
        self.cpu_carried
    }

    /// Returns the fuel to install into the interpreter store of the next
    /// frame, so a whole call graph never exceeds one declared budget.
    #[must_use]
    pub const fn cpu_remaining(&self) -> u64 {
        self.budget.cpu_fuel.saturating_sub(self.cpu_carried)
    }

    /// Returns the fuel attributed to this meter by the last recorded frame.
    #[must_use]
    pub const fn cpu_total(&self) -> u64 {
        self.cpu_fuel
    }

    pub(crate) fn carry_cpu(&mut self, consumed: u64) -> Result<(), MeterRefusal> {
        let attempted = self.counter_add(ResourceKind::Cpu, self.cpu_carried, consumed)?;
        self.admit(ResourceKind::Cpu, self.budget.cpu_fuel, attempted)?;
        self.cpu_carried = attempted;
        Ok(())
    }

    pub(crate) fn restore_cpu_carry(&mut self, carried: u64) {
        self.cpu_carried = carried;
        self.cpu_fuel = 0;
    }

    pub(crate) const fn active_frame_resources(&self) -> (u64, u64) {
        (self.active_memory_bytes, self.active_table_elements)
    }

    pub(crate) fn restore_active_frame_resources(
        &mut self,
        memory_bytes: u64,
        table_elements: u64,
    ) {
        self.active_memory_bytes = memory_bytes;
        self.active_table_elements = table_elements;
    }

    /// Charges bytes read through the future versioned storage ABI.
    ///
    /// # Errors
    ///
    /// Returns a typed refusal before the read when its cumulative budget is exceeded.
    pub fn charge_storage_read(&mut self, bytes: u64) -> Result<(), MeterRefusal> {
        let attempted =
            self.counter_add(ResourceKind::StorageRead, self.storage_read_bytes, bytes)?;
        self.admit(
            ResourceKind::StorageRead,
            self.budget.storage_read_bytes,
            attempted,
        )?;
        self.storage_read_bytes = attempted;
        Ok(())
    }

    /// Charges bytes written through the future versioned storage ABI.
    ///
    /// # Errors
    ///
    /// Returns a typed refusal before the write when its cumulative budget is exceeded.
    pub fn charge_storage_write(&mut self, bytes: u64) -> Result<(), MeterRefusal> {
        let attempted =
            self.counter_add(ResourceKind::StorageWrite, self.storage_write_bytes, bytes)?;
        self.admit(
            ResourceKind::StorageWrite,
            self.budget.storage_write_bytes,
            attempted,
        )?;
        self.storage_write_bytes = attempted;
        Ok(())
    }

    /// Charges CPU fuel for computational operations like wide-integer arithmetic.
    ///
    /// # Errors
    ///
    /// Returns a typed refusal when the cumulative CPU budget is exceeded.
    pub fn charge_cpu(&mut self, fuel: u64) -> Result<(), MeterRefusal> {
        let attempted = self.counter_add(ResourceKind::Cpu, self.cpu_fuel, fuel)?;
        self.admit(ResourceKind::Cpu, self.budget.cpu_fuel, attempted)?;
        self.cpu_fuel = attempted;
        Ok(())
    }

    pub(crate) fn record_cpu(&mut self, consumed: u64) {
        match self.cpu_carried.checked_add(consumed) {
            Some(total) => self.cpu_fuel = total,
            None => {
                self.record_exhaustion(MeterRefusal::CounterOverflow {
                    resource: ResourceKind::Cpu,
                });
            }
        }
    }

    pub(crate) fn mark_cpu_exhausted(&mut self) {
        let attempted = if self.is_activity() {
            self.budget.cpu_fuel.checked_add(1)
        } else {
            self.cpu_fuel.checked_add(1)
        };
        self.record_exhaustion(match attempted {
            Some(attempted) => MeterRefusal::BudgetExceeded {
                resource: ResourceKind::Cpu,
                limit: self.budget.cpu_fuel,
                attempted,
            },
            None => MeterRefusal::CounterOverflow {
                resource: ResourceKind::Cpu,
            },
        });
    }

    pub(crate) fn charge_output(&mut self, values: usize) -> Result<(), MeterRefusal> {
        let requested = u64::try_from(values).unwrap_or(u64::MAX);
        let attempted = self.counter_add(
            ResourceKind::Output,
            u64::from(self.output_values),
            requested,
        )?;
        self.admit(
            ResourceKind::Output,
            u64::from(self.budget.output_values),
            attempted,
        )?;
        self.output_values =
            u32::try_from(attempted).map_err(|_| MeterRefusal::BudgetExceeded {
                resource: ResourceKind::Output,
                limit: u64::from(self.budget.output_values),
                attempted,
            })?;
        Ok(())
    }

    pub(crate) fn charge_output_bytes(&mut self, bytes: usize) -> Result<(), MeterRefusal> {
        let requested = u64::try_from(bytes).unwrap_or(u64::MAX);
        let attempted =
            self.counter_add(ResourceKind::OutputBytes, self.output_bytes, requested)?;
        self.admit(
            ResourceKind::OutputBytes,
            self.budget.output_bytes,
            attempted,
        )?;
        self.output_bytes = attempted;
        Ok(())
    }

    pub(crate) const fn exhaustion(&self) -> Option<MeterRefusal> {
        match self.exhausted {
            Some(MeterExhaustion::Legacy(refusal)) => Some(refusal),
            Some(MeterExhaustion::Budget(refusal)) => Some(project_budget_refusal(refusal)),
            None => None,
        }
    }

    pub(crate) const fn budget_exhaustion(&self) -> Option<BudgetMeterRefusal> {
        match self.exhausted {
            Some(MeterExhaustion::Budget(refusal)) => Some(refusal),
            Some(MeterExhaustion::Legacy(refusal)) => budget_refusal(refusal),
            None => None,
        }
    }

    /// Finalises exact usage and fee units after guest execution.
    ///
    /// # Errors
    ///
    /// Returns a prior resource refusal or fee overflow.
    pub fn finish(&self) -> Result<MeteredUsage, MeterRefusal> {
        if let Some(refusal) = self.exhaustion() {
            return Err(refusal);
        }
        self.finish_bounded_usage(self.cpu_fuel)
    }

    /// Finalises a published candidate failure after that same guest frame
    /// consumed its complete CPU allowance.
    pub(crate) fn finish_published_failure(&self) -> Result<MeteredUsage, MeterRefusal> {
        match self.exhaustion() {
            None => self.finish_bounded_usage(self.cpu_fuel),
            Some(MeterRefusal::BudgetExceeded {
                resource: ResourceKind::Cpu,
                limit,
                attempted,
            }) if limit == self.budget.cpu_fuel
                && self.cpu_fuel.checked_add(1) == Some(attempted)
                && (attempted == limit || limit.checked_add(1) == Some(attempted)) =>
            {
                self.finish_bounded_usage(limit)
            }
            Some(refusal) => Err(refusal),
        }
    }

    /// Prices only counters admitted before a resource increment was refused.
    pub(crate) fn finish_resource_failure(&self) -> Result<MeteredUsage, MeterRefusal> {
        self.finish_bounded_usage(self.cpu_fuel)
    }

    fn finish_bounded_usage(&self, cpu_fuel: u64) -> Result<MeteredUsage, MeterRefusal> {
        let priced = [
            (u128::from(cpu_fuel), self.prices.cpu),
            (u128::from(self.memory_bytes), self.prices.memory_byte),
            (
                u128::from(self.storage_read_bytes),
                self.prices.storage_read_byte,
            ),
            (
                u128::from(self.storage_write_bytes),
                self.prices.storage_write_byte,
            ),
            (u128::from(self.output_values), self.prices.output_value),
            (u128::from(self.output_bytes), self.prices.output_byte),
        ];
        let mut fee_units = 0u128;
        for (use_units, price) in priced {
            fee_units = fee_units
                .checked_add(
                    use_units
                        .checked_mul(u128::from(price))
                        .ok_or(MeterRefusal::FeeOverflow)?,
                )
                .ok_or(MeterRefusal::FeeOverflow)?;
        }
        Ok(MeteredUsage {
            cpu_fuel,
            memory_bytes: self.memory_bytes,
            storage_read_bytes: self.storage_read_bytes,
            storage_write_bytes: self.storage_write_bytes,
            output_values: self.output_values,
            output_bytes: self.output_bytes,
            occupancy_byte_batches: 0,
            occupancy_fee_units: 0,
            fee_units,
        })
    }

    fn admit(
        &mut self,
        resource: ResourceKind,
        limit: u64,
        attempted: u64,
    ) -> Result<(), MeterRefusal> {
        if attempted <= limit {
            return Ok(());
        }
        let refusal = MeterRefusal::BudgetExceeded {
            resource,
            limit,
            attempted,
        };
        self.record_exhaustion(refusal);
        Err(refusal)
    }

    fn counter_add(
        &mut self,
        resource: ResourceKind,
        current: u64,
        increment: u64,
    ) -> Result<u64, MeterRefusal> {
        current.checked_add(increment).ok_or_else(|| {
            let refusal = MeterRefusal::CounterOverflow { resource };
            self.record_exhaustion(refusal);
            refusal
        })
    }

    fn record_exhaustion(&mut self, refusal: MeterRefusal) {
        if self.is_activity() && self.exhausted.is_some() {
            return;
        }
        self.exhausted = Some(if self.is_activity() {
            match budget_refusal(refusal) {
                Some(refusal) => MeterExhaustion::Budget(refusal),
                None => MeterExhaustion::Legacy(refusal),
            }
        } else {
            MeterExhaustion::Legacy(refusal)
        });
    }
}

impl Default for Meter {
    fn default() -> Self {
        Self::declared()
    }
}

impl ResourceLimiter for Meter {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> Result<bool, MemoryError> {
        if maximum.is_some_and(|maximum| desired > maximum) {
            return Ok(false);
        }
        let desired = u64::try_from(desired).unwrap_or(u64::MAX);
        let attempted = if self.is_activity() {
            let current = u64::try_from(current).unwrap_or(u64::MAX);
            let increment = desired
                .checked_sub(current)
                .ok_or(MemoryError::OutOfBoundsGrowth)?;
            self.counter_add(ResourceKind::Memory, self.active_memory_bytes, increment)
                .map_err(|_| MemoryError::OutOfBoundsGrowth)?
        } else {
            desired
        };
        if self
            .admit(ResourceKind::Memory, self.budget.memory_bytes, attempted)
            .is_err()
        {
            return Err(MemoryError::OutOfBoundsGrowth);
        }
        self.memory_bytes = self.memory_bytes.max(attempted);
        if self.is_activity() {
            self.active_memory_bytes = attempted;
        }
        Ok(true)
    }

    fn table_growing(
        &mut self,
        current: u32,
        desired: u32,
        maximum: Option<u32>,
    ) -> Result<bool, TableError> {
        if self.is_activity() {
            if maximum.is_some_and(|maximum| desired > maximum) {
                return Ok(false);
            }
            let increment = u64::from(desired.checked_sub(current).ok_or(
                TableError::GrowOutOfBounds {
                    maximum: self.budget.table_elements,
                    current,
                    delta: 0,
                },
            )?);
            let Some(attempted) = self.active_table_elements.checked_add(increment) else {
                if self.exhausted.is_none() {
                    self.exhausted = Some(MeterExhaustion::Budget(
                        BudgetMeterRefusal::CounterOverflow {
                            resource: BudgetResourceKind::Table,
                        },
                    ));
                }
                return Err(TableError::GrowOutOfBounds {
                    maximum: self.budget.table_elements,
                    current,
                    delta: desired.saturating_sub(current),
                });
            };
            if attempted > u64::from(self.budget.table_elements) {
                if self.exhausted.is_none() {
                    self.exhausted = Some(MeterExhaustion::Budget(
                        BudgetMeterRefusal::BudgetExceeded {
                            resource: BudgetResourceKind::Table,
                            limit: u64::from(self.budget.table_elements),
                            attempted,
                        },
                    ));
                }
                return Err(TableError::GrowOutOfBounds {
                    maximum: self.budget.table_elements,
                    current,
                    delta: desired.saturating_sub(current),
                });
            }
            self.active_table_elements = attempted;
            return Ok(true);
        }
        let limit = maximum
            .unwrap_or(self.budget.table_elements)
            .min(self.budget.table_elements);
        if desired <= limit {
            return Ok(true);
        }
        self.record_exhaustion(MeterRefusal::BudgetExceeded {
            resource: ResourceKind::Memory,
            limit: u64::from(limit),
            attempted: u64::from(desired),
        });
        Err(TableError::GrowOutOfBounds {
            maximum: limit,
            current,
            delta: desired.saturating_sub(current),
        })
    }

    fn instances(&self) -> usize {
        1
    }

    fn tables(&self) -> usize {
        1
    }

    fn memories(&self) -> usize {
        1
    }
}

#[cfg(test)]
mod response_tests {
    use super::{FeeSchedule, Meter, MeterRefusal, ResourceBudget, ResourceKind};

    #[test]
    fn response_byte_counter_overflow_is_typed() {
        let mut meter = Meter::new(
            ResourceBudget::declared().with_output_bytes(u64::MAX),
            FeeSchedule::declared(),
        );
        meter.output_bytes = u64::MAX;
        assert_eq!(
            meter.charge_output_bytes(1),
            Err(MeterRefusal::CounterOverflow {
                resource: ResourceKind::OutputBytes,
            })
        );
    }
}
