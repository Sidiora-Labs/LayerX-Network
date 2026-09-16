use layerx_programs::{lxt721::reference_interface, InterfaceCapability, ProgramInterface};

#[test]
fn real_nft_module_binds_the_published_interface_fixture() {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lxt721");
    let wasm = std::fs::read(directory.join("nft-lxt721.wasm")).unwrap_or_else(|e| panic!("{e}"));
    let interface = reference_interface(&wasm).unwrap_or_else(|e| panic!("{e}"));
    let published =
        std::fs::read(directory.join("nft-lxt721.interface")).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(interface.canonical_encoding(), published);
    assert_eq!(ProgramInterface::decode(&published), Ok(interface.clone()));
    assert!(interface
        .entries()
        .iter()
        .all(|entry| entry.capabilities.iter().all(|capability| matches!(
            capability,
            InterfaceCapability::StorageRead
                | InterfaceCapability::StorageWrite
                | InterfaceCapability::SharedStorageRead
                | InterfaceCapability::SharedStorageWrite
        ))));
}
