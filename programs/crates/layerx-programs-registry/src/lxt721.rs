use layerx_programs_runtime::ABI_V2_VERSION;

use crate::{
    InterfaceCapability, InterfaceEntryPoint, InterfaceRefusal, ProgramInterface, ValueSchema,
    ValueType,
};

/// # Errors
/// Refuses a module that does not implement the reference's exact exports and capabilities.
pub fn reference_interface(module: &[u8]) -> Result<ProgramInterface, InterfaceRefusal> {
    let definitions = [
        ("approve", 3, own()),
        ("balance_of", 6, read()),
        ("metadata", 9, vec![]),
        ("mint", 1, own()),
        ("owner_of", 5, read()),
        ("set_approval_for_all", 4, write()),
        ("token_uri", 7, read()),
        ("total_supply", 8, read()),
        ("transfer", 2, own()),
    ];
    ProgramInterface::bind(
        module,
        ABI_V2_VERSION,
        definitions
            .into_iter()
            .map(|(name, ordinal, capabilities)| InterfaceEntryPoint {
                name: name.to_owned(),
                discriminator: [b'L', b'X', TAG, ordinal],
                calldata: ValueSchema::layerx(ValueType::Bytes { max_len: 48 }),
                response: ValueSchema::layerx(ValueType::Bytes { max_len: 64 }),
                capabilities,
                event_topics: vec![],
                failures: vec![],
            })
            .collect(),
    )
}

const TAG: u8 = layerx_program_sdk::lxt721::STANDARD.to_le_bytes()[0];

fn read() -> Vec<InterfaceCapability> {
    vec![
        InterfaceCapability::StorageRead,
        InterfaceCapability::SharedStorageRead,
    ]
}

fn write() -> Vec<InterfaceCapability> {
    vec![
        InterfaceCapability::StorageWrite,
        InterfaceCapability::SharedStorageWrite,
    ]
}

fn own() -> Vec<InterfaceCapability> {
    vec![
        InterfaceCapability::StorageRead,
        InterfaceCapability::StorageWrite,
        InterfaceCapability::SharedStorageRead,
        InterfaceCapability::SharedStorageWrite,
    ]
}
