//! Developer dashboards over the hosted `LayerX` surfaces.
//!
//! The dashboard is a read model, not an authority: it opens the hosted
//! gateway's durable store and the hosted webhook store for reading and
//! projects what they already recorded onto the vocabulary the human plane
//! displays. Keys, quotas, usage and the request log come from the gateway;
//! endpoint health, delivery logs, the dead-letter path and verified payment
//! receipts come from the webhook event store and its principal scope.
//!
//! Every protocol fact carries the verification level its evidence established,
//! and no payment is presented as settled on anything weaker than a verified
//! `LayerX` receipt. The surface is JSON: the client experience is rendered by
//! the human plane's own component library rather than by markup emitted here.

pub mod error;
pub mod gateway;
pub mod model;
pub mod service;

pub use error::DashboardError;
pub use gateway::{Snapshot, Store};
pub use model::{
    DeliverySummary, KeyView, Overview, PaymentView, ReceiptView, RequestOutcome, RequestRecord,
    RequestSummary, UsageSummary,
};
pub use service::Dashboard;

#[must_use]
pub fn platform_dashboard() -> &'static str {
    "read-only-developer-dashboards-over-keys-usage-requests-receipts-and-webhook-delivery"
}
