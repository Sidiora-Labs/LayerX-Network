//! Caller-declared execution ceilings and trusted activity admission.

use core::fmt::{self, Display};

use crate::{FeeSchedule, PrincipalId, ResourceBudget};

/// Canonical domain separating a declared execution budget from every receipt.
pub const DECLARED_BUDGET_DOMAIN: &[u8] = b"LXP/program-declared-budget/v1\0";
/// Canonical empty access-set charge plus the smallest instrumented empty entry.
pub const MIN_ACTIVITY_CPU_FUEL: u64 = 39;

/// Nonzero canonical activity identity bound into one admitted budget token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivityBudgetBinding([u8; 32]);

impl ActivityBudgetBinding {
    /// Constructs a nonzero activity binding.
    ///
    /// # Errors
    ///
    /// Returns malformed when the transition supplies the all-zero identity.
    pub const fn new(bytes: [u8; 32]) -> Result<Self, BudgetAdmissionRefusal> {
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] != 0 {
                return Ok(Self(bytes));
            }
            index += 1;
        }
        Err(BudgetAdmissionRefusal::MalformedCanonicalBytes)
    }

    /// Returns the canonical activity binding bytes.
    #[must_use]
    pub const fn bytes(self) -> [u8; 32] {
        self.0
    }
}

/// One component of a caller-declared execution ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetDimension {
    /// Interpreter instruction fuel.
    CpuFuel,
    /// Peak guest linear memory.
    MemoryBytes,
    /// Cumulative storage bytes read.
    StorageReadBytes,
    /// Cumulative storage bytes written.
    StorageWriteBytes,
    /// Integer values returned across the guest boundary.
    OutputValues,
    /// Response or refusal bytes returned across call boundaries.
    OutputBytes,
    /// Peak guest table elements.
    TableElements,
}

impl Display for BudgetDimension {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CpuFuel => write!(formatter, "cpu fuel"),
            Self::MemoryBytes => write!(formatter, "memory bytes"),
            Self::StorageReadBytes => write!(formatter, "storage read bytes"),
            Self::StorageWriteBytes => write!(formatter, "storage write bytes"),
            Self::OutputValues => write!(formatter, "output values"),
            Self::OutputBytes => write!(formatter, "output bytes"),
            Self::TableElements => write!(formatter, "table elements"),
        }
    }
}

/// Typed refusal produced before any activity guest code can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetAdmissionRefusal {
    /// A component was below the smallest proven version-one execution.
    BelowMinimum {
        /// Component that was too small.
        dimension: BudgetDimension,
        /// Inclusive minimum.
        minimum: u64,
        /// Caller-declared value.
        declared: u64,
    },
    /// A component exceeded the executor's effective protocol maximum.
    AboveMaximum {
        /// Component that was too large.
        dimension: BudgetDimension,
        /// Inclusive maximum.
        maximum: u64,
        /// Caller-declared value.
        declared: u64,
    },
    /// The maximum fee could not be represented exactly.
    CeilingFeeOverflow,
    /// The authenticated payer cannot cover the admitted maximum fee.
    InsufficientCoverage {
        /// Maximum fee units required by the declaration.
        required: u128,
        /// Authenticated fee units available for reservation.
        available: u128,
    },
    /// Canonical declared-budget bytes were malformed, truncated, or trailing.
    MalformedCanonicalBytes,
    /// An admitted token was presented under a different fee schedule.
    ScheduleMismatch,
    /// The independently carried payer did not match the admitted payer.
    PayerMismatch,
    /// The independently carried activity identity did not match the token.
    ActivityBindingMismatch,
    /// The token was admitted under a different effective maximum policy.
    MaximumPolicyMismatch,
}

impl Display for BudgetAdmissionRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BelowMinimum {
                dimension,
                minimum,
                declared,
            } => write!(
                formatter,
                "declared {dimension} {declared} is below minimum {minimum}"
            ),
            Self::AboveMaximum {
                dimension,
                maximum,
                declared,
            } => write!(
                formatter,
                "declared {dimension} {declared} exceeds maximum {maximum}"
            ),
            Self::CeilingFeeOverflow => write!(formatter, "declared budget fee ceiling overflowed"),
            Self::InsufficientCoverage {
                required,
                available,
            } => write!(
                formatter,
                "payer coverage {available} is below required ceiling {required}"
            ),
            Self::MalformedCanonicalBytes => {
                write!(formatter, "malformed canonical declared-budget bytes")
            }
            Self::ScheduleMismatch => {
                write!(
                    formatter,
                    "admitted budget fee schedule does not match executor"
                )
            }
            Self::PayerMismatch => {
                write!(formatter, "admitted budget payer does not match request")
            }
            Self::ActivityBindingMismatch => {
                write!(
                    formatter,
                    "admitted budget activity binding does not match request"
                )
            }
            Self::MaximumPolicyMismatch => {
                write!(
                    formatter,
                    "admitted budget maximum policy does not match executor"
                )
            }
        }
    }
}

impl std::error::Error for BudgetAdmissionRefusal {}

/// Seven caller-declared resource ceilings, validated against version-one law.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeclaredBudget {
    resources: ResourceBudget,
}

impl DeclaredBudget {
    /// Validates all dimensions in their canonical stable order.
    ///
    /// # Errors
    ///
    /// Returns a typed refusal for the first component outside version-one bounds.
    pub const fn new(
        cpu_fuel: u64,
        memory_bytes: u64,
        storage_read_bytes: u64,
        storage_write_bytes: u64,
        output_values: u32,
        output_bytes: u64,
        table_elements: u32,
    ) -> Result<Self, BudgetAdmissionRefusal> {
        let resources = ResourceBudget::new_complete(
            cpu_fuel,
            memory_bytes,
            storage_read_bytes,
            storage_write_bytes,
            output_values,
            output_bytes,
            table_elements,
        );
        match validate_bounds(resources, ResourceBudget::declared()) {
            Ok(()) => Ok(Self { resources }),
            Err(refusal) => Err(refusal),
        }
    }

    /// Returns the version-one protocol maximum as a valid declaration.
    #[must_use]
    pub const fn protocol_maximum() -> Self {
        Self {
            resources: ResourceBudget::declared(),
        }
    }

    /// Returns the smallest declaration proven to execute the canonical empty call.
    #[must_use]
    pub const fn minimum() -> Self {
        Self {
            resources: ResourceBudget::new_complete(
                MIN_ACTIVITY_CPU_FUEL,
                65_536,
                0,
                0,
                1,
                0,
                0,
            ),
        }
    }

    /// Returns the effective raw meter limits.
    #[must_use]
    pub const fn resource_budget(self) -> ResourceBudget {
        self.resources
    }

    /// Returns the instruction-fuel ceiling.
    #[must_use]
    pub const fn cpu_fuel(self) -> u64 {
        self.resources.cpu_fuel()
    }

    /// Returns the peak-memory ceiling.
    #[must_use]
    pub const fn memory_bytes(self) -> u64 {
        self.resources.memory_bytes()
    }

    /// Returns the storage-read ceiling.
    #[must_use]
    pub const fn storage_read_bytes(self) -> u64 {
        self.resources.storage_read_bytes()
    }

    /// Returns the storage-write ceiling.
    #[must_use]
    pub const fn storage_write_bytes(self) -> u64 {
        self.resources.storage_write_bytes()
    }

    /// Returns the result-value ceiling.
    #[must_use]
    pub const fn output_values(self) -> u32 {
        self.resources.output_values()
    }

    /// Returns the returned-byte ceiling.
    #[must_use]
    pub const fn output_bytes(self) -> u64 {
        self.resources.output_bytes()
    }

    /// Returns the table-element ceiling.
    #[must_use]
    pub const fn table_elements(self) -> u32 {
        self.resources.table_elements()
    }

    /// Encodes all seven fields with fixed-width big-endian integers.
    #[must_use]
    pub fn canonical_bytes(self) -> Vec<u8> {
        let mut encoded = Vec::with_capacity(DECLARED_BUDGET_DOMAIN.len() + 48);
        encoded.extend_from_slice(DECLARED_BUDGET_DOMAIN);
        encoded.extend_from_slice(&self.cpu_fuel().to_be_bytes());
        encoded.extend_from_slice(&self.memory_bytes().to_be_bytes());
        encoded.extend_from_slice(&self.storage_read_bytes().to_be_bytes());
        encoded.extend_from_slice(&self.storage_write_bytes().to_be_bytes());
        encoded.extend_from_slice(&self.output_values().to_be_bytes());
        encoded.extend_from_slice(&self.output_bytes().to_be_bytes());
        encoded.extend_from_slice(&self.table_elements().to_be_bytes());
        encoded
    }

    /// Strictly decodes and revalidates canonical declared-budget bytes.
    ///
    /// # Errors
    ///
    /// Returns malformed for the wrong domain, length, truncation, or trailing bytes,
    /// and returns the same bound refusal as [`Self::new`] for invalid values.
    pub fn canonical_decode(encoded: &[u8]) -> Result<Self, BudgetAdmissionRefusal> {
        const FIELDS_BYTES: usize = 48;
        if encoded.len() != DECLARED_BUDGET_DOMAIN.len() + FIELDS_BYTES
            || !encoded.starts_with(DECLARED_BUDGET_DOMAIN)
        {
            return Err(BudgetAdmissionRefusal::MalformedCanonicalBytes);
        }
        let mut cursor = DECLARED_BUDGET_DOMAIN.len();
        let cpu_fuel = take_u64(encoded, &mut cursor)?;
        let memory_bytes = take_u64(encoded, &mut cursor)?;
        let storage_read_bytes = take_u64(encoded, &mut cursor)?;
        let storage_write_bytes = take_u64(encoded, &mut cursor)?;
        let output_values = take_u32(encoded, &mut cursor)?;
        let output_bytes = take_u64(encoded, &mut cursor)?;
        let table_elements = take_u32(encoded, &mut cursor)?;
        if cursor != encoded.len() {
            return Err(BudgetAdmissionRefusal::MalformedCanonicalBytes);
        }
        Self::new(
            cpu_fuel,
            memory_bytes,
            storage_read_bytes,
            storage_write_bytes,
            output_values,
            output_bytes,
            table_elements,
        )
    }
}

/// Authenticated payer coverage supplied by the protocol transition.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct PayerCoverage {
    payer: PrincipalId,
    activity_binding: ActivityBudgetBinding,
    available_fee_units: u128,
}

impl PayerCoverage {
    pub(crate) const fn new(
        payer: PrincipalId,
        activity_binding: ActivityBudgetBinding,
        available_fee_units: u128,
    ) -> Self {
        Self {
            payer,
            activity_binding,
            available_fee_units,
        }
    }

    pub(crate) const fn into_parts(self) -> (PrincipalId, ActivityBudgetBinding, u128) {
        (self.payer, self.activity_binding, self.available_fee_units)
    }
}

/// Immutable token proving one declaration was admitted under one schedule.
#[derive(Debug, PartialEq, Eq)]
pub struct AdmittedBudget {
    resources: ResourceBudget,
    payer: PrincipalId,
    activity_binding: ActivityBudgetBinding,
    maximum_fee_units: u128,
    schedule: FeeSchedule,
    maximum_policy: ResourceBudget,
}

impl AdmittedBudget {
    pub(crate) const fn new(
        resources: ResourceBudget,
        payer: PrincipalId,
        activity_binding: ActivityBudgetBinding,
        maximum_fee_units: u128,
        schedule: FeeSchedule,
        maximum_policy: ResourceBudget,
    ) -> Self {
        Self {
            resources,
            payer,
            activity_binding,
            maximum_fee_units,
            schedule,
            maximum_policy,
        }
    }

    /// Returns the exact effective meter limits.
    #[must_use]
    pub const fn resource_budget(&self) -> ResourceBudget {
        self.resources
    }

    /// Returns the authenticated payer bound into this token.
    #[must_use]
    pub const fn payer(&self) -> PrincipalId {
        self.payer
    }

    /// Returns the exact activity identity bound into this token.
    #[must_use]
    pub const fn activity_binding(&self) -> ActivityBudgetBinding {
        self.activity_binding
    }

    /// Returns the checked maximum fee admitted before execution.
    #[must_use]
    pub const fn maximum_fee_units(&self) -> u128 {
        self.maximum_fee_units
    }

    pub(crate) const fn schedule(&self) -> FeeSchedule {
        self.schedule
    }

    pub(crate) const fn maximum_policy(&self) -> ResourceBudget {
        self.maximum_policy
    }
}

pub(crate) const fn validate_bounds(
    resources: ResourceBudget,
    maximum: ResourceBudget,
) -> Result<(), BudgetAdmissionRefusal> {
    let dimensions = [
        (
            BudgetDimension::CpuFuel,
            resources.cpu_fuel(),
            MIN_ACTIVITY_CPU_FUEL,
            maximum.cpu_fuel(),
        ),
        (
            BudgetDimension::MemoryBytes,
            resources.memory_bytes(),
            65_536,
            maximum.memory_bytes(),
        ),
        (
            BudgetDimension::StorageReadBytes,
            resources.storage_read_bytes(),
            0,
            maximum.storage_read_bytes(),
        ),
        (
            BudgetDimension::StorageWriteBytes,
            resources.storage_write_bytes(),
            0,
            maximum.storage_write_bytes(),
        ),
        (
            BudgetDimension::OutputValues,
            resources.output_values() as u64,
            1,
            maximum.output_values() as u64,
        ),
        (
            BudgetDimension::OutputBytes,
            resources.output_bytes(),
            0,
            maximum.output_bytes(),
        ),
        (
            BudgetDimension::TableElements,
            resources.table_elements() as u64,
            0,
            maximum.table_elements() as u64,
        ),
    ];
    let mut index = 0;
    while index < dimensions.len() {
        let (dimension, declared, minimum, maximum) = dimensions[index];
        if declared < minimum {
            return Err(BudgetAdmissionRefusal::BelowMinimum {
                dimension,
                minimum,
                declared,
            });
        }
        if declared > maximum {
            return Err(BudgetAdmissionRefusal::AboveMaximum {
                dimension,
                maximum,
                declared,
            });
        }
        index += 1;
    }
    Ok(())
}

pub(crate) fn maximum_fee_units(
    resources: ResourceBudget,
    schedule: FeeSchedule,
) -> Result<u128, BudgetAdmissionRefusal> {
    let priced = [
        (
            u128::from(resources.cpu_fuel()),
            u128::from(schedule.cpu_price()),
        ),
        (
            u128::from(resources.memory_bytes()),
            u128::from(schedule.memory_byte_price()),
        ),
        (
            u128::from(resources.storage_read_bytes()),
            u128::from(schedule.storage_read_byte_price()),
        ),
        (
            u128::from(resources.storage_write_bytes()),
            u128::from(schedule.storage_write_byte_price()),
        ),
        (
            u128::from(resources.output_values()),
            u128::from(schedule.output_value_price()),
        ),
        (
            u128::from(resources.output_bytes()),
            u128::from(schedule.output_byte_price()),
        ),
    ];
    checked_fee_components(priced)
}

fn checked_fee_components(priced: [(u128, u128); 6]) -> Result<u128, BudgetAdmissionRefusal> {
    let mut total = 0_u128;
    for (units, price) in priced {
        let component = units
            .checked_mul(price)
            .ok_or(BudgetAdmissionRefusal::CeilingFeeOverflow)?;
        total = total
            .checked_add(component)
            .ok_or(BudgetAdmissionRefusal::CeilingFeeOverflow)?;
    }
    Ok(total)
}

fn take_u64(encoded: &[u8], cursor: &mut usize) -> Result<u64, BudgetAdmissionRefusal> {
    let end = cursor
        .checked_add(8)
        .ok_or(BudgetAdmissionRefusal::MalformedCanonicalBytes)?;
    let bytes: [u8; 8] = encoded
        .get(*cursor..end)
        .ok_or(BudgetAdmissionRefusal::MalformedCanonicalBytes)?
        .try_into()
        .map_err(|_| BudgetAdmissionRefusal::MalformedCanonicalBytes)?;
    *cursor = end;
    Ok(u64::from_be_bytes(bytes))
}

fn take_u32(encoded: &[u8], cursor: &mut usize) -> Result<u32, BudgetAdmissionRefusal> {
    let end = cursor
        .checked_add(4)
        .ok_or(BudgetAdmissionRefusal::MalformedCanonicalBytes)?;
    let bytes: [u8; 4] = encoded
        .get(*cursor..end)
        .ok_or(BudgetAdmissionRefusal::MalformedCanonicalBytes)?
        .try_into()
        .map_err(|_| BudgetAdmissionRefusal::MalformedCanonicalBytes)?;
    *cursor = end;
    Ok(u32::from_be_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::{checked_fee_components, BudgetAdmissionRefusal};

    #[test]
    fn checked_fee_ceiling_rejects_product_and_sum_overflow() {
        assert_eq!(
            checked_fee_components([(u128::MAX, 2), (0, 0), (0, 0), (0, 0), (0, 0), (0, 0)]),
            Err(BudgetAdmissionRefusal::CeilingFeeOverflow)
        );
        assert_eq!(
            checked_fee_components([(u128::MAX, 1), (1, 1), (0, 0), (0, 0), (0, 0), (0, 0),]),
            Err(BudgetAdmissionRefusal::CeilingFeeOverflow)
        );
    }
}
