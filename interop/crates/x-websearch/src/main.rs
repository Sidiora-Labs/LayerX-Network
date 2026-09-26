use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use x_websearch::content::{self, ContentStore};
use x_websearch::crawl::{CrawlBudget, CrawlReport, Crawler};
use x_websearch::fetch::Fetcher;
use x_websearch::index::WebIndex;
use x_websearch::payment::{system_clock, PaymentGate};
use x_websearch::{search, Config, KeyFiles, Keys, Limits, RouteTable, Server};

/// How long after the start of one crawl cycle the next one starts. The
/// first cycle starts when the sidecar does.
const CRAWL_INTERVAL: Duration = Duration::from_secs(900);

fn config_path(arguments: impl IntoIterator<Item = OsString>) -> Option<PathBuf> {
    let mut arguments = arguments.into_iter();
    let flag = arguments.next()?;
    let path = arguments.next()?;
    (flag == "--config" && !path.is_empty() && arguments.next().is_none())
        .then(|| PathBuf::from(path))
}

/// The routes and the crawler built from the configuration and the keys.
/// Every refusal names the configuration field or key it concerns.
fn assemble(config: &Config, keys: &Keys) -> Result<(RouteTable, Crawler), String> {
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
    Ok((routes, crawler))
}

fn describe(report: &CrawlReport) -> String {
    format!(
        "x-websearch crawl cycle finished: {} visited, {} indexed, {} deferred",
        report.pages.len(),
        report.indexed().count(),
        report.deferred
    )
}

/// Runs a crawl cycle over the seeds every [`CRAWL_INTERVAL`], the first at
/// once, on its own thread.
fn start_crawler(mut crawler: Crawler, seeds: Vec<String>) -> std::io::Result<()> {
    thread::Builder::new()
        .name("x-websearch-crawler".to_owned())
        .spawn(move || loop {
            let started = Instant::now();
            match crawler.run_cycle(&seeds) {
                Ok(report) => eprintln!("{}", describe(&report)),
                Err(error) => eprintln!("x-websearch crawl cycle failed: {error}"),
            }
            thread::sleep(CRAWL_INTERVAL.saturating_sub(started.elapsed()));
        })
        .map(drop)
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
    let (routes, crawler) = match assembled {
        Ok(assembled) => assembled,
        Err(error) => {
            eprintln!("x-websearch refused startup: {error}");
            return ExitCode::from(2);
        }
    };
    let server = match Server::bind(config.listen, Limits::default(), routes) {
        Ok(server) => server,
        Err(error) => {
            eprintln!("x-websearch refused startup: listen: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = start_crawler(crawler, config.seeds) {
        eprintln!("x-websearch refused startup: crawler thread: {error}");
        return ExitCode::FAILURE;
    }
    eprintln!("x-websearch listening on {}", config.listen);
    match server.run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("x-websearch stopped: {error}");
            ExitCode::FAILURE
        }
    }
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
