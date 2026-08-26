#![forbid(unsafe_code)]

mod adapter;
mod error;
mod jose;
mod model;
mod portable;
mod verify;

pub use adapter::{
    authorize_payment, AdapterOutcome, Ap2Adapter, AuthorizedPayment, ExecutedPayment,
    LayerXAssetBinding, LayerXIntentPlane, PlaneOutcome, PortableLayerXEvidence, ReceiptSigner,
    SignedAp2Evidence,
};
pub use error::Ap2Error;
pub use jose::{KeyResolver, KeyUse, ProtectedHeader};
pub use model::{Merchant, PaymentAmount, PaymentInstrument};
pub use portable::{Ap2ExternalMandateVerifier, Ap2MandatePair, AP2_MANDATE_PAIR_MEDIA_TYPE};
pub use verify::{
    MandateMode, MandateUsage, MandateVerifier, VerificationContext, VerifiedMandates,
};

use layerx_interop_gateway::adapter::{
    AdapterDescriptor, AdapterId, ConformanceSuite, PinnedSpec, SpecVersion,
};

/// Exact official AP2 repository revision used by this adapter.
pub const AP2_SPEC_COMMIT: &str = "e1ea56db72a6385bce3e5c1112b3a56ce60acb43";
/// SHA-256 of `docs/ap2/specification.md` at [`AP2_SPEC_COMMIT`].
pub const AP2_SPEC_SHA256: [u8; 32] = [
    0x32, 0xc3, 0xbe, 0x50, 0x11, 0xf4, 0x81, 0xd2, 0x76, 0x0e, 0x56, 0xe7, 0xb9, 0x93, 0x5b, 0x08,
    0x42, 0xc3, 0xda, 0x0d, 0x5f, 0x7d, 0x7b, 0x8a, 0x68, 0x40, 0x2a, 0x59, 0x9f, 0x1e, 0x6a, 0xa3,
];

/// Builds the gateway descriptor for AP2 mandate schema version 1 and a real,
/// caller-supplied conformance suite. No synthetic vector digest is embedded.
///
/// # Errors
///
/// Returns a declaration refusal if a fixed identifier or pin is invalid.
pub fn ap2_adapter_descriptor(
    conformance: ConformanceSuite,
) -> Result<AdapterDescriptor, Ap2Error> {
    let id = AdapterId::new("ap2").map_err(|error| Ap2Error::Gateway(error.into()))?;
    let protocol = AdapterId::new("ap2").map_err(|error| Ap2Error::Gateway(error.into()))?;
    let version = SpecVersion::parse("1.0.0").map_err(|error| Ap2Error::Gateway(error.into()))?;
    let spec = PinnedSpec::new(protocol, version, AP2_SPEC_SHA256)
        .map_err(|error| Ap2Error::Gateway(error.into()))?;
    Ok(AdapterDescriptor::new(id, spec, conformance))
}

/// Codify anchor for the AP2 checkout/payment mandate adapter.
#[must_use]
pub const fn interop_ap2() -> &'static str {
    "ap2-v1-signature-constraint-receipt-verified"
}
