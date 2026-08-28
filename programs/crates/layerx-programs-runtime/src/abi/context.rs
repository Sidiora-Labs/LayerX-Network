//! Frozen field-addressed execution context for the candidate ABI.
//!
//! Program and principal identifiers are their exact 32 canonical bytes.
//! Unsigned integers are fixed-width big-endian. The immediate caller is
//! encoded as `0x00` when absent or `0x01 || program_id[32]` when present, so
//! the root frame can never be confused with a fabricated zero identifier.

use crate::storage::{PrincipalId, ProgramId};

/// Fuel charged for every context byte returned to guest memory.
pub const CONTEXT_FUEL_PER_BYTE: u64 = 1;

/// Largest encoded context field: the optional caller tag and program id.
pub const MAX_CONTEXT_FIELD_BYTES: usize = 33;

/// Frozen candidate-v2 execution-context field identifiers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContextField {
    /// Canonical 32-byte identifier of the program owning the active frame.
    ExecutingProgram = 1,
    /// Tagged optional canonical identifier of the frame's immediate caller.
    ImmediateCaller = 2,
    /// Canonical 32-byte principal that invoked the activity.
    InvokingPrincipal = 3,
    /// Canonical eight-byte global activity sequence.
    ActivitySequence = 4,
    /// Canonical eight-byte protocol batch height.
    BatchHeight = 5,
    /// Canonical two-byte deterministic runtime version.
    RuntimeVersion = 6,
    /// Canonical two-byte ABI version selected for the deployed program.
    AbiVersion = 7,
    /// Canonical eight-byte fuel remaining after paying for this context read.
    RemainingFuel = 8,
    /// Canonical four-byte effective governed fee-schedule version.
    FeeScheduleVersion = 9,
}

impl TryFrom<i32> for ContextField {
    type Error = ContextRefusal;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::ExecutingProgram),
            2 => Ok(Self::ImmediateCaller),
            3 => Ok(Self::InvokingPrincipal),
            4 => Ok(Self::ActivitySequence),
            5 => Ok(Self::BatchHeight),
            6 => Ok(Self::RuntimeVersion),
            7 => Ok(Self::AbiVersion),
            8 => Ok(Self::RemainingFuel),
            9 => Ok(Self::FeeScheduleVersion),
            _ => Err(ContextRefusal::UnknownField),
        }
    }
}

/// Typed refusal produced before an execution-context value can escape.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextRefusal {
    /// The numeric field identifier is not part of the frozen table.
    UnknownField,
    /// The protocol transition did not supply complete authenticated facts.
    Unauthenticated,
    /// Runtime authorization and the active host-owned frame disagree.
    FrameMismatch,
}

/// Protocol-owned facts fixed before any candidate-v2 guest executes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutionContext {
    activity_sequence: u64,
    batch_height: u64,
    runtime_version: u16,
    abi_version: u16,
    fee_schedule_version: u32,
}

impl ExecutionContext {
    pub(crate) fn canonical_bytes(self) -> [u8; 24] {
        let mut bytes = [0_u8; 24];
        bytes[..8].copy_from_slice(&self.activity_sequence.to_be_bytes());
        bytes[8..16].copy_from_slice(&self.batch_height.to_be_bytes());
        bytes[16..18].copy_from_slice(&self.runtime_version.to_be_bytes());
        bytes[18..20].copy_from_slice(&self.abi_version.to_be_bytes());
        bytes[20..].copy_from_slice(&self.fee_schedule_version.to_be_bytes());
        bytes
    }

    pub(crate) const fn authenticated(
        activity_sequence: u64,
        batch_height: u64,
        runtime_version: u16,
        abi_version: u16,
        fee_schedule_version: u32,
    ) -> Result<Self, ContextRefusal> {
        if activity_sequence == 0
            || batch_height == 0
            || runtime_version == 0
            || abi_version == 0
            || fee_schedule_version == 0
        {
            return Err(ContextRefusal::Unauthenticated);
        }
        Ok(Self {
            activity_sequence,
            batch_height,
            runtime_version,
            abi_version,
            fee_schedule_version,
        })
    }

    pub(crate) fn encode(
        self,
        field: ContextField,
        executing_program: ProgramId,
        immediate_caller: Option<ProgramId>,
        invoking_principal: PrincipalId,
        remaining_fuel: u64,
    ) -> Vec<u8> {
        match field {
            ContextField::ExecutingProgram => executing_program.bytes().to_vec(),
            ContextField::ImmediateCaller => match immediate_caller {
                None => vec![0],
                Some(caller) => {
                    let mut encoded = Vec::with_capacity(33);
                    encoded.push(1);
                    encoded.extend_from_slice(&caller.bytes());
                    encoded
                }
            },
            ContextField::InvokingPrincipal => invoking_principal.bytes().to_vec(),
            ContextField::ActivitySequence => self.activity_sequence.to_be_bytes().to_vec(),
            ContextField::BatchHeight => self.batch_height.to_be_bytes().to_vec(),
            ContextField::RuntimeVersion => self.runtime_version.to_be_bytes().to_vec(),
            ContextField::AbiVersion => self.abi_version.to_be_bytes().to_vec(),
            ContextField::RemainingFuel => remaining_fuel.to_be_bytes().to_vec(),
            ContextField::FeeScheduleVersion => self.fee_schedule_version.to_be_bytes().to_vec(),
        }
    }

    pub(crate) const fn authenticates_versions(
        self,
        runtime_version: u16,
        abi_version: u16,
        fee_schedule_version: u32,
    ) -> bool {
        self.runtime_version == runtime_version
            && self.abi_version == abi_version
            && self.fee_schedule_version == fee_schedule_version
    }
}

#[cfg(test)]
mod tests {
    use super::{ContextField, ContextRefusal, ExecutionContext};
    use crate::{PrincipalId, ProgramId};

    #[test]
    fn frozen_field_ids_and_encodings_are_canonical() {
        let context = ExecutionContext::authenticated(0x0102_0304_0506_0708, 9, 1, 2, 3)
            .expect("authenticated context");
        let program = ProgramId::new([0x11; 32]).expect("program");
        let caller = ProgramId::new([0x22; 32]).expect("caller");
        let principal = PrincipalId::new([0x33; 32]).expect("principal");
        let vectors = [
            (1, vec![0x11; 32]),
            (2, [vec![1], vec![0x22; 32]].concat()),
            (3, vec![0x33; 32]),
            (4, vec![1, 2, 3, 4, 5, 6, 7, 8]),
            (5, vec![0, 0, 0, 0, 0, 0, 0, 9]),
            (6, vec![0, 1]),
            (7, vec![0, 2]),
            (8, vec![0, 0, 0, 0, 0, 0, 0, 10]),
            (9, vec![0, 0, 0, 3]),
        ];
        for (id, expected) in vectors {
            let field = ContextField::try_from(id).expect("known field");
            assert_eq!(
                context.encode(field, program, Some(caller), principal, 10),
                expected
            );
        }
        assert_eq!(
            context.encode(
                ContextField::ImmediateCaller,
                program,
                None,
                principal,
                10
            ),
            [0]
        );
        assert_eq!(ContextField::try_from(0), Err(ContextRefusal::UnknownField));
        assert_eq!(ContextField::try_from(10), Err(ContextRefusal::UnknownField));
    }
}
