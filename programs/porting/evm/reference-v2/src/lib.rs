#![no_std]
use layerx_program_sdk::{Context,EntryResponse,CallResult,ProgramRefusal,trap_on_panic};
use layerx_program_sdk::crypto::{self,HashAlgorithm,HashInput,RecoveryId};
trap_on_panic!();
fn execute(input:&[u8])->Result<EntryResponse<'_>,ProgramRefusal<'static>>{
    let digest=crypto::hash(HashAlgorithm::Keccak256,HashInput::new(input).unwrap_or_else(|_|panic!("bounded calldata"))).unwrap_or_else(|_|panic!("keccak refusal"));
    let _digest=digest;
    let recovery=RecoveryId::new(0).unwrap_or_else(|_|panic!("recovery id"));
    let _typed_recovery=crypto::secp256k1_recover(&digest,&[0u8;64],recovery);
    let _sender=Context::invoking_principal().unwrap_or_else(|_|panic!("msg.sender"));let _contract=Context::executing_program().unwrap_or_else(|_|panic!("address(this)"));let _height=Context::batch_height().unwrap_or_else(|_|panic!("block.number"));
    Ok(EntryResponse::new(CallResult::OK,input))
}
layerx_program_sdk::failure_entrypoint!(execute);
