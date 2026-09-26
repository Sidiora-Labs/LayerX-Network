use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;
use x_websearch::{KeyFiles, Limits, RouteTable, Server};

fn config_path(arguments: impl IntoIterator<Item = OsString>) -> Option<PathBuf> {
    let mut arguments = arguments.into_iter();
    let flag = arguments.next()?;
    let path = arguments.next()?;
    (flag == "--config" && !path.is_empty() && arguments.next().is_none())
        .then(|| PathBuf::from(path))
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
    let server = match Server::bind(config.listen, Limits::default(), RouteTable::new()) {
        Ok(server) => server,
        Err(error) => {
            eprintln!("x-websearch refused startup: {error}");
            return ExitCode::FAILURE;
        }
    };
    let result = server.run();
    drop(keys);
    match result {
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
}
