#![no_std]
use layerx_program_sdk::{EntryResponse,CallResult,ProgramRefusal,trap_on_panic};
use layerx_program_sdk::crypto::HashInput;
use layerx_porting_solana_guest as anchor;
trap_on_panic!();
fn execute(input:&[u8])->Result<EntryResponse<'_>,ProgramRefusal<'static>>{
    let context=anchor::AnchorContext::current().unwrap_or_else(|_|panic!("anchor context"));let digest=anchor::sha256(HashInput::new(input).unwrap_or_else(|_|panic!("bounded instruction"))).unwrap_or_else(|_|panic!("sha256 refusal"));
    if context.signer.bytes()==[0;32]||context.program_id.bytes()==[0;32]||context.slot==0{panic!("mapped result");} let code=u32::from(digest[0]^context.signer.bytes()[31]^context.program_id.bytes()[31]^context.slot.to_be_bytes()[7]);Ok(EntryResponse::new(CallResult::new(code).unwrap_or_else(|_|panic!("result code")),input))
}
layerx_program_sdk::failure_entrypoint!(execute);
