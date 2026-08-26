#![no_std]
//! Guest-side EVM vocabulary mapped onto ABI v2.
#[cfg(target_arch="wasm32")] use layerx_program_sdk::{Context,ProgramError};
#[cfg(target_arch="wasm32")] use layerx_program_sdk::crypto::{self,HashAlgorithm,HashInput,RecoveryId};
#[cfg(target_arch="wasm32")]
fn address(bytes:[u8;32])->[u8;20]{let mut out=[0u8;20];out.copy_from_slice(&bytes[12..]);out}
/// Returns the immediate program caller, or the invoking principal at the root.
#[cfg(target_arch="wasm32")]
pub fn msg_sender()->Result<[u8;20],ProgramError>{match Context::immediate_caller()?{Some(caller)=>Ok(address(caller.bytes())),None=>Ok(address(Context::invoking_principal()?.bytes()))}}
/// Returns the executing program as `address(this)`.
#[cfg(target_arch="wasm32")]
pub fn address_this()->Result<[u8;20],ProgramError>{Ok(address(Context::executing_program()?.bytes()))}
/// Returns authenticated batch height as `block.number`.
#[cfg(target_arch="wasm32")]
pub fn block_number()->Result<u64,ProgramError>{Context::batch_height()}
/// Calls the ABI v2 Keccak-256 primitive.
#[cfg(target_arch="wasm32")]
pub fn keccak256(input:HashInput<'_>)->Result<[u8;32],ProgramError>{crypto::hash(HashAlgorithm::Keccak256,input)}
/// Recovers an EVM address through ABI v2 secp256k1 recovery and Keccak-256.
#[cfg(target_arch="wasm32")]
pub fn ecrecover(digest:&[u8;32],signature:&[u8;64],recovery_id:RecoveryId)->Result<[u8;20],ProgramError>{let key=crypto::secp256k1_recover(digest,signature,recovery_id)?;let digest=keccak256(HashInput::new(&key[1..])?)?;Ok(address(digest))}
