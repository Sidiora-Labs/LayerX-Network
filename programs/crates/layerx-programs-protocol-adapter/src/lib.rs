#![deny(unsafe_code)]

#[allow(unsafe_code)]
mod ffi;

pub use ffi::{read_program_state, ProtocolAdapterError, ProtocolProgramStateRead};
