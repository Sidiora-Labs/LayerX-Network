#![no_std]
use layerx_program_sdk::{EntryResponse,CallResult,ProgramRefusal,trap_on_panic};
use layerx_program_sdk::crypto::HashInput;
use layerx_porting_cosmwasm_guest as cosmwasm;
trap_on_panic!();
fn execute(input:&[u8])->Result<EntryResponse<'_>,ProgramRefusal<'static>>{
    let (env,info)=cosmwasm::current().unwrap_or_else(|_|panic!("context"));let digest=cosmwasm::blake3(HashInput::new(input).unwrap_or_else(|_|panic!("bounded message"))).unwrap_or_else(|_|panic!("blake3 refusal"));
    if info.sender.bytes()==[0;32]||env.contract.bytes()==[0;32]||env.block_height==0{panic!("mapped result");}let code=u32::from(digest[0]^info.sender.bytes()[31]^env.contract.bytes()[31]^env.block_height.to_be_bytes()[7]);Ok(EntryResponse::new(CallResult::new(code).unwrap_or_else(|_|panic!("result code")),input))
}
layerx_program_sdk::failure_entrypoint!(execute);
