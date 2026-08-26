#![no_std]
use layerx_program_sdk::{Context,EntryResponse,CallResult,ProgramRefusal,trap_on_panic};
use layerx_program_sdk::crypto::{self,HashAlgorithm,HashInput};
trap_on_panic!();
fn execute(input:&[u8])->Result<EntryResponse<'_>,ProgramRefusal<'static>>{
    let _signer=Context::invoking_principal().unwrap_or_else(|_|panic!("anchor signer"));let _program=Context::executing_program().unwrap_or_else(|_|panic!("program id"));let _slot=Context::batch_height().unwrap_or_else(|_|panic!("clock slot"));
    let digest=crypto::hash(HashAlgorithm::Sha256,HashInput::new(input).unwrap_or_else(|_|panic!("bounded instruction"))).unwrap_or_else(|_|panic!("sha256 refusal"));
    let _digest=digest; Ok(EntryResponse::new(CallResult::OK,input))
}
layerx_program_sdk::failure_entrypoint!(execute);
