use layerx_gas_station::config::StationConfig;
use layerx_gas_station::signer::LocalSigner;
use layerx_gas_station::Station;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

fn config_path(arguments: impl IntoIterator<Item = OsString>) -> Option<PathBuf> {
    let mut arguments = arguments.into_iter();
    let flag = arguments.next()?;
    let path = arguments.next()?;
    (flag == "--config" && !path.is_empty() && arguments.next().is_none())
        .then(|| PathBuf::from(path))
}

fn main() -> ExitCode {
    let Some(path) = config_path(std::env::args_os().skip(1)) else {
        eprintln!("Paxeer X Network gas station initialization check");
        eprintln!("usage: paxeer-gas-station --config PATH");
        return ExitCode::from(2);
    };
    let config = match StationConfig::load(&path) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    let signer = match LocalSigner::from_config(&config) {
        Ok(signer) => signer,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    match Station::new(config, signer) {
        Ok(_) => match report_initialization(io::stdout().lock()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}

fn report_initialization(mut output: impl Write) -> io::Result<()> {
    writeln!(
        output,
        "Paxeer X Network gas station initialization check passed; no service was started."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn initialization_output_does_not_claim_service_startup() -> io::Result<()> {
        let mut output = Vec::new();
        report_initialization(&mut output)?;
        assert_eq!(
            output,
            b"Paxeer X Network gas station initialization check passed; no service was started.\n"
        );
        let mut full = [];
        assert_eq!(
            report_initialization(full.as_mut_slice())
                .err()
                .map(|e| e.kind()),
            Some(io::ErrorKind::WriteZero)
        );
        Ok(())
    }

    #[test]
    fn exact_config_arguments_required() {
        assert_eq!(
            config_path(["--config".into(), "station.json".into()]),
            Some(PathBuf::from("station.json"))
        );
        for args in [
            vec![],
            vec!["--config"],
            vec!["--other", "station.json"],
            vec!["--config", ""],
            vec!["--config", "station.json", "extra"],
        ] {
            assert_eq!(config_path(args.into_iter().map(OsString::from)), None);
        }
    }
}
