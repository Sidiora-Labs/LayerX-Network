use layerx_gas_station::config::ServiceConfig;
use layerx_gas_station::journal::Journal;
use layerx_gas_station::price::PaymasterRateSource;
use layerx_gas_station::rpc::{ConfiguredRpc, HttpsExchange};
use layerx_gas_station::service::{serve, Limits, Service};
use layerx_gas_station::signer::LocalSigner;
use layerx_gas_station::station::{GasStation, StationError};
use std::ffi::OsString;
use std::io::{self, Write};
use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Eq, PartialEq)]
struct Arguments {
    config: PathBuf,
    journal: PathBuf,
}

fn arguments(arguments: impl IntoIterator<Item = OsString>) -> Option<Arguments> {
    let mut arguments = arguments.into_iter();
    let (config_flag, config) = (arguments.next()?, arguments.next()?);
    let (journal_flag, journal) = (arguments.next()?, arguments.next()?);
    (config_flag == "--config"
        && journal_flag == "--journal"
        && !config.is_empty()
        && !journal.is_empty()
        && arguments.next().is_none())
    .then(|| Arguments {
        config: PathBuf::from(config),
        journal: PathBuf::from(journal),
    })
}

fn unix_time() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|elapsed| elapsed.as_secs())
}

fn main() -> ExitCode {
    let Some(arguments) = arguments(std::env::args_os().skip(1)) else {
        eprintln!("Paxeer X Network gas station");
        eprintln!("usage: paxeer-gas-station --config PATH --journal PATH");
        return ExitCode::from(2);
    };
    let config = match ServiceConfig::load(&arguments.config) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    let signer = match LocalSigner::from_config(&config.station) {
        Ok(signer) => signer,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let (rpc, rates) = match (
        ConfiguredRpc::new(&config.station, HttpsExchange),
        ConfiguredRpc::new(&config.station, HttpsExchange),
    ) {
        (Ok(rpc), Ok(rates)) => (
            rpc,
            PaymasterRateSource::new(rates, config.station.paymaster),
        ),
        (Err(error), _) | (_, Err(error)) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    let journal = match Journal::open(&arguments.journal) {
        Ok(journal) => journal,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let station = match GasStation::new(config.station.clone(), signer, rpc, rates, journal) {
        Ok(station) => station,
        Err(error @ StationError::Invalid) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let Ok(listener) = TcpListener::bind(config.listen) else {
        eprintln!("listen address unavailable");
        return ExitCode::FAILURE;
    };
    let mut service = Service::new(&config, station, unix_time, Limits::default());
    if let Err(error) = report_listening(io::stdout().lock(), config.listen) {
        eprintln!("{error}");
        return ExitCode::FAILURE;
    }
    match serve(&listener, &mut service, &mut io::stderr().lock()) {
        Ok(never) => match never {},
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn report_listening(mut output: impl Write, listen: SocketAddr) -> io::Result<()> {
    writeln!(
        output,
        "Paxeer X Network gas station serving POST /quote and POST /submit on {listen}"
    )?;
    output.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listening_report_names_the_endpoints_and_address() -> io::Result<()> {
        let mut output = Vec::new();
        report_listening(&mut output, SocketAddr::from(([127, 0, 0, 1], 8545)))?;
        assert_eq!(
            output,
            b"Paxeer X Network gas station serving POST /quote and POST /submit on 127.0.0.1:8545\n"
        );
        let mut full = [];
        assert_eq!(
            report_listening(
                full.as_mut_slice(),
                SocketAddr::from(([127, 0, 0, 1], 8545))
            )
            .err()
            .map(|e| e.kind()),
            Some(io::ErrorKind::WriteZero)
        );
        Ok(())
    }

    #[test]
    fn exact_config_and_journal_arguments_required() {
        assert_eq!(
            arguments(
                [
                    "--config",
                    "station.json",
                    "--journal",
                    "state/sponsorship.jsonl"
                ]
                .map(OsString::from)
            ),
            Some(Arguments {
                config: PathBuf::from("station.json"),
                journal: PathBuf::from("state/sponsorship.jsonl"),
            })
        );
        for args in [
            vec![],
            vec!["--config"],
            vec!["--config", "station.json"],
            vec!["--config", "station.json", "--journal"],
            vec!["--journal", "state.jsonl", "--config", "station.json"],
            vec!["--other", "station.json", "--journal", "state.jsonl"],
            vec!["--config", "", "--journal", "state.jsonl"],
            vec!["--config", "station.json", "--journal", ""],
            vec![
                "--config",
                "station.json",
                "--journal",
                "state.jsonl",
                "extra",
            ],
        ] {
            assert_eq!(arguments(args.into_iter().map(OsString::from)), None);
        }
    }

    #[test]
    fn clock_reads_unix_seconds() {
        let before = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .ok();
        let now = unix_time();
        assert!(now.is_some());
        assert!(now >= before);
    }
}
