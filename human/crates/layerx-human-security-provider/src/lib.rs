mod state;
mod transport;

pub use state::{ingest_recovery_receipt, RecoveryReceipt, Store};
pub use transport::{serve, Config};

use std::io;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Refused,
    Corrupt,
    Configuration,
    Io(io::Error),
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Refused => "operation refused",
            Self::Corrupt => "security state is inconsistent",
            Self::Configuration => "invalid security provider configuration",
            Self::Io(_) => "security provider I/O failure",
        })
    }
}
impl std::error::Error for Error {}

fn text(bytes: &[u8]) -> Result<&str> {
    let value = std::str::from_utf8(bytes).map_err(|_| Error::Refused)?;
    if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        return Err(Error::Refused);
    }
    Ok(value)
}
fn number(bytes: &[u8]) -> Result<u64> {
    Ok(u64::from_be_bytes(
        bytes.try_into().map_err(|_| Error::Refused)?,
    ))
}
