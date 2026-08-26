#![no_std]
use layerx_program_sdk::{Context,EntryResponse,CallResult,ProgramRefusal,trap_on_panic};
use layerx_program_sdk::crypto::{self,HashAlgorithm,HashInput};
trap_on_panic!();
fn execute(input:&[u8])->Result<EntryResponse<'_>,ProgramRefusal<'static>>{
    let _sender=Context::invoking_principal().unwrap_or_else(|_|panic!("info.sender"));let _contract=Context::executing_program().unwrap_or_else(|_|panic!("env.contract.address"));let _height=Context::batch_height().unwrap_or_else(|_|panic!("env.block.height"));
    let digest=crypto::hash(HashAlgorithm::Blake3,HashInput::new(input).unwrap_or_else(|_|panic!("bounded message"))).unwrap_or_else(|_|panic!("blake3 refusal"));
    let _digest=digest; Ok(EntryResponse::new(CallResult::OK,input))
}
layerx_program_sdk::failure_entrypoint!(execute);
