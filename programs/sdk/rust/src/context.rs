//! Authenticated execution context exposed by the version-two ABI.

use crate::{ProgramError, ProgramId};

const PROGRAM_BYTES: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
enum Field { ExecutingProgram=1, ImmediateCaller=2, InvokingPrincipal=3, ActivitySequence=4, BatchHeight=5, RuntimeVersion=6, AbiVersion=7, RemainingFuel=8, FeeScheduleVersion=9 }

/// Opaque canonical identifier of the principal that invoked this activity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Principal([u8; PROGRAM_BYTES]);
impl Principal { #[must_use] pub const fn bytes(self) -> [u8; PROGRAM_BYTES] { self.0 } }

/// Protocol-authenticated facts for the active call frame.
pub struct Context;

#[cfg(target_arch = "wasm32")]
impl Context {
    fn read<const N: usize>(field: Field) -> Result<[u8; N], ProgramError> {
        let mut output=[0u8;N];
        crate::host::context_read(field as i32, &mut output)?;
        Ok(output)
    }
    /// Returns `address(this)`. # Errors Refuses unauthenticated context.
    pub fn executing_program() -> Result<ProgramId, ProgramError> { ProgramId::new(Self::read::<32>(Field::ExecutingProgram)?) }
    /// Returns the immediate caller frame. # Errors Refuses malformed or unauthenticated context.
    pub fn immediate_caller() -> Result<Option<ProgramId>, ProgramError> {
        let mut output=[0u8;33];
        let length=crate::host::context_read(Field::ImmediateCaller as i32, &mut output)?;
        match length { 1 if output[0]==0 => Ok(None), 33 if output[0]==1 => { let mut id=[0u8;32]; id.copy_from_slice(&output[1..]); Ok(Some(ProgramId::new(id)?)) }, _ => Err(ProgramError::value(crate::Field::Buffer, crate::Reason::Malformed)) }
    }
    /// Returns the activity principal. # Errors Refuses unauthenticated context.
    pub fn invoking_principal() -> Result<Principal, ProgramError> { Ok(Principal(Self::read::<32>(Field::InvokingPrincipal)?)) }
    /// Returns the activity sequence. # Errors Refuses unauthenticated context.
    pub fn activity_sequence() -> Result<u64, ProgramError> { Ok(u64::from_be_bytes(Self::read::<8>(Field::ActivitySequence)?)) }
    /// Returns the batch height. # Errors Refuses unauthenticated context.
    pub fn batch_height() -> Result<u64, ProgramError> { Ok(u64::from_be_bytes(Self::read::<8>(Field::BatchHeight)?)) }
    /// Returns the runtime version. # Errors Refuses unauthenticated context.
    pub fn runtime_version() -> Result<u16, ProgramError> { Ok(u16::from_be_bytes(Self::read::<2>(Field::RuntimeVersion)?)) }
    /// Returns the admitted ABI version. # Errors Refuses unauthenticated context.
    pub fn abi_version() -> Result<u16, ProgramError> { Ok(u16::from_be_bytes(Self::read::<2>(Field::AbiVersion)?)) }
    /// Returns fuel after this read. # Errors Refuses unauthenticated context.
    pub fn remaining_fuel() -> Result<u64, ProgramError> { Ok(u64::from_be_bytes(Self::read::<8>(Field::RemainingFuel)?)) }
    /// Returns the effective fee schedule. # Errors Refuses unauthenticated context.
    pub fn fee_schedule_version() -> Result<u32, ProgramError> { Ok(u32::from_be_bytes(Self::read::<4>(Field::FeeScheduleVersion)?)) }
}
