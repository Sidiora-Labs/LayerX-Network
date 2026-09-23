use std::process::ExitCode;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use layerx_indexer::abi::AbiRegistry;
use layerx_indexer::api;
use layerx_indexer::config::Config;
use layerx_indexer::follow::StepOutcome;
use layerx_indexer::layerx::LayerXIngester;
use layerx_indexer::paxeer::PaxeerIngester;
use layerx_indexer::store::Store;
use layerx_indexer::IndexError;

fn follow<F>(name: &'static str, store: Arc<Store>, poll: Duration, step: F)
where
    F: Fn(&Store) -> Result<StepOutcome, IndexError> + Send + 'static,
{
    thread::spawn(move || loop {
        match step(&store) {
            Ok(StepOutcome::Advanced { .. } | StepOutcome::RolledBack { .. }) => {}
            Ok(StepOutcome::Idle) => thread::sleep(poll),
            Err(error @ (IndexError::ReorgBeyondFinality { .. } | IndexError::Integrity(_))) => {
                eprintln!("layerx-indexer {name} halted: {error}");
                std::process::exit(2);
            }
            Err(error) => {
                eprintln!("layerx-indexer {name} step failed: {error}");
                thread::sleep(poll);
            }
        }
    });
}

fn run() -> Result<(), IndexError> {
    let config = Config::from_environment()?;
    let store = Arc::new(Store::open(&config.database)?);
    store.register_assets(&config.pointers)?;
    let tls = if config.tls {
        Some(
            layerx_platform_internal::tls::server_config("LAYERX_INDEXER")
                .map_err(IndexError::Config)?,
        )
    } else {
        None
    };
    let listener = api::bind(config.listen, config.tls)?;
    if let Some(source) = config.layerx {
        let ingester = LayerXIngester::new(source.relay, source.policy, source.start_batch);
        follow("layerx", Arc::clone(&store), config.poll, move |store| {
            ingester.step(store)
        });
    }
    if let Some(source) = config.paxeer {
        let registry = config
            .abi_dir
            .as_deref()
            .map_or_else(|| Ok(AbiRegistry::default()), AbiRegistry::load_dir)?;
        eprintln!(
            "layerx-indexer loaded {} precompile ABIs",
            registry.abis().len()
        );
        let ingester = PaxeerIngester::new(
            source.evm,
            source.comet,
            registry,
            source.policy,
            source.start_block,
            source.chain_id,
            source.encoding,
        );
        follow("paxeer", Arc::clone(&store), config.poll, move |store| {
            ingester.step(store)
        });
    }
    eprintln!(
        "layerx-indexer listening on {}{}",
        config.listen,
        if config.tls { " with TLS" } else { "" }
    );
    api::serve(&listener, &store, tls.as_ref());
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("layerx-indexer: {error}");
            ExitCode::FAILURE
        }
    }
}
