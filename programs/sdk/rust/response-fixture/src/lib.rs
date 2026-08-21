#![no_std]

use layerx_program_sdk::{
    call, trap_on_panic, CallInput, CallResult, CapabilitySet, EntryResponse, ProgramId,
    ProgramRefusal, RefusalClass, RefusalReason,
};

const MAXIMUM_REASON: [u8; layerx_program_sdk::MAX_REFUSAL_REASON_BYTES] =
    [0xa5; layerx_program_sdk::MAX_REFUSAL_REASON_BYTES];

trap_on_panic!();

fn refusal<'a>(class: RefusalClass, bytes: &'a [u8]) -> ProgramRefusal<'a> {
    let reason = RefusalReason::new(bytes).unwrap_or_else(|_| panic!("bounded fixture reason"));
    ProgramRefusal::new(class, reason).unwrap_or_else(|_| panic!("guest refusal class"))
}

fn echo<'a>(input: &'a [u8]) -> Result<EntryResponse<'a>, ProgramRefusal<'a>> {
    if input.first() == Some(&0xf0) {
        return Err(refusal(RefusalClass::InvalidInput, &[0, 0xff, 0x80]));
    }
    if input.first() == Some(&0xf2) {
        return Err(refusal(RefusalClass::Rejected, &[]));
    }
    if input.first() == Some(&0xf3) {
        return Err(refusal(RefusalClass::Unauthorized, &MAXIMUM_REASON));
    }
    if input.first() == Some(&0x7f) {
        return Ok(EntryResponse::new(CallResult::OK, input));
    }
    let callee = ProgramId::new([7; 32]).unwrap_or_else(|_| panic!("fixture program id"));
    let capabilities = CapabilitySet::<0>::empty();
    let mut capability_scratch = [0u8; 2];
    let mut nested_output = [0u8; 32];
    let nested_input = if input.first() == Some(&0xf1) {
        [0xf0, 0, 0xff]
    } else {
        [0x7f, 0, 0xff]
    };
    let response = call::invoke_response_with(
        callee,
        CallInput::new(&nested_input).unwrap_or_else(|_| panic!("bounded fixture input")),
        &capabilities,
        &mut capability_scratch,
        &mut nested_output,
    )
    .unwrap_or_else(|_| panic!("nested refusal traps before returning"));
    if response.code() != CallResult::OK.code() || response.bytes() != nested_input {
        return Err(refusal(RefusalClass::Conflict, b"response mismatch"));
    }
    Ok(EntryResponse::new(CallResult::OK, input))
}

layerx_program_sdk::failure_entrypoint!(echo);
