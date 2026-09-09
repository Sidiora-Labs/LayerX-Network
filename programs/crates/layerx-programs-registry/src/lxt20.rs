use layerx_program_sdk::lxt20::{
    REFERENCE_ASSET, REFERENCE_CEILING, REFERENCE_ISSUER, REFERENCE_SUPPLY,
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
    let issuer_account = derive_program_account(program, &REFERENCE_ISSUER)
        .map_err(|_| InterfaceRefusal::Invalid)?;
    let definitions = [
        (
            "allowance",
            5,
            vec![
                InterfaceCapability::StorageRead,
                InterfaceCapability::SharedStorageRead,
            ],
        ),
        (
            "approve",
            2,
            vec![
                InterfaceCapability::StorageWrite,
                InterfaceCapability::SharedStorageWrite,
            ],
        ),
        (
            "balance_of",
            4,
            vec![
                InterfaceCapability::StorageRead,
                InterfaceCapability::SharedStorageRead,
            ],
        ),
        (
            "initialize",
            0,
            vec![
                InterfaceCapability::StorageRead,
                InterfaceCapability::StorageWrite,
                InterfaceCapability::SharedStorageRead,
                InterfaceCapability::SharedStorageWrite,
                InterfaceCapability::Transfer402 {
                    asset: REFERENCE_ASSET,
                    to: issuer_account.bytes(),
                    maximum_amount: REFERENCE_SUPPLY,
                },
            ],
        ),
        ("metadata", 7, vec![]),
        (
            "total_supply",
            6,
            vec![
                InterfaceCapability::StorageRead,
                InterfaceCapability::SharedStorageRead,
            ],
        ),
        ("transfer", 1, spend(10, 42)),
        ("transfer_from", 3, spend(42, 74)),
    ];
    ProgramInterface::bind(
        module,
        ABI_V2_VERSION,
        definitions
            .into_iter()
            .map(|(name, ordinal, capabilities)| InterfaceEntryPoint {
                name: name.to_owned(),
                discriminator: [b'L', b'X', 20, ordinal],
                calldata: ValueSchema::layerx(ValueType::Bytes { max_len: 80 }),
                response: ValueSchema::layerx(ValueType::Bytes { max_len: 64 }),
                capabilities,
                event_topics: vec![],
                failures: vec![],
            })
            .collect(),
    )
}

fn spend(recipient_offset: u32, amount_offset: u32) -> Vec<InterfaceCapability> {
    vec![
        InterfaceCapability::StorageRead,
        InterfaceCapability::StorageWrite,
        InterfaceCapability::SharedStorageRead,
        InterfaceCapability::SharedStorageWrite,
        InterfaceCapability::CallerAuthorizedSpend {
            asset: REFERENCE_ASSET,
            maximum_amount: REFERENCE_CEILING,
            recipient_offset,
            amount_offset,
        },
    ]
}
