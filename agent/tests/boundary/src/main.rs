mod cases;
mod gaps;
mod production;

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut arguments = std::env::args_os();
    let _program = arguments.next();
    let Some(node) = arguments.next().map(PathBuf::from) else {
        eprintln!("missing real layerxd executable path");
        return ExitCode::FAILURE;
    };
    let Some(repository) = arguments.next().map(PathBuf::from) else {
        eprintln!("missing repository root");
        return ExitCode::FAILURE;
    };
    if arguments.next().is_some() {
        eprintln!("unexpected boundary-suite argument");
        return ExitCode::FAILURE;
    }
    match cases::agent_boundary_conformance_suite(&node, &repository) {
        Ok(report) => {
            println!("{report}");
            match production::run_if_configured() {
                Ok(Some(production)) => println!("{production}"),
                Ok(None) => {}
                Err(error) => {
                    eprintln!("production boundary qualification failed: {error}");
                    return ExitCode::FAILURE;
                }
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("boundary conformance failed: {error}");
            ExitCode::FAILURE
        }
    }
}
