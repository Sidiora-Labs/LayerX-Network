#![forbid(unsafe_code)]

pub mod buyer;
mod codec;
pub mod facilitator;
pub mod model;
pub mod seller;
pub mod transport;

pub use buyer::Buyer;
pub use facilitator::Facilitator;
pub use model::{PaymentPayload, PaymentRequired, PaymentRequirements, SettlementResponse};
pub use seller::Seller;

use layerx_interop_gateway::adapter::{
    AdapterDescriptor, AdapterId, ConformanceSuite, PinnedSpec, SpecVersion,
};

use crate::model::X402Error;

/// Exact upstream x402 v2 specification revision used by this adapter.
pub const X402_SPEC_COMMIT: &str = "7d5363a6d51750dc246041f2b0ed5819dd46a0d7";
/// SHA-256 of `specs/x402-specification-v2.md` at [`X402_SPEC_COMMIT`].
pub const X402_SPEC_SHA256: [u8; 32] = [
    0x7d, 0x9b, 0xe6, 0x6c, 0xbc, 0xf5, 0x1d, 0x35, 0x93, 0xe1, 0x7a, 0xc5, 0x1a, 0x62, 0x33, 0x95,
    0xf8, 0xcc, 0xb8, 0x6f, 0xd3, 0xd7, 0x6a, 0x27, 0x91, 0x94, 0x19, 0xe4, 0xce, 0x83, 0xef, 0xef,
];
const X402_SPEC_DOCUMENT: &[u8] =
    include_bytes!("../../../specs/vendor/x402/x402-specification-v2.md");

/// Builds the gateway descriptor for this exact upstream specification and a
/// caller-supplied real conformance suite. The adapter does not embed a
/// synthetic suite digest when upstream vectors have not been imported.
///
/// # Errors
///
/// Returns a declaration refusal if the fixed identifiers or version cannot
/// satisfy the gateway's registration contract.
pub fn x402_adapter_descriptor(
    conformance: ConformanceSuite,
) -> Result<AdapterDescriptor, X402Error> {
    let id = AdapterId::new("x402").map_err(|error| X402Error::Gateway(error.into()))?;
    let protocol = AdapterId::new("x402").map_err(|error| X402Error::Gateway(error.into()))?;
    let version = SpecVersion::parse("2.0.0").map_err(|error| X402Error::Gateway(error.into()))?;
    let spec = PinnedSpec::new(protocol, version, X402_SPEC_SHA256)
        .map_err(|error| X402Error::Gateway(error.into()))?;
    spec.verify_document(X402_SPEC_DOCUMENT)
        .map_err(|error| X402Error::Gateway(error.into()))?;
    Ok(AdapterDescriptor::new(id, spec, conformance))
}

/// Codify anchor for the production x402 v2 seller role.
#[must_use]
pub const fn interop_x402_seller() -> &'static str {
    "x402-v2-receipt-verified-seller"
}

/// Codify anchor for the production x402 v2 buyer role.
#[must_use]
pub const fn interop_x402_buyer() -> &'static str {
    "x402-v2-evidence-bound-buyer"
}

/// Codify anchor for the receipt-backed facilitator and transport matrix.
#[must_use]
pub const fn interop_x402_facilitator() -> &'static str {
    "x402-v2-receipt-backed-facilitator-http-mcp-a2a"
}
