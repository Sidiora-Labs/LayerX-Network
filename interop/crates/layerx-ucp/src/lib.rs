#![forbid(unsafe_code)]

use std::fmt::{Display, Formatter};

use layerx_interop_gateway::adapter::{AdapterDescriptor, AdapterId, ConformanceSuite, PinnedSpec};
use layerx_interop_gateway::error::GatewayError;
use layerx_interop_gateway::gateway::{TranslationKind, TranslationRequest, TranslationStatus};
use layerx_interop_gateway::principal::PrincipalId;
use layerx_interop_gateway::trace::{TraceId, Traced};
use layerx_interop_gateway::GatewayCore;
use layerx_proof::merkle::leaf_hash;
use layerx_proof::receipt::{verify, AuthorizedBatch};
use sha2::{Digest as _, Sha256};

const ADAPTER_ID: &str = "ucp";
const UCP_VERSION: &str = "2026-04-08";
const CHECKOUT_CAPABILITY: &str = "dev.ucp.shopping.checkout";
const ORDER_CAPABILITY: &str = "dev.ucp.shopping.order";
const CHECKOUT_SPEC: &str = "https://ucp.dev/2026-04-08/specification/checkout";
const CHECKOUT_SCHEMA: &str = "https://ucp.dev/2026-04-08/schemas/shopping/checkout.json";
const ORDER_SPEC: &str = "https://ucp.dev/2026-04-08/specification/order";
const ORDER_SCHEMA: &str = "https://ucp.dev/2026-04-08/schemas/shopping/order.json";
const REST_SCHEMA: &str = "https://ucp.dev/2026-04-08/services/shopping/rest.openapi.json";
const VALUE_LIMIT: usize = 256;
const REQUEST_DOMAIN: &[u8] = b"LayerX/interop/ucp/checkout/v1\0";
const IDEMPOTENCY_DOMAIN: &[u8] = b"LayerX/interop/ucp/idempotency/v1\0";

/// SHA-256 of the published checkout specification page at [`CHECKOUT_SPEC`]
/// (canonical trailing-slash URL), UCP revision `2026-04-08`.
pub const UCP_CHECKOUT_SPEC_SHA256: [u8; 32] = [
    0xa5, 0x79, 0xdf, 0x10, 0xae, 0x58, 0x9a, 0x63, 0xf8, 0xf0, 0xd0, 0x1e, 0x7b, 0x4f, 0x8e, 0x31,
    0x83, 0xba, 0x22, 0x70, 0x25, 0xc6, 0x92, 0xbb, 0x03, 0x85, 0xd9, 0x23, 0x62, 0xb8, 0x94, 0xb3,
];
/// SHA-256 of the published order specification page at [`ORDER_SPEC`]
/// (canonical trailing-slash URL), UCP revision `2026-04-08`.
pub const UCP_ORDER_SPEC_SHA256: [u8; 32] = [
    0x44, 0x63, 0xb5, 0x24, 0x58, 0x2c, 0xbd, 0xd5, 0xe5, 0xf1, 0xb8, 0x4d, 0x58, 0x70, 0xe9, 0x0e,
    0xd8, 0xae, 0xf2, 0x2a, 0x1c, 0x15, 0x42, 0x0e, 0x48, 0x36, 0x4e, 0x08, 0xd1, 0xd3, 0x79, 0xab,
];
/// SHA-256 of the published checkout JSON schema at [`CHECKOUT_SCHEMA`].
pub const UCP_CHECKOUT_SCHEMA_SHA256: [u8; 32] = [
    0xb7, 0xd4, 0x30, 0x00, 0xbd, 0xb1, 0xf8, 0x45, 0x33, 0x4a, 0xf1, 0xe3, 0xe1, 0xa9, 0xa3, 0x71,
    0x75, 0x84, 0x40, 0xdd, 0xe1, 0x66, 0xb5, 0xa0, 0x31, 0x07, 0xbd, 0xac, 0xdd, 0x59, 0x48, 0xac,
];
/// SHA-256 of the published order JSON schema at [`ORDER_SCHEMA`].
pub const UCP_ORDER_SCHEMA_SHA256: [u8; 32] = [
    0x12, 0xe6, 0x22, 0x1a, 0xc7, 0x53, 0xa2, 0x02, 0xea, 0x3a, 0x2f, 0x23, 0x05, 0xe8, 0x94, 0xb6,
    0x60, 0xea, 0x81, 0xeb, 0xd1, 0x18, 0x9b, 0xf2, 0x9f, 0x88, 0xf4, 0x8d, 0xd5, 0xd9, 0x2e, 0xed,
];
/// SHA-256 of the published shopping REST OpenAPI schema at [`REST_SCHEMA`].
pub const UCP_REST_SCHEMA_SHA256: [u8; 32] = [
    0xe5, 0x0a, 0x19, 0x54, 0x41, 0x4d, 0xe2, 0x33, 0x44, 0x4f, 0x72, 0x62, 0xf6, 0x57, 0x06, 0x46,
    0xd9, 0x41, 0xcb, 0xc1, 0xc9, 0xf3, 0x2b, 0x87, 0x60, 0x84, 0xdb, 0x7f, 0x01, 0xe8, 0x99, 0x7f,
];

/// One exact UCP capability declaration from a discovery profile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Capability {
    name: String,
    version: String,
    spec: String,
    schema: String,
}

impl Capability {
    /// Parses one bounded, versioned capability declaration.
    ///
    /// # Errors
    ///
    /// Refuses invalid names, versions, or non-HTTPS specification URLs.
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
        spec: impl Into<String>,
        schema: impl Into<String>,
    ) -> Result<Self, UcpError> {
        let value = Self {
            name: name.into(),
            version: version.into(),
            spec: spec.into(),
            schema: schema.into(),
        };
        if !valid_name(&value.name)
            || !valid_version(&value.version)
            || !valid_https_url(&value.spec)
            || !valid_https_url(&value.schema)
        {
            return Err(UcpError::InvalidProfile);
        }
        Ok(value)
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    #[must_use]
    pub fn spec(&self) -> &str {
        &self.spec
    }

    #[must_use]
    pub fn schema(&self) -> &str {
        &self.schema
    }
}

/// UCP service advertisement. The REST endpoint is deployment-owned while
/// the service schema is pinned to the supported UCP revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestService {
    endpoint: String,
    schema: String,
}

/// One profile-advertised UCP payment handler. The handler wire contract is
/// deployment-supplied and must name exact HTTPS specification and schema
/// documents rather than an unversioned implementation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentHandler {
    id: String,
    version: String,
    spec: String,
    schema: String,
}

impl PaymentHandler {
    /// Creates one exact payment-handler declaration.
    ///
    /// # Errors
    ///
    /// Refuses invalid identifiers, versions, and non-HTTPS contract URLs.
    pub fn new(
        id: impl Into<String>,
        version: impl Into<String>,
        spec: impl Into<String>,
        schema: impl Into<String>,
    ) -> Result<Self, UcpError> {
        let value = Self {
            id: id.into(),
            version: version.into(),
            spec: spec.into(),
            schema: schema.into(),
        };
        if !valid_name(&value.id)
            || !valid_version(&value.version)
            || !valid_https_url(&value.spec)
            || !valid_https_url(&value.schema)
        {
            return Err(UcpError::InvalidProfile);
        }
        Ok(value)
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    #[must_use]
    pub fn payment_handler_digest(&self) -> [u8; 32] {
        self.digest()
    }

    fn digest(&self) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(self.id.as_bytes());
        hash.update([0]);
        hash.update(self.version.as_bytes());
        hash.update([0]);
        hash.update(self.spec.as_bytes());
        hash.update([0]);
        hash.update(self.schema.as_bytes());
        hash.finalize().into()
    }
}

impl RestService {
    /// Creates an HTTPS REST service declaration.
    ///
    /// # Errors
    ///
    /// Refuses non-HTTPS or oversized URLs.
    pub fn new(endpoint: impl Into<String>) -> Result<Self, UcpError> {
        let endpoint = endpoint.into();
        if !valid_https_url(&endpoint) {
            return Err(UcpError::InvalidProfile);
        }
        Ok(Self {
            endpoint,
            schema: REST_SCHEMA.to_owned(),
        })
    }

    #[must_use]
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    #[must_use]
    pub fn schema(&self) -> &str {
        &self.schema
    }
}

/// Machine-readable merchant discovery profile for a `LayerX` seller.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MerchantProfile {
    version: String,
    rest: RestService,
    capabilities: Vec<Capability>,
    payment_handlers: Vec<PaymentHandler>,
}

impl MerchantProfile {
    /// Publishes the exact supported UCP checkout and order capabilities.
    ///
    /// # Errors
    ///
    /// Refuses an invalid deployment endpoint.
    pub fn layerx(
        rest_endpoint: impl Into<String>,
        payment_handler: PaymentHandler,
    ) -> Result<Self, UcpError> {
        Ok(Self {
            version: UCP_VERSION.to_owned(),
            rest: RestService::new(rest_endpoint)?,
            capabilities: vec![
                Capability::new(
                    CHECKOUT_CAPABILITY,
                    UCP_VERSION,
                    CHECKOUT_SPEC,
                    CHECKOUT_SCHEMA,
                )?,
                Capability::new(ORDER_CAPABILITY, UCP_VERSION, ORDER_SPEC, ORDER_SCHEMA)?,
            ],
            payment_handlers: vec![payment_handler],
        })
    }

    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    #[must_use]
    pub const fn rest(&self) -> &RestService {
        &self.rest
    }

    #[must_use]
    pub fn capabilities(&self) -> &[Capability] {
        &self.capabilities
    }

    #[must_use]
    pub fn payment_handlers(&self) -> &[PaymentHandler] {
        &self.payment_handlers
    }
}

/// Platform-advertised profile URL and capabilities carried by `UCP-Agent`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlatformProfile {
    pub profile_url: String,
    pub capabilities: Vec<Capability>,
    pub payment_handlers: Vec<PaymentHandler>,
}

/// Server-selected intersection of merchant and platform capabilities.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NegotiatedCapabilities {
    checkout: bool,
    order: bool,
    payment_handler_digest: [u8; 32],
}

impl NegotiatedCapabilities {
    /// Computes the exact name-and-version intersection declared by UCP.
    ///
    /// # Errors
    ///
    /// Refuses an invalid profile URL or a platform without the supported
    /// checkout revision.
    pub fn negotiate(
        platform: &PlatformProfile,
        merchant_handler: &PaymentHandler,
    ) -> Result<Self, UcpError> {
        if !valid_https_url(&platform.profile_url) {
            return Err(UcpError::InvalidProfile);
        }
        let supports = |name: &str| {
            platform
                .capabilities
                .iter()
                .any(|capability| capability.name == name && capability.version == UCP_VERSION)
        };
        let checkout = supports(CHECKOUT_CAPABILITY);
        if !checkout {
            return Err(UcpError::CapabilityUnavailable);
        }
        let handler = platform.payment_handlers.iter().find(|handler| {
            handler.id == merchant_handler.id && handler.version == merchant_handler.version
        });
        let Some(handler) = handler else {
            return Err(UcpError::PaymentHandlerUnavailable);
        };
        if handler != merchant_handler {
            return Err(UcpError::PaymentHandlerMismatch);
        }
        Ok(Self {
            checkout,
            order: supports(ORDER_CAPABILITY),
            payment_handler_digest: handler.digest(),
        })
    }

    #[must_use]
    pub const fn checkout(self) -> bool {
        self.checkout
    }

    #[must_use]
    pub const fn order(self) -> bool {
        self.order
    }

    #[must_use]
    pub const fn payment_handler_digest(self) -> [u8; 32] {
        self.payment_handler_digest
    }
}

/// Exact UUID idempotency identity required by UCP completion operations.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct UcpIdempotencyKey([u8; 16]);

impl UcpIdempotencyKey {
    /// Parses a canonical lowercase or uppercase UUID string.
    ///
    /// # Errors
    ///
    /// Refuses malformed and all-zero UUIDs.
    pub fn parse(value: &str) -> Result<Self, UcpError> {
        if value.len() != 36
            || !value.bytes().enumerate().all(|(index, byte)| match index {
                8 | 13 | 18 | 23 => byte == b'-',
                _ => byte.is_ascii_hexdigit(),
            })
        {
            return Err(UcpError::InvalidIdempotencyKey);
        }
        let mut bytes = [0_u8; 16];
        let mut output = 0;
        let mut high = None;
        for byte in value.bytes().filter(|byte| *byte != b'-') {
            let nibble = hex_nibble(byte).ok_or(UcpError::InvalidIdempotencyKey)?;
            if let Some(first) = high.take() {
                bytes[output] = (first << 4) | nibble;
                output += 1;
            } else {
                high = Some(nibble);
            }
        }
        if bytes == [0; 16] {
            return Err(UcpError::InvalidIdempotencyKey);
        }
        Ok(Self(bytes))
    }

    #[must_use]
    pub fn gateway_key(self) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(IDEMPOTENCY_DOMAIN);
        hash.update(self.0);
        hash.finalize().into()
    }
}

/// UCP checkout lifecycle, including the official status vocabulary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckoutStatus {
    Incomplete,
    RequiresEscalation,
    ReadyForComplete,
    CompleteInProgress,
    Completed,
    Canceled,
}

/// One exact checkout completion request in ISO-4217 minor units.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckoutSubmission {
    pub checkout_id: String,
    pub currency: [u8; 3],
    pub total_minor: u128,
    pub layerx_asset: [u8; 32],
    pub layerx_recipient: [u8; 32],
    pub idempotency_key: UcpIdempotencyKey,
    pub negotiated: NegotiatedCapabilities,
}

impl CheckoutSubmission {
    fn validate(&self) -> Result<(), UcpError> {
        if !valid_identifier(&self.checkout_id)
            || !self.currency.iter().all(u8::is_ascii_uppercase)
            || self.total_minor == 0
            || self.layerx_asset == [0; 32]
            || self.layerx_recipient == [0; 32]
            || !self.negotiated.checkout
            || self.negotiated.payment_handler_digest == [0; 32]
        {
            return Err(UcpError::InvalidCheckout);
        }
        Ok(())
    }
}

/// Merchant-owned, non-financial metadata returned only alongside an
/// executed protocol receipt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrderMetadata {
    pub order_id: String,
    pub permalink_url: String,
}

impl OrderMetadata {
    fn validate(&self) -> Result<(), UcpError> {
        if !valid_identifier(&self.order_id) || !valid_https_url(&self.permalink_url) {
            return Err(UcpError::InvalidOrder);
        }
        Ok(())
    }
}

/// Existing plane request. Payment construction remains in the plane's typed
/// path; this adapter supplies no payload encoding or protocol authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UcpPaymentIntent {
    pub checkout_id: String,
    pub currency: [u8; 3],
    pub amount: u128,
    pub asset: [u8; 32],
    pub recipient: [u8; 32],
    pub idempotency_key: [u8; 32],
}

/// Plane result. No completed order can be constructed from an open result.
#[derive(Debug)]
pub enum UcpPlaneResult {
    Pending,
    Refused,
    Executed(Box<ExecutedUcpPayment>),
}

/// Executed merchant metadata and independently verifiable protocol evidence.
#[derive(Debug)]
pub struct ExecutedUcpPayment {
    pub metadata: OrderMetadata,
    pub canonical_receipt: Vec<u8>,
    pub authorised_batch: AuthorizedBatch,
}

/// Typed `LayerX` payment boundary used by the UCP seller.
pub trait UcpPaymentPlane {
    /// Executes one checkout payment under the supplied stable identity.
    ///
    /// # Errors
    ///
    /// Returns a typed refusal and never fabricates merchant or receipt data.
    /// Implementations preserve the intent's idempotency key and replay the
    /// original executed result for a completed retry.
    fn execute(
        &mut self,
        intent: &UcpPaymentIntent,
        trace: &TraceId,
    ) -> Result<UcpPlaneResult, UcpError>;
}

/// Receipt-backed UCP order snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UcpOrder {
    pub id: String,
    pub checkout_id: String,
    pub permalink_url: String,
    pub currency: [u8; 3],
    pub total_minor: u128,
    pub receipt_digest: [u8; 32],
}

/// UCP-visible completion response. A pending status carries no order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckoutOutcome {
    pub status: CheckoutStatus,
    pub order: Option<UcpOrder>,
}

/// Stored order material used by GET order. Re-verification is mandatory on
/// every read so stale or corrupted receipt bytes never back a response.
#[derive(Debug)]
pub struct StoredOrder {
    pub submission: CheckoutSubmission,
    pub metadata: OrderMetadata,
    pub canonical_receipt: Vec<u8>,
    pub authorised_batch: AuthorizedBatch,
}

/// UCP checkout and order adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UcpAdapter;

impl UcpAdapter {
    /// Completes a checkout through the receipt-gated payment boundary.
    ///
    /// # Errors
    ///
    /// Refuses invalid checkout data, gateway conflicts, plane failures, and
    /// every receipt that does not match the checkout total and recipient.
    pub fn complete_checkout(
        gateway: &mut GatewayCore,
        principal: &PrincipalId,
        submission: &CheckoutSubmission,
        plane: &mut impl UcpPaymentPlane,
        trace: &TraceId,
        now: u64,
    ) -> Result<CheckoutOutcome, Traced<UcpError>> {
        let fail = |error| trace.wrap(error);
        submission.validate().map_err(fail)?;
        let key = submission.idempotency_key.gateway_key();
        let request = TranslationRequest::new(
            adapter_id().map_err(fail)?,
            TranslationKind::StateChanging,
            key,
            request_digest(submission),
        )
        .map_err(|error| fail(UcpError::Gateway(error)))?;
        let previously_settled = match gateway
            .begin_translation(principal, &request, trace, now)
            .map_err(|error| trace.wrap(UcpError::Gateway(error.into_error())))?
        {
            TranslationStatus::Pending => None,
            TranslationStatus::Refused => return Ok(refused_outcome()),
            TranslationStatus::ReceiptVerified { receipt_digest } => Some(receipt_digest),
            TranslationStatus::Translated => {
                return Err(fail(UcpError::Gateway(GatewayError::Corrupt(
                    "UCP payment has a read-only completion",
                ))));
            }
        };
        let intent = UcpPaymentIntent {
            checkout_id: submission.checkout_id.clone(),
            currency: submission.currency,
            amount: submission.total_minor,
            asset: submission.layerx_asset,
            recipient: submission.layerx_recipient,
            idempotency_key: key,
        };
        match plane.execute(&intent, trace).map_err(fail)? {
            UcpPlaneResult::Pending if previously_settled.is_none() => Ok(pending_outcome()),
            UcpPlaneResult::Pending => Err(fail(UcpError::OrderEvidenceRequired)),
            UcpPlaneResult::Refused => {
                if previously_settled.is_some() {
                    return Err(fail(UcpError::OrderEvidenceRequired));
                }
                gateway
                    .refuse_translation(principal, key, trace, now)
                    .map_err(|error| trace.wrap(UcpError::Gateway(error.into_error())))?;
                Ok(refused_outcome())
            }
            UcpPlaneResult::Executed(executed) => {
                let ExecutedUcpPayment {
                    metadata,
                    canonical_receipt,
                    authorised_batch,
                } = *executed;
                metadata.validate().map_err(fail)?;
                let receipt_digest =
                    verify_order_receipt(submission, &canonical_receipt, &authorised_batch)
                        .map_err(fail)?;
                let status = gateway
                    .settle_with_receipt(
                        principal,
                        key,
                        &canonical_receipt,
                        &authorised_batch,
                        trace,
                        now,
                    )
                    .map_err(|error| trace.wrap(UcpError::Gateway(error.into_error())))?;
                if status != (TranslationStatus::ReceiptVerified { receipt_digest })
                    || previously_settled.is_some_and(|digest| digest != receipt_digest)
                {
                    return Err(fail(UcpError::ReceiptRequired));
                }
                Ok(completed_outcome(submission, &metadata, receipt_digest))
            }
        }
    }

    /// Re-verifies stored order evidence for the UCP order read operation.
    ///
    /// # Errors
    ///
    /// Refuses missing order capability, invalid merchant metadata, and stale
    /// or mismatched receipt evidence.
    pub fn read_order(stored: &StoredOrder) -> Result<UcpOrder, UcpError> {
        stored.submission.validate()?;
        if !stored.submission.negotiated.order {
            return Err(UcpError::CapabilityUnavailable);
        }
        stored.metadata.validate()?;
        let receipt_digest = verify_order_receipt(
            &stored.submission,
            &stored.canonical_receipt,
            &stored.authorised_batch,
        )?;
        Ok(order(&stored.submission, &stored.metadata, receipt_digest))
    }
}

fn verify_order_receipt(
    submission: &CheckoutSubmission,
    canonical_receipt: &[u8],
    authorised_batch: &AuthorizedBatch,
) -> Result<[u8; 32], UcpError> {
    let verified =
        verify(canonical_receipt, authorised_batch).map_err(|_| UcpError::ReceiptMismatch)?;
    let protocol = verified
        .receipt()
        .protocol()
        .ok_or(UcpError::ReceiptMismatch)?;
    if protocol.asset() != submission.layerx_asset
        || protocol.amount() != submission.total_minor
        || protocol.to() != submission.layerx_recipient
    {
        return Err(UcpError::ReceiptMismatch);
    }
    leaf_hash(verified.canonical_bytes()).map_err(|_| UcpError::ReceiptMismatch)
}

fn completed_outcome(
    submission: &CheckoutSubmission,
    metadata: &OrderMetadata,
    receipt_digest: [u8; 32],
) -> CheckoutOutcome {
    CheckoutOutcome {
        status: CheckoutStatus::Completed,
        order: Some(order(submission, metadata, receipt_digest)),
    }
}

fn order(
    submission: &CheckoutSubmission,
    metadata: &OrderMetadata,
    receipt_digest: [u8; 32],
) -> UcpOrder {
    UcpOrder {
        id: metadata.order_id.clone(),
        checkout_id: submission.checkout_id.clone(),
        permalink_url: metadata.permalink_url.clone(),
        currency: submission.currency,
        total_minor: submission.total_minor,
        receipt_digest,
    }
}

const fn pending_outcome() -> CheckoutOutcome {
    CheckoutOutcome {
        status: CheckoutStatus::CompleteInProgress,
        order: None,
    }
}

const fn refused_outcome() -> CheckoutOutcome {
    CheckoutOutcome {
        status: CheckoutStatus::Incomplete,
        order: None,
    }
}

fn request_digest(submission: &CheckoutSubmission) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(REQUEST_DOMAIN);
    hash.update(submission.checkout_id.as_bytes());
    hash.update([0]);
    hash.update(submission.currency);
    hash.update(submission.total_minor.to_be_bytes());
    hash.update(submission.layerx_asset);
    hash.update(submission.layerx_recipient);
    hash.update(submission.negotiated.payment_handler_digest);
    hash.update([
        u8::from(submission.negotiated.checkout),
        u8::from(submission.negotiated.order),
    ]);
    hash.finalize().into()
}

fn adapter_id() -> Result<AdapterId, UcpError> {
    AdapterId::new(ADAPTER_ID).map_err(|error| UcpError::Gateway(error.into()))
}

/// Declares the adapter against a content-pinned UCP specification and its
/// real conformance suite.
///
/// # Errors
///
/// Returns an adapter declaration refusal if the stable identifier is invalid.
pub fn ucp_adapter_descriptor(
    spec: PinnedSpec,
    conformance: ConformanceSuite,
) -> Result<AdapterDescriptor, UcpError> {
    Ok(AdapterDescriptor::new(adapter_id()?, spec, conformance))
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= VALUE_LIMIT
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}

fn valid_version(value: &str) -> bool {
    value.len() == 10
        && value.bytes().enumerate().all(|(index, byte)| match index {
            4 | 7 => byte == b'-',
            _ => byte.is_ascii_digit(),
        })
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= VALUE_LIMIT
        && value
            .bytes()
            .all(|byte| !byte.is_ascii_control() && !byte.is_ascii_whitespace())
}

fn valid_https_url(value: &str) -> bool {
    value.len() <= VALUE_LIMIT
        && value.starts_with("https://")
        && value[8..].contains('.')
        && !value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
}

const fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Stable UCP adapter refusals.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UcpError {
    InvalidProfile,
    CapabilityUnavailable,
    PaymentHandlerUnavailable,
    PaymentHandlerMismatch,
    InvalidIdempotencyKey,
    InvalidCheckout,
    InvalidOrder,
    PlaneRefused,
    ReceiptRequired,
    ReceiptMismatch,
    OrderEvidenceRequired,
    Gateway(GatewayError),
}

impl Display for UcpError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidProfile => formatter.write_str("UCP profile is invalid"),
            Self::CapabilityUnavailable => {
                formatter.write_str("required UCP capability is unavailable")
            }
            Self::PaymentHandlerUnavailable => {
                formatter.write_str("required UCP payment handler is unavailable")
            }
            Self::PaymentHandlerMismatch => {
                formatter.write_str("UCP payment handler contract does not match")
            }
            Self::InvalidIdempotencyKey => formatter.write_str("UCP idempotency key is invalid"),
            Self::InvalidCheckout => formatter.write_str("UCP checkout is invalid"),
            Self::InvalidOrder => formatter.write_str("UCP order metadata is invalid"),
            Self::PlaneRefused => formatter.write_str("checkout payment was refused"),
            Self::ReceiptRequired => formatter.write_str("a verified LayerX receipt is required"),
            Self::ReceiptMismatch => {
                formatter.write_str("LayerX receipt does not match the checkout")
            }
            Self::OrderEvidenceRequired => {
                formatter.write_str("stored order evidence is required for completed checkout")
            }
            Self::Gateway(error) => write!(formatter, "gateway translation failed: {error}"),
        }
    }
}

impl std::error::Error for UcpError {}

/// Codify anchor for the UCP merchant profile, checkout, and order adapter.
#[must_use]
pub const fn interop_ucp() -> &'static str {
    "ucp-2026-04-08-receipt-backed-commerce"
}
