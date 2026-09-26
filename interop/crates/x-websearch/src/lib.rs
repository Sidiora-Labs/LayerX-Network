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
    0xf3, 0x3f, 0x7a, 0x4f, 0xa1, 0x73, 0xb2, 0xe3, 0xd3, 0x31, 0x6f, 0x3c, 0xf2, 0x9b, 0xd7, 0xb7,
    0x38, 0x41, 0x3a, 0x8d, 0x3e, 0xa1, 0x96, 0x4c, 0xf4, 0x26, 0xbb, 0xcb, 0x1b, 0xb7, 0x51, 0xfe,
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
