//! Scalar-only, activity-owned C ingress for migration validation.
//!
//! C owns the bounded source bytes in the current module-context arena. Rust
//! reads them through one scalar callback and reconstructs transient local
//! input; only the hash-bound compiled artifact may outlive the call.

use crate::{Executor, ModuleCacheKey, RuntimeArtifactOwnerRefusal};

const RESULT_OK: i32 = 0;
const RESULT_NON_CANONICAL: i32 = -3;
const RESULT_LENGTH_LIMIT: i32 = -5;
const RESULT_UNKNOWN_ACTIVITY: i32 = -106;
const RESULT_GAS_EXHAUSTED: i32 = -601;
const RESULT_FATAL_INVARIANT: i32 = -1001;
const MAX_MODULE_BYTES: usize = 1_048_576;
const MAX_HOOK_BYTES: usize = 1_024;
const WASM_SECTION: u16 = 0;
const HOOK_SECTION: u16 = 1;

unsafe extern "C" {
    /// Returns one C-owned activity byte as `0..=255`, or a negative LayerX
    /// refusal. The token and bytes are valid only for this synchronous call.
    fn layerx_programs_migration_activity_byte(token: u64, section: u16, offset: u32) -> i32;
}

fn activity_bytes(token: u64, section: u16, length: usize) -> Result<Vec<u8>, i32> {
    let length_u32 = u32::try_from(length).map_err(|_| RESULT_LENGTH_LIMIT)?;
    let mut bytes = Vec::with_capacity(length);
    for offset in 0..length_u32 {
        // The C boundary returns only a scalar byte or typed refusal; it never
        // shares a pointer, and the token is owned by the active C context.
        let value = unsafe { layerx_programs_migration_activity_byte(token, section, offset) };
        let byte = u8::try_from(value).map_err(|_| {
            if value < 0 {
                value
            } else {
                RESULT_NON_CANONICAL
            }
        })?;
        bytes.push(byte);
    }
    Ok(bytes)
}

/// Validates and executes one C-owned migration request under declared
/// metering. The activity token is transient and cannot outlive its module
/// context; this bridge owns no pending state.
#[no_mangle]
pub extern "C" fn layerx_programs_migration_execute_activity(
    token: u64,
    wasm_length: u32,
    hook_length: u16,
    abi_version: u16,
    h0: u64,
    h1: u64,
    h2: u64,
    h3: u64,
) -> i32 {
    let wasm_length = wasm_length as usize;
    let hook_length = hook_length as usize;
    if token == 0
        || wasm_length == 0
        || wasm_length > MAX_MODULE_BYTES
        || hook_length == 0
        || hook_length > MAX_HOOK_BYTES
    {
        return RESULT_NON_CANONICAL;
    }
    let wasm = match activity_bytes(token, WASM_SECTION, wasm_length) {
        Ok(wasm) => wasm,
        Err(refusal) => return refusal,
    };
    let hook = match activity_bytes(token, HOOK_SECTION, hook_length) {
        Ok(hook) => hook,
        Err(refusal) => return refusal,
    };
    let Ok(hook) = String::from_utf8(hook) else {
        return RESULT_NON_CANONICAL;
    };
    let mut code_hash = [0_u8; 32];
    for (chunk, word) in code_hash.chunks_exact_mut(8).zip([h0, h1, h2, h3]) {
        chunk.copy_from_slice(&word.to_be_bytes());
    }
    let owner = match crate::cache::runtime_artifacts() {
        Ok(owner) => owner,
        Err(_) => return RESULT_FATAL_INVARIANT,
    };
    let module = match owner.get_or_compile(
        ModuleCacheKey::new(code_hash, crate::RUNTIME_VERSION, abi_version),
        &wasm,
    ) {
        Ok(module) => module,
        Err(RuntimeArtifactOwnerRefusal::Compilation(_)) => return RESULT_NON_CANONICAL,
        Err(
            RuntimeArtifactOwnerRefusal::Initialization(_)
            | RuntimeArtifactOwnerRefusal::SynchronizationPoisoned,
        ) => return RESULT_FATAL_INVARIANT,
    };
    match Executor::declared().execute(module.validated(), &hook, &[]) {
        Ok(_) => RESULT_OK,
        Err(crate::ExecutionError::Resource(_)) => RESULT_GAS_EXHAUSTED,
        Err(crate::ExecutionError::Fault(crate::ExecutionFault::UnknownExport { .. })) => {
            RESULT_UNKNOWN_ACTIVITY
        }
        Err(_) => RESULT_NON_CANONICAL,
    }
}
