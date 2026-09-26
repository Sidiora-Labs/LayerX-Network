//! x-websearch, the Paxeer X Network web search sidecar.
//!
//! `config::load` reads the JSON configuration and refuses unknown fields,
//! missing fields, placeholders and key material, naming the field. Keys are
//! read only from the files `X_WEBSEARCH_ATTESTOR_KEY_FILE`,
//! `X_WEBSEARCH_SUBMITTER_KEY_FILE` and `X_WEBSEARCH_RECEIVER_KEY_FILE` name.
//! `server::Server` serves `GET /health` free, `GET /search` and `GET /fetch`
//! behind `payment::PaymentGate`, and `GET /content/<digest>` unpaid.

pub mod assets;
pub mod canonical;
pub mod config;
pub mod content;
pub mod crawl;
pub mod extract;
pub mod fetch;
pub mod index;
pub mod keys;
pub mod payment;
pub mod robots;
pub mod search;
pub mod server;

pub use config::{load, Config, ConfigError};
pub use keys::{KeyError, KeyFiles, Keys};
pub use server::{Limits, Request, Response, Route, RouteTable, RunningServer, Server};

use layerx_interop_gateway::adapter::{AdapterError, AdapterId, ConformanceSuite};

/// The conformance suite the sidecar's 402LXP seller is qualified against:
/// the recorded gateway exchange under `tests/fixtures/gateway`.
pub const CONFORMANCE_SUITE: &str = "x402-v2";

/// The number of recorded gateway answers in the suite.
pub const CONFORMANCE_VECTOR_COUNT: u64 = 30;

/// SHA-256 over the suite's recording files concatenated in file name order.
pub const CONFORMANCE_SUITE_DIGEST: [u8; 32] = [
    0xdc, 0xc7, 0x65, 0xb6, 0x7b, 0xfa, 0xab, 0x00, 0xc0, 0x07, 0x39, 0xfb, 0xf4, 0xa2, 0x5b, 0x98,
    0xc2, 0xa1, 0x4c, 0xb2, 0x40, 0x3a, 0x30, 0xb5, 0xfb, 0x64, 0xd4, 0xa2, 0x73, 0xde, 0x83, 0x79,
];

/// The pinned conformance suite the binary registers its x402 adapter with.
///
/// # Errors
/// Returns the gateway's refusal of the pinned identifier, count or digest.
pub fn conformance_suite() -> Result<ConformanceSuite, AdapterError> {
    ConformanceSuite::new(
        AdapterId::new(CONFORMANCE_SUITE)?,
        CONFORMANCE_VECTOR_COUNT,
        CONFORMANCE_SUITE_DIGEST,
    )
}
