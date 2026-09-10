use layerx_programs::{lxt20::reference_interface, ProgramInterface};
use layerx_programs_runtime::ProgramId;

#[test]
fn real_token_module_binds_the_published_interface_fixture() {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pay5");
    let wasm = std::fs::read(directory.join("token-lxt20.wasm")).unwrap_or_else(|e| panic!("{e}"));
    let program = ProgramId::new([0x55; 32]).unwrap_or_else(|e| panic!("{e}"));
    let interface = reference_interface(&wasm, program).unwrap_or_else(|e| panic!("{e}"));
    let published =
        std::fs::read(directory.join("token-lxt20.interface")).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(interface.canonical_encoding(), published);
    assert_eq!(ProgramInterface::decode(&published), Ok(interface));
}
