use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::iterator::{Handle, Signals};
use x_websearch::attest::{self, Attestor, SignatureExchange};
use x_websearch::content::{self, ContentStore};
use x_websearch::crawl::{CrawlBudget, CrawlReport, Crawler, StopSignal};
use x_websearch::fetch::Fetcher;
use x_websearch::index::WebIndex;
use x_websearch::payment::{system_clock, PaymentGate};
use x_websearch::server::Stopper;
use x_websearch::submit::{self, Outcome, Submitter};
use x_websearch::watch::{EvmRpc, RequestWatcher};
use x_websearch::{search, Config, KeyFiles, Keys, Limits, RouteTable, Server};

/// How long after the start of one attestation round the next one starts.
const ATTEST_INTERVAL: Duration = Duration::from_secs(2);

/// The attestor's loops: the request watcher, the attestor, the signature
/// exchange and, with a submitter key, the fulfil submitter.
struct Pipeline {
    rpc: EvmRpc,
    watcher: RequestWatcher,
    attestor: Attestor,
    exchange: Arc<SignatureExchange>,
    submitter: Option<Submitter>,
}

fn config_path(arguments: impl IntoIterator<Item = OsString>) -> Option<PathBuf> {
    let mut arguments = arguments.into_iter();
    let flag = arguments.next()?;
    let path = arguments.next()?;
    (flag == "--config" && !path.is_empty() && arguments.next().is_none())
        .then(|| PathBuf::from(path))
}

/// What the configuration and the keys assemble into: the routes, the
/// crawler, the index it writes and the attestor's loops.
struct Assembled {
    routes: RouteTable,
    crawler: Crawler,
    index: Arc<WebIndex>,
    pipeline: Option<Pipeline>,
}

/// The routes and the crawler built from the configuration and the keys.
/// Every refusal names the configuration field or key it concerns.
fn assemble(config: &Config, keys: &Keys) -> Result<Assembled, String> {
    let conformance = x_websearch::conformance_suite()
        .map_err(|error| format!("payment conformance suite: {error}"))?;
    let gate = Arc::new(
        PaymentGate::new(config, keys.receiver(), conformance, system_clock)
            .map_err(|error| format!("assets, gateway or data_dir: {error}"))?,
    );
    let store = Arc::new(
        ContentStore::open(&config.data_dir, &config.peers)
            .map_err(|error| format!("peers or data_dir: {error}"))?,
    );
    let index =
        Arc::new(WebIndex::open(&config.data_dir).map_err(|error| format!("data_dir: {error}"))?);
    let fetcher = Arc::new(Fetcher::new(config.fetch).map_err(|error| format!("fetch: {error}"))?);
    let crawler = Crawler::new(
        CrawlBudget::from_config(&config.crawl),
        Arc::clone(&fetcher),
        Arc::clone(&index),
    )
    .map_err(|error| format!("crawl: {error}"))?;
    let mut routes = RouteTable::new();
    search::register(&mut routes, &gate, &index, &store)
        .map_err(|error| format!("route /search: {error}"))?;
    content::register(&mut routes, &gate, &fetcher, &store)
        .map_err(|error| format!("route /fetch or /content: {error}"))?;
    let pipeline = pipeline(config, keys, &mut routes, &fetcher, &index, &store)?;
    Ok(Assembled {
        routes,
        crawler,
        index,
        pipeline,
    })
}

/// The attestor's loops when an attestor key is present, with the
/// signature-exchange route registered; none without one.
fn pipeline(
    config: &Config,
    keys: &Keys,
    routes: &mut RouteTable,
    fetcher: &Arc<Fetcher>,
    index: &Arc<WebIndex>,
    store: &Arc<ContentStore>,
) -> Result<Option<Pipeline>, String> {
    let Some(attestor_key) = keys.attestor() else {
        return Ok(None);
    };
    let rpc =
        EvmRpc::new(&config.evm.endpoint).map_err(|error| format!("evm.endpoint: {error}"))?;
    let watcher = RequestWatcher::open(
        rpc.clone(),
        config.evm.confirmations,
        &config.data_dir.join("watch"),
        None,
    )
    .map_err(|error| format!("data_dir: {error}"))?;
    let exchange = Arc::new(
        SignatureExchange::open(&config.data_dir.join("attest"), &config.peers)
            .map_err(|error| format!("peers or data_dir: {error}"))?,
    );
    attest::register(routes, &exchange).map_err(|error| format!("route /attestations: {error}"))?;
    let attestor = Attestor::new(
        attestor_key.clone(),
        config.evm.chain_id,
        Arc::clone(fetcher),
        Arc::clone(index),
        Arc::clone(store),
    );
    let submitter = keys
        .submitter()
        .map(|key| {
            Submitter::open(
                rpc.clone(),
                key.clone(),
                config.evm.chain_id,
                &config.data_dir.join("submit"),
            )
        })
        .transpose()
        .map_err(|error| format!("data_dir: {error}"))?;
    Ok(Some(Pipeline {
        rpc,
        watcher,
        attestor,
        exchange,
        submitter,
    }))
}

/// One attestation round: answer the newly confirmed requests, drop the
/// timed-out ones, exchange signatures and submit or settle each request.
fn attest_round(pipeline: &mut Pipeline) {
    match pipeline.watcher.poll() {
        Ok(requests) => {
            for request in requests {
                match pipeline.attestor.attest(&request) {
                    Ok(answer) => pipeline.exchange.record(answer),
                    Err(error) => eprintln!(
                        "x-websearch could not answer request {}: {error}",
                        request.request_id
                    ),
                }
            }
        }
        Err(error) => eprintln!("x-websearch request watch failed: {error}"),
    }
    pipeline.exchange.expire(pipeline.watcher.last_head());
    let set = match submit::attestor_set(&pipeline.rpc) {
        Ok(set) => set,
        Err(error) => {
            eprintln!("x-websearch could not read the attestor set: {error}");
            return;
        }
    };
    for request_id in pipeline.exchange.pending() {
        let held = pipeline.exchange.collect(request_id, &set);
        let settled = match &pipeline.submitter {
            Some(submitter) => settle(submitter, &pipeline.exchange, request_id, &set),
            None => submit::request_status(&pipeline.rpc, request_id)
                .map(|status| status != submit::STATUS_PENDING)
                .map_err(|error| error.to_string()),
        };
        match settled {
            Ok(true) => pipeline.exchange.forget(request_id),
            Ok(false) => {}
            Err(error) => {
                eprintln!("x-websearch request {request_id} with {held} signatures: {error}");
            }
        }
    }
}

/// Confirms a journalled fulfil, or submits one once the threshold is
/// reached. `true` means the request needs nothing more.
fn settle(
    submitter: &Submitter,
    exchange: &SignatureExchange,
    request_id: u64,
    set: &attest::AttestorSet,
) -> Result<bool, String> {
    if let Some(outcome) = submitter
        .confirm(request_id)
        .map_err(|error| error.to_string())?
    {
        if outcome.completed() {
            return Ok(true);
        }
    }
    let Some(ready) = exchange.ready(request_id, set) else {
        return Ok(false);
    };
    submitter
        .submit(&ready)
        .map(Outcome::completed)
        .map_err(|error| error.to_string())
}

/// Rebroadcasts the journalled fulfilments, then runs an attestation round
/// every [`ATTEST_INTERVAL`] on its own thread until `stop` is raised.
fn start_attestor(mut pipeline: Pipeline, stop: StopSignal) -> std::io::Result<JoinHandle<()>> {
    thread::Builder::new()
        .name("x-websearch-attestor".to_owned())
        .spawn(move || {
            if let Some(submitter) = &pipeline.submitter {
                match submitter.resume() {
                    Ok(results) => {
                        for (request_id, result) in results {
                            if let Err(error) = result {
                                eprintln!("x-websearch rebroadcast of {request_id}: {error}");
                            }
                        }
                    }
                    Err(error) => eprintln!("x-websearch journal unreadable: {error}"),
                }
            }
            while !stop.is_raised() {
                let started = Instant::now();
                attest_round(&mut pipeline);
                if stop.wait_timeout(ATTEST_INTERVAL.saturating_sub(started.elapsed())) {
                    break;
                }
            }
        })
}

fn describe(report: &CrawlReport) -> String {
    format!(
        "x-websearch crawl cycle finished: {} visited, {} indexed, {} deferred",
        report.pages.len(),
        report.indexed().count(),
        report.deferred
    )
}

/// Runs a crawl cycle over the seeds every `interval`, the first at once,
/// on its own thread until `stop` is raised.
fn start_crawler(
    mut crawler: Crawler,
    seeds: Vec<String>,
    interval: Duration,
    stop: StopSignal,
) -> std::io::Result<JoinHandle<()>> {
    thread::Builder::new()
        .name("x-websearch-crawler".to_owned())
        .spawn(move || {
            crawler.run_every(&seeds, interval, &stop, |result| match result {
                Ok(report) => eprintln!("{}", describe(&report)),
                Err(error) => eprintln!("x-websearch crawl cycle failed: {error}"),
            });
        })
}

/// Raises `stop` and stops the server on the first SIGTERM or SIGINT, on
/// its own thread. Every other signal keeps its default action.
fn start_signals(
    mut signals: Signals,
    stop: StopSignal,
    server: Stopper,
) -> std::io::Result<JoinHandle<Option<i32>>> {
    thread::Builder::new()
        .name("x-websearch-signals".to_owned())
        .spawn(move || {
            let received = signals.forever().next();
            stop.raise();
            server.stop();
            received
        })
}

fn signal_name(signal: i32) -> &'static str {
    match signal {
        SIGTERM => "SIGTERM",
        SIGINT => "SIGINT",
        _ => "a signal",
    }
}

/// Stops the loops, waits for them, and commits the index. `true` when
/// every step succeeded.
fn wind_down(
    stop: &StopSignal,
    signals: &Handle,
    watcher: JoinHandle<Option<i32>>,
    loops: Vec<JoinHandle<()>>,
    index: &WebIndex,
) -> bool {
    stop.raise();
    signals.close();
    let mut clean = true;
    match watcher.join() {
        Ok(Some(signal)) => eprintln!("x-websearch stopping on {}", signal_name(signal)),
        Ok(None) => {}
        Err(_) => {
            eprintln!("x-websearch stopped: the signal thread panicked");
            clean = false;
        }
    }
    for handle in loops {
        if handle.join().is_err() {
            eprintln!("x-websearch stopped: a sidecar thread panicked");
            clean = false;
        }
    }
    if let Err(error) = index.commit() {
        eprintln!("x-websearch stopped: index commit failed: {error}");
        clean = false;
    }
    clean
}

fn main() -> ExitCode {
    let Some(path) = config_path(std::env::args_os().skip(1)) else {
        eprintln!("Paxeer X Network web search sidecar");
        eprintln!("usage: x-websearch --config PATH");
        return ExitCode::from(2);
    };
    let config = match x_websearch::load(&path) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    let keys = match KeyFiles::from_env().and_then(|files| files.load()) {
        Ok(keys) => keys,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    let assembled = assemble(&config, &keys);
    drop(keys);
    let Assembled {
        routes,
        crawler,
        index,
        pipeline,
    } = match assembled {
        Ok(assembled) => assembled,
        Err(error) => {
            eprintln!("x-websearch refused startup: {error}");
            return ExitCode::from(2);
        }
    };
    let signals = match Signals::new([SIGTERM, SIGINT]) {
        Ok(signals) => signals,
        Err(error) => {
            eprintln!("x-websearch refused startup: signal handling: {error}");
            return ExitCode::FAILURE;
        }
    };
    let server =
        match Server::bind(config.listen, Limits::default(), routes).and_then(Server::spawn) {
            Ok(server) => server,
            Err(error) => {
                eprintln!("x-websearch refused startup: listen: {error}");
                return ExitCode::FAILURE;
            }
        };
    let stop = StopSignal::new();
    let handle = signals.handle();
    let watcher = match start_signals(signals, stop.clone(), server.stopper()) {
        Ok(watcher) => watcher,
        Err(error) => {
            eprintln!("x-websearch refused startup: signal thread: {error}");
            drop(server.shutdown());
            return ExitCode::FAILURE;
        }
    };
    let (loops, started) = start_loops(
        crawler,
        pipeline,
        config.seeds.clone(),
        config.crawl_interval(),
        &stop,
    );
    match &started {
        Ok(()) => eprintln!("x-websearch listening on {}", config.listen),
        Err(error) => {
            eprintln!("x-websearch refused startup: {error}");
            server.stopper().stop();
        }
    }
    let outcome = server.wait();
    if let Err(error) = &outcome {
        eprintln!("x-websearch stopped: {error}");
    }
    let clean = wind_down(&stop, &handle, watcher, loops, &index);
    if started.is_ok() && outcome.is_ok() && clean {
        eprintln!("x-websearch stopped cleanly");
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Starts the crawler and, with an attestor key, the attestor loop. The
/// handles of the threads that started come back with the first failure.
fn start_loops(
    crawler: Crawler,
    pipeline: Option<Pipeline>,
    seeds: Vec<String>,
    interval: Duration,
    stop: &StopSignal,
) -> (Vec<JoinHandle<()>>, Result<(), String>) {
    let mut loops = Vec::with_capacity(2);
    match start_crawler(crawler, seeds, interval, stop.clone()) {
        Ok(handle) => loops.push(handle),
        Err(error) => return (loops, Err(format!("crawler thread: {error}"))),
    }
    if let Some(pipeline) = pipeline {
        match start_attestor(pipeline, stop.clone()) {
            Ok(handle) => loops.push(handle),
            Err(error) => return (loops, Err(format!("attestor thread: {error}"))),
        }
    }
    (loops, Ok(()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_config_arguments_required() {
        assert_eq!(
            config_path(["--config".into(), "x-websearch.json".into()]),
            Some(PathBuf::from("x-websearch.json"))
        );
        for args in [
            vec![],
            vec!["--config"],
            vec!["--other", "x-websearch.json"],
            vec!["--config", ""],
            vec!["--config", "x-websearch.json", "extra"],
        ] {
            assert_eq!(config_path(args.into_iter().map(OsString::from)), None);
        }
    }

    #[test]
    fn a_settled_journal_entry_completes_the_request_before_any_call(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let scratch =
            std::env::temp_dir().join(format!("x-websearch-main-settle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        let mut secret = [0_u8; 32];
        secret[0] = 0x5a;
        secret[31] = 1;
        let key = k256::ecdsa::SigningKey::from_slice(&secret)?;
        let unreachable = EvmRpc::new("http://127.0.0.1:1")?;
        let submitter = Submitter::open(unreachable, key, 713_714, &scratch.join("submit"))?;
        submitter.journal().store(&submit::JournalEntry {
            request_id: 7,
            state: submit::JournalState::AlreadyFulfilled,
            transaction: None,
        })?;
        let exchange = SignatureExchange::open(&scratch.join("attest"), &[])?;
        let set = attest::AttestorSet::default();
        assert_eq!(settle(&submitter, &exchange, 7, &set), Ok(true));
        assert_eq!(settle(&submitter, &exchange, 8, &set), Ok(false));
        std::fs::remove_dir_all(&scratch)?;
        Ok(())
    }

    #[test]
    fn a_crawl_report_is_described_by_its_counts() {
        let report = CrawlReport {
            pages: Vec::new(),
            deferred: 3,
        };
        assert_eq!(
            describe(&report),
            "x-websearch crawl cycle finished: 0 visited, 0 indexed, 3 deferred"
        );
    }
}
