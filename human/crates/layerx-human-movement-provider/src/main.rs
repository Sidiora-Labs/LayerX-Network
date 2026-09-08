#![forbid(unsafe_code)]

mod config;
mod execution;
mod journal;
mod listener;
mod planning;
mod producer;
mod service;

use config::Config;
use journal::Journal;
use listener::Listener;
use service::EvidenceService;

fn main() {
    if run().is_err() {
        eprintln!(
            "movement provider refused configuration or encountered an integrity/transport failure"
        );
        std::process::exit(1);
    }
}

fn run() -> Result<(), Error> {
    let config = Config::from_environment()?;
    let journal = Journal::open(&config.state_root, config.listener.protocol)?;
    let mut service = EvidenceService::new(&config, journal)?;
    let listener = Listener::bind(config.listener)?;
    eprintln!(
        "movement provider serving; readiness requires complete on-chain witness publication"
    );
    loop {
        listener.serve_next(&mut service)?;
    }
}

#[derive(Debug)]
enum Error {
    Configuration,
    Integrity,
    Conflict,
    Capacity,
    Io(std::io::Error),
}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "movement provider IO failure: {}", error.kind()),
            Self::Configuration => f.write_str("movement provider configuration refused"),
            Self::Integrity => f.write_str("movement provider integrity failure"),
            Self::Conflict => f.write_str("movement provider action conflict"),
            Self::Capacity => f.write_str("movement provider capacity exceeded"),
        }
    }
}
impl std::error::Error for Error {}

#[cfg(test)]
mod tests;
