use layerx_program_sdk::naming::{
    REFERENCE_ASSET, REFERENCE_OCCUPANCY_CEILING, REFERENCE_OCCUPANCY_SEED,
};
use layerx_programs_runtime::{derive_program_account, ProgramId, ABI_V2_VERSION};

use crate::{
    InterfaceCapability, InterfaceEntryPoint, InterfaceRefusal, ProgramInterface, ValueSchema,
    ValueType,
};

/// # Errors
/// Refuses a module that does not implement the reference's exact exports and capabilities.
pub fn reference_interface(
    module: &[u8],
    program: ProgramId,
) -> Result<ProgramInterface, InterfaceRefusal> {
    let occupancy_account = derive_program_account(program, REFERENCE_OCCUPANCY_SEED)
        .map_err(|_| InterfaceRefusal::Invalid)?;
    let definitions = [
        ("register", 1, occupancy(occupancy_account.bytes())),
        ("renew", 3, occupancy(occupancy_account.bytes())),
        ("resolve", 4, read()),
        ("reverse_resolve", 5, read()),
        ("transfer", 2, write()),
    ];
    ProgramInterface::bind(
        module,
        ABI_V2_VERSION,
        definitions
            .into_iter()
            .map(|(name, ordinal, capabilities)| InterfaceEntryPoint {
                name: name.to_owned(),
                discriminator: [b'L', b'X', b'N', ordinal],
                calldata: ValueSchema::layerx(ValueType::Bytes { max_len: 100 }),
                response: ValueSchema::layerx(ValueType::Bytes { max_len: 64 }),
                capabilities,
                event_topics: vec![],
                failures: vec![],
            })
            .collect(),
    )
}

fn read() -> Vec<InterfaceCapability> {
    vec![
        InterfaceCapability::StorageRead,
        InterfaceCapability::SharedStorageRead,
    ]
}

fn write() -> Vec<InterfaceCapability> {
    vec![
        InterfaceCapability::StorageRead,
        InterfaceCapability::StorageWrite,
        InterfaceCapability::SharedStorageRead,
        InterfaceCapability::SharedStorageWrite,
    ]
}

fn occupancy(account: [u8; 32]) -> Vec<InterfaceCapability> {
    let mut capabilities = write();
    capabilities.push(InterfaceCapability::Transfer402 {
        asset: REFERENCE_ASSET,
        to: account,
        maximum_amount: REFERENCE_OCCUPANCY_CEILING,
    });
    capabilities
}
