use tempfile::TempDir;

mod types;
mod zksync_solidity;

struct ServerMetadata {
    base_url: url::Url,
    _zk_compilers_tempdir: TempDir,
    _evm_compilers_tempdir: TempDir,
}

async fn start() -> ServerMetadata {
    let (settings, server) = {
        let mut settings = smart_contract_verifier_server::Settings::default();
        let (server_settings, base) =
            blockscout_service_launcher::test_server::get_test_server_settings();
        settings.server = server_settings;

        settings.solidity.enabled = false;
        settings.vyper.enabled = false;
        settings.sourcify.enabled = false;

        let zk_compilers_tempdir =
            tempfile::tempdir().expect("creation temporary directory for zk_compilers");
        let evm_compilers_tempdir =
            tempfile::tempdir().expect("creation temporary directory for evm_compilers");

        settings.zksync_solidity.enabled = true;
        settings.zksync_solidity.zk_compilers_dir = zk_compilers_tempdir.path().to_path_buf();
        settings.zksync_solidity.evm_compilers_dir = evm_compilers_tempdir.path().to_path_buf();

        settings.metrics.enabled = false;
        settings.tracing.enabled = false;
        settings.jaeger.enabled = false;

        (
            settings,
            ServerMetadata {
                base_url: base,
                _zk_compilers_tempdir: zk_compilers_tempdir,
                _evm_compilers_tempdir: evm_compilers_tempdir,
            },
        )
    };

    blockscout_service_launcher::test_server::init_server(
        || smart_contract_verifier_server::run(settings),
        &server.base_url,
    )
    .await;

    server
}
