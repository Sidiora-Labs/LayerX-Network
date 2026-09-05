//! The `layerx-internal` services the hosted developer plane dials: the
//! Ed25519 signing boundary behind `kms.layerx-internal.svc` and the durable
//! event sources behind `journeys`, `payments`, `approvals` and
//! `programs.layerx-internal.svc`.
//!
//! Both services follow the hosted house style: a blocking TLS listener, a
//! bounded hand-rolled HTTP/1.1 parser, bearer tokens read from files, exact
//! JSON bodies the consumers parse with `deny_unknown_fields`, and readiness
//! that is answered only from the real dependency behind the service.

pub mod base64;
pub mod events;
pub mod http;
pub mod journal;
pub mod kms;
pub mod seal;
pub mod secret;
pub mod tls;

/// Names the contract this crate implements.
#[must_use]
pub fn platform_internal() -> &'static str {
    "layerx-internal-signing-boundary-and-verified-event-sources"
}
