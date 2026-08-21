//! Entrypoint plumbing owned by the SDK.
//!
//! The composition layer enters a callee by asking it to reserve a bounded
//! region of its own linear memory, writing the call input there, and then
//! invoking the call entry export. The reservation lives here so a program
//! declares its entry points without writing a single unsafe line of its own.

use core::cell::UnsafeCell;

use crate::abi::MAX_CALL_INPUT_BYTES;
use crate::error::{Field, ProgramError, Reason};
use crate::CallResult;

/// Declared capacity of the call-input reservation every SDK program owns.
pub const CALL_INPUT_CAPACITY: usize = MAX_CALL_INPUT_BYTES;

const RESERVATION_REFUSED: i32 = -1;

/// Response returned synchronously by a candidate entry handler.
pub struct EntryResponse<'a> {
    pub result: CallResult,
    pub bytes: &'a [u8],
}

impl<'a> EntryResponse<'a> {
    #[must_use]
    pub const fn new(result: CallResult, bytes: &'a [u8]) -> Self {
        Self { result, bytes }
    }
}

struct CallBuffer(UnsafeCell<[u8; CALL_INPUT_CAPACITY]>);

unsafe impl Sync for CallBuffer {}

static CALL_INPUT: CallBuffer = CallBuffer(UnsafeCell::new([0; CALL_INPUT_CAPACITY]));

fn reservation() -> *mut u8 {
    CALL_INPUT.0.get().cast::<u8>()
}

/// Reserves the call-input region for a caller of the declared length,
/// returning its address or a negative refusal.
#[must_use]
pub fn reserve_call_input(length: i32) -> i32 {
    let Ok(requested) = usize::try_from(length) else {
        return RESERVATION_REFUSED;
    };
    if requested > CALL_INPUT_CAPACITY {
        return RESERVATION_REFUSED;
    }
    i32::try_from(reservation() as usize).unwrap_or(RESERVATION_REFUSED)
}

/// Borrows the call input the caller wrote into the reserved region.
///
/// # Errors
///
/// Refuses a length past the declared reservation and any pointer other than
/// the one [`reserve_call_input`] handed out.
pub fn with_call_input<T>(
    pointer: i32,
    length: i32,
    handler: impl FnOnce(&[u8]) -> T,
) -> Result<T, ProgramError> {
    let requested = usize::try_from(length)
        .map_err(|_| ProgramError::value(Field::CallInput, Reason::Malformed))?;
    if requested > CALL_INPUT_CAPACITY {
        return Err(ProgramError::value(Field::CallInput, Reason::TooLarge));
    }
    let base = reservation();
    let offset = usize::try_from(pointer)
        .map_err(|_| ProgramError::value(Field::CallInput, Reason::Malformed))?;
    if requested > 0 && offset != base as usize {
        return Err(ProgramError::value(Field::CallInput, Reason::Malformed));
    }
    let input = unsafe { core::slice::from_raw_parts(base.cast_const(), requested) };
    Ok(handler(input))
}
