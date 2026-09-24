use std::process::ExitCode;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use layerx_indexer::abi::AbiRegistry;
use layerx_indexer::api;
use layerx_indexer::backfill::Backfill;
use layerx_indexer::config::{BackfillConfig, Config};
use layerx_indexer::follow::StepOutcome;
use layerx_indexer::layerx::LayerXIngester;
use layerx_indexer::paxeer::PaxeerIngester;
use layerx_indexer::paxscan::PaxscanDatabase;
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

fn load_registry(config: &Config) -> Result<AbiRegistry, IndexError> {
    let registry = config
        .abi_dir
        .as_deref()
        .map_or_else(|| Ok(AbiRegistry::default()), AbiRegistry::load_dir)?;
    eprintln!(
        "layerx-indexer loaded {} precompile ABIs",
        registry.abis().len()
    );
    Ok(registry)
}

fn backfill(args: &[String]) -> Result<(), IndexError> {
    let config = Config::from_environment()?;
    let settings = BackfillConfig::from_args(args, &|name: &str| std::env::var(name).ok())?;
    let source = config
        .paxeer
        .clone()
        .ok_or_else(|| IndexError::Config("backfill needs LAYERX_INDEXER_EVM_URL".to_owned()))?;
    let store = Store::open(&config.database)?;
    store.register_assets(&config.pointers)?;
    let node = PaxeerIngester::new(
        source.evm,
        source.comet,
        load_registry(&config)?,
        source.policy,
        source.start_block,
        source.chain_id,
        source.encoding,
    );
    let plan = Backfill::new(&node, settings.cutover, settings.range_blocks);
    plan.next_height(&store)?;
    let mut paxscan = PaxscanDatabase::connect(&settings.paxscan_url, &settings.paxscan_tls)?;
    let cutover = plan.run(&store, |from, to| paxscan.range(from, to))?;
    eprintln!(
        "layerx-indexer backfill reached the cutover {cutover}; live ingestion resumes at {}",
        cutover + 1
    );
    Ok(())
}

fn run() -> Result<(), IndexError> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some((mode, rest)) = args.split_first() {
        return match mode.as_str() {
            "backfill" => backfill(rest),
            other => Err(IndexError::Config(format!("unknown mode {other}"))),
        };
    }
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
    if let Some(source) = config.layerx.clone() {
        let ingester = LayerXIngester::new(source.relay, source.policy, source.start_batch);
        follow("layerx", Arc::clone(&store), config.poll, move |store| {
            ingester.step(store)
        });
    }
    if let Some(source) = config.paxeer.clone() {
        let registry = load_registry(&config)?;
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
