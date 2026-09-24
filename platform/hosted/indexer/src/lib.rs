//! `layerx-indexer`: a durable, decoded history of both halves of the network.
//!
//! The LayerX half follows a relay/archive node's public synchronization and
//! history protocol and decodes every canonical receipt with the frozen
//! `layerx-wire` decoder into both the 15-field activity receipt and the
//! 21-field 402LXP receipt. The Paxeer half walks EVM blocks, receipts and
//! logs plus CometBFT `tx_search` pages, decoding ERC-20 and pointer
//! transfers, native value transfers, bank and tokenfactory typed events,
//! LayerX anchor string events and every precompile ABI event found in the
//! configured ABI directory. Everything lands in one SQLite file with
//! checkpointed cursors and reorg rollback to a configured finality depth.

pub mod abi;
pub mod api;
pub mod backfill;
pub mod blockscout;
pub mod codec;
pub mod config;
pub mod follow;
pub mod layerx;
pub mod paxeer;
pub mod paxscan;
pub mod store;
pub mod transport;

use std::fmt;

/// Every way indexing can fail.
#[derive(Debug)]
pub enum IndexError {
    /// A source answered with something the indexer cannot decode.
    Decode(String),
    /// A source could not be reached or refused the request.
    Source(String),
    /// The durable store failed.
    Store(String),
    /// The configuration is incomplete or invalid.
    Config(String),
    /// A reorganisation reaches below the finality window; indexing stops
    /// rather than rewrite history the configuration declared final.
    ReorgBeyondFinality { source: String, position: u64 },
    /// Two sources or two records disagree about the same fact.
    Integrity(String),
}

impl fmt::Display for IndexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decode(detail) => write!(formatter, "decode failed: {detail}"),
            Self::Source(detail) => write!(formatter, "source failed: {detail}"),
            Self::Store(detail) => write!(formatter, "store failed: {detail}"),
            Self::Config(detail) => write!(formatter, "configuration invalid: {detail}"),
            Self::ReorgBeyondFinality { source, position } => write!(
                formatter,
                "{source} reorganised below the finality window at {position}"
            ),
            Self::Integrity(detail) => write!(formatter, "integrity violation: {detail}"),
        }
    }
}

impl std::error::Error for IndexError {}

impl From<rusqlite::Error> for IndexError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Store(error.to_string())
    }
}
