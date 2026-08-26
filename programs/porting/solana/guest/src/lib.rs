#![no_std]
//! Guest-side Anchor context and syscall vocabulary mapped onto ABI v2.
#[cfg(target_arch="wasm32")] use layerx_program_sdk::{Context,Principal,ProgramError,ProgramId};
#[cfg(target_arch="wasm32")] use layerx_program_sdk::crypto::{self,HashAlgorithm,HashInput};
/// Anchor accounts/context supplied by authenticated protocol facts.
#[cfg(target_arch="wasm32")]
pub struct AnchorContext{pub program_id:ProgramId,pub signer:Principal,pub slot:u64}
#[cfg(target_arch="wasm32")]
impl AnchorContext{/// Reads the active Anchor context.
pub fn current()->Result<Self,ProgramError>{Ok(Self{program_id:Context::executing_program()?,signer:Context::invoking_principal()?,slot:Context::batch_height()?})}}
/// Calls the SHA-256 syscall equivalent.
#[cfg(target_arch="wasm32")]
pub fn sha256(input:HashInput<'_>)->Result<[u8;32],ProgramError>{crypto::hash(HashAlgorithm::Sha256,input)}
