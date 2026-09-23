//! `layerx-bridge-relayer --config PATH`

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use layerx_bridge_relayer::config::RelayerConfig;
use layerx_bridge_relayer::relayer::Relayer;

fn config_path() -> Option<PathBuf> {
    let mut arguments = std::env::args_os().skip(1);
    let flag = arguments.next()?;
    let path = arguments.next()?;
    (flag == "--config" && arguments.next().is_none()).then(|| PathBuf::from(path))
}

fn main() -> ExitCode {
    let Some(path) = config_path() else {
        eprintln!("usage: layerx-bridge-relayer --config PATH");
        return ExitCode::from(2);
    };
    let config = match RelayerConfig::load(&path) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("layerx-bridge-relayer: {error}");
            return ExitCode::from(2);
        }
    };
    let mut relayer = match config.build().and_then(Relayer::new) {
        Ok(relayer) => relayer,
        Err(error) => {
            eprintln!("layerx-bridge-relayer: startup refused: {error}");
            return ExitCode::FAILURE;
        }
    };
    let interval = Duration::from_millis(config.poll_interval_ms);
    loop {
        for (stream, result) in relayer.tick() {
            match result {
                Ok(report) if report == Default::default() => {}
                Ok(report) => eprintln!("layerx-bridge-relayer: {stream}: {report:?}"),
                Err(error) => eprintln!("layerx-bridge-relayer: {stream}: {error}"),
            }
        }
        std::thread::sleep(interval);
    }
}
