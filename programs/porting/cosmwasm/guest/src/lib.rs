#![no_std]
//! Guest-side CosmWasm environment mapped onto ABI v2.
#[cfg(target_arch="wasm32")] use layerx_program_sdk::{Context,Principal,ProgramError,ProgramId};
#[cfg(target_arch="wasm32")] use layerx_program_sdk::crypto::{self,HashAlgorithm,HashInput};
/// Deterministic subset of `Env`.
#[cfg(target_arch="wasm32")]
pub struct Env{pub contract:ProgramId,pub block_height:u64}
/// Deterministic subset of `MessageInfo`.
#[cfg(target_arch="wasm32")]
pub struct MessageInfo{pub sender:Principal}
/// Reads authenticated `Env` and `MessageInfo` values.
#[cfg(target_arch="wasm32")]
pub fn current()->Result<(Env,MessageInfo),ProgramError>{Ok((Env{contract:Context::executing_program()?,block_height:Context::batch_height()?},MessageInfo{sender:Context::invoking_principal()?}))}
/// Calls the ABI v2 BLAKE3 primitive.
#[cfg(target_arch="wasm32")]
pub fn blake3(input:HashInput<'_>)->Result<[u8;32],ProgramError>{crypto::hash(HashAlgorithm::Blake3,input)}
