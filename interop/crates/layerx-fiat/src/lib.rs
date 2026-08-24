#![forbid(unsafe_code)]

use std::fmt::{Debug, Display, Formatter};

use layerx_interop_gateway::adapter::{AdapterDescriptor, AdapterId, ConformanceSuite, PinnedSpec};
use layerx_interop_gateway::error::GatewayError;
use layerx_interop_gateway::gateway::{TranslationKind, TranslationRequest, TranslationStatus};
use layerx_interop_gateway::principal::PrincipalId;
use layerx_interop_gateway::trace::{TraceId, Traced};
use layerx_interop_gateway::GatewayCore;
use layerx_proof::receipt::{verify, AuthorizedBatch};
use sha2::{Digest as _, Sha256};
use zeroize::Zeroizing;

const ADAPTER_ID: &str = "fiat";
const EVIDENCE_LIMIT: usize = 64 * 1_024;
const TOKEN_LIMIT: usize = 512;
const IDENTIFIER_LIMIT: usize = 96;
const IDEMPOTENCY_DOMAIN: &[u8] = b"LayerX/interop/fiat/idempotency/v1\0";
const REQUEST_DOMAIN: &[u8] = b"LayerX/interop/fiat/request/v1\0";

/// Supported certified-provider edge rails. Card data terminates at the
/// provider; this crate accepts only opaque token references for all rails.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FiatRail {
    Card,
    Bank,
    RealTimePayment,
}

impl FiatRail {
    const fn code(self) -> u8 {
        match self {
            Self::Card => 1,
            Self::Bank => 2,
            Self::RealTimePayment => 3,
        }
    }
}

/// Provider evidence classes, kept distinct so authorisation and clearing can
/// never be mistaken for settled funds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvidenceClass {
    Authorised,
    Clearing,
    Settled,
    Reversed,
    Chargeback,
}

impl EvidenceClass {
    const fn code(self) -> u8 {
        match self {
            Self::Authorised => 1,
            Self::Clearing => 2,
            Self::Settled => 3,
            Self::Reversed => 4,
            Self::Chargeback => 5,
        }
    }
}

/// Validated provider or settlement identifier with no path/control bytes.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExternalId(String);

impl ExternalId {
    /// Parses one bounded opaque identifier.
    ///
    /// # Errors
    ///
    /// Refuses empty, oversized and control-bearing values.
    pub fn new(value: impl Into<String>) -> Result<Self, FiatError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > IDENTIFIER_LIMIT
            || value.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(FiatError::InvalidIdentifier);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Opaque provider token. Its contents are zeroised, never formatted, and a
/// decimal string that could be raw card data is rejected at construction.
pub struct TokenReference(Zeroizing<Vec<u8>>);

impl TokenReference {
    /// Adopts a certified-provider token after excluding raw PAN-shaped input.
    ///
    /// # Errors
    ///
    /// Refuses empty, oversized, control-bearing and PAN-shaped values.
    pub fn new(value: Vec<u8>) -> Result<Self, FiatError> {
        if value.is_empty()
            || value.len() > TOKEN_LIMIT
            || value.iter().any(u8::is_ascii_control)
            || looks_like_pan(&value)
        {
            return Err(FiatError::CardDataRefused);
        }
        Ok(Self(Zeroizing::new(value)))
    }

    /// Exposes the opaque token only to the certified provider boundary.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl Debug for TokenReference {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("TokenReference([REDACTED])")
    }
}

/// Bounded canonical provider evidence. Bytes are deliberately excluded from
/// `Debug`; only the digest crosses audit and telemetry boundaries.
pub struct ProviderEvidence {
    canonical: Zeroizing<Vec<u8>>,
    digest: [u8; 32],
}

impl ProviderEvidence {
    /// Creates a bounded evidence envelope and commits to its exact bytes.
    ///
    /// # Errors
    ///
    /// Refuses empty or oversized evidence.
    pub fn new(canonical: Vec<u8>) -> Result<Self, FiatError> {
        if canonical.is_empty() || canonical.len() > EVIDENCE_LIMIT {
            return Err(FiatError::InvalidEvidence);
        }
        let digest = Sha256::digest(&canonical).into();
        Ok(Self {
            canonical: Zeroizing::new(canonical),
            digest,
        })
    }

    #[must_use]
    pub fn canonical(&self) -> &[u8] {
        &self.canonical
    }

    #[must_use]
    pub const fn digest(&self) -> [u8; 32] {
        self.digest
    }
}

impl Debug for ProviderEvidence {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderEvidence")
            .field("digest", &self.digest)
            .field("canonical", &"[REDACTED]")
            .finish()
    }
}

/// Provider-authenticated settlement facts returned by a rail-specific
/// verifier. Amounts are exact integers and the destination is a `LayerX`
/// account, never a card or bank credential.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedProviderFacts {
    pub provider: ExternalId,
    pub settlement: ExternalId,
    pub rail: FiatRail,
    pub class: EvidenceClass,
    pub amount: u128,
    pub asset: [u8; 32],
    pub destination: [u8; 32],
    pub observed_at: u64,
    pub hold_until: Option<u64>,
}

impl VerifiedProviderFacts {
    fn validate(&self) -> Result<(), FiatError> {
        if self.amount == 0
            || self.asset == [0; 32]
            || self.destination == [0; 32]
            || self.observed_at == 0
        {
            return Err(FiatError::InvalidEvidence);
        }
        match self.class {
            EvidenceClass::Authorised | EvidenceClass::Clearing => {
                if self
                    .hold_until
                    .is_none_or(|until| until <= self.observed_at)
                {
                    return Err(FiatError::HoldRequired);
                }
            }
            EvidenceClass::Settled => {
                if self.hold_until.is_some() {
                    return Err(FiatError::InvalidEvidence);
                }
            }
            EvidenceClass::Reversed | EvidenceClass::Chargeback => {
                if self
                    .hold_until
                    .is_some_and(|until| until < self.observed_at)
                {
                    return Err(FiatError::InvalidEvidence);
                }
            }
        }
        Ok(())
    }
}

/// Certified provider edge. Implementations verify provider signatures,
/// finality and account binding before returning typed facts.
pub trait ProviderVerifier {
    /// Verifies one provider evidence envelope and token reference.
    ///
    /// # Errors
    ///
    /// Returns a typed provider or evidence refusal.
    fn verify(
        &self,
        token: &TokenReference,
        evidence: &ProviderEvidence,
        trace: &TraceId,
    ) -> Result<VerifiedProviderFacts, FiatError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FiatIntentKind {
    Credit,
    Reverse,
    Chargeback,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FiatIntent {
    pub kind: FiatIntentKind,
    pub provider: ExternalId,
    pub settlement: ExternalId,
    pub amount: u128,
    pub asset: [u8; 32],
    pub destination: [u8; 32],
    pub evidence_digest: [u8; 32],
    pub idempotency_key: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlaneFiatOutcome {
    Pending,
    Refused,
}

#[derive(Debug)]
pub struct ExecutedFiatOutcome {
    pub canonical_receipt: Vec<u8>,
    pub authorised_batch: AuthorizedBatch,
}

#[derive(Debug)]
pub enum FiatPlaneResult {
    Open(PlaneFiatOutcome),
    Executed(ExecutedFiatOutcome),
}

/// Real `LayerX` transition boundary used for credits, reversals and
/// chargebacks. Implementations preserve the supplied idempotency key.
pub trait FiatPlane {
    /// Executes one receipt-producing `LayerX` intent.
    ///
    /// # Errors
    ///
    /// Returns a typed plane refusal without inventing settlement evidence.
    fn execute(
        &mut self,
        intent: FiatIntent,
        trace: &TraceId,
    ) -> Result<FiatPlaneResult, FiatError>;
}

/// Honest adapter-visible journey state. Provider-stage holds and protocol
/// execution remain explicit until receipt evidence supports completion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FiatJourneyState {
    AuthorisedHold { until: u64 },
    ClearingHold { until: u64 },
    CreditPending,
    Credited { receipt_digest: [u8; 32] },
    ReversalPending { hold_until: Option<u64> },
    Reversed { receipt_digest: [u8; 32] },
    ChargebackPending { hold_until: Option<u64> },
    ChargedBack { receipt_digest: [u8; 32] },
    Refused,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FiatAdapter;

impl FiatAdapter {
    /// Verifies provider-authenticated evidence and returns only validated,
    /// typed settlement facts. Executable gateway composition uses this gate
    /// before durable reservation or a typed plane call.
    ///
    /// # Errors
    /// Returns a trace-bound provider or evidence refusal.
    pub fn verify_evidence(
        token: &TokenReference,
        evidence: &ProviderEvidence,
        verifier: &impl ProviderVerifier,
        trace: &TraceId,
    ) -> Result<VerifiedProviderFacts, Traced<FiatError>> {
        let fail = |error| trace.wrap(error);
        let facts = verifier.verify(token, evidence, trace).map_err(fail)?;
        facts.validate().map_err(fail)?;
        Ok(facts)
    }

    /// Verifies provider evidence and advances the matching honest journey.
    /// Authorisation and clearing only produce declared holds. Credit,
    /// reversal and chargeback complete only under a verified `LayerX` receipt.
    ///
    /// # Errors
    ///
    /// Returns a trace-bound typed refusal for provider, evidence, gateway or
    /// receipt mismatches. No failed path credits value.
    #[allow(clippy::too_many_arguments)]
    pub fn apply(
        gateway: &mut GatewayCore,
        principal: &PrincipalId,
        token: &TokenReference,
        evidence: &ProviderEvidence,
        verifier: &impl ProviderVerifier,
        plane: &mut impl FiatPlane,
        trace: &TraceId,
        now: u64,
    ) -> Result<FiatJourneyState, Traced<FiatError>> {
        let fail = |error| trace.wrap(error);
        let facts = Self::verify_evidence(token, evidence, verifier, trace)?;
        match facts.class {
            EvidenceClass::Authorised | EvidenceClass::Clearing => {
                Self::record_hold(gateway, principal, evidence, &facts, trace, now)
            }
            EvidenceClass::Settled | EvidenceClass::Reversed | EvidenceClass::Chargeback => {
                Self::execute(gateway, principal, evidence, &facts, plane, trace, now)
            }
        }
    }

    fn record_hold(
        gateway: &mut GatewayCore,
        principal: &PrincipalId,
        evidence: &ProviderEvidence,
        facts: &VerifiedProviderFacts,
        trace: &TraceId,
        now: u64,
    ) -> Result<FiatJourneyState, Traced<FiatError>> {
        let fail = |error| trace.wrap(error);
        let idempotency_key = idempotency_key(facts);
        let request = TranslationRequest::new(
            adapter_id().map_err(fail)?,
            TranslationKind::ReadOnly,
            idempotency_key,
            request_digest(facts, evidence.digest()),
        )
        .map_err(|error| fail(FiatError::Gateway(error)))?;
        let opened = gateway
            .begin_translation(principal, &request, trace, now)
            .map_err(|error| trace.wrap(FiatError::Gateway(error.into_error())))?;
        if opened == TranslationStatus::Refused {
            return Ok(FiatJourneyState::Refused);
        }
        gateway
            .complete_read_only(principal, idempotency_key, trace, now)
            .map_err(|error| trace.wrap(FiatError::Gateway(error.into_error())))?;
        Ok(pending_state(facts))
    }

    fn execute(
        gateway: &mut GatewayCore,
        principal: &PrincipalId,
        evidence: &ProviderEvidence,
        facts: &VerifiedProviderFacts,
        plane: &mut impl FiatPlane,
        trace: &TraceId,
        now: u64,
    ) -> Result<FiatJourneyState, Traced<FiatError>> {
        let fail = |error| trace.wrap(error);
        let kind = intent_kind(facts.class).map_err(fail)?;
        let request_digest = request_digest(facts, evidence.digest());
        let idempotency_key = idempotency_key(facts);
        let request = TranslationRequest::new(
            adapter_id().map_err(fail)?,
            TranslationKind::StateChanging,
            idempotency_key,
            request_digest,
        )
        .map_err(|error| fail(FiatError::Gateway(error)))?;
        let opened = gateway
            .begin_translation(principal, &request, trace, now)
            .map_err(|error| trace.wrap(FiatError::Gateway(error.into_error())))?;
        if opened == TranslationStatus::Refused {
            return Ok(FiatJourneyState::Refused);
        }
        let intent = FiatIntent {
            kind,
            provider: facts.provider.clone(),
            settlement: facts.settlement.clone(),
            amount: facts.amount,
            asset: facts.asset,
            destination: facts.destination,
            evidence_digest: evidence.digest(),
            idempotency_key,
        };
        match plane.execute(intent, trace).map_err(fail)? {
            FiatPlaneResult::Open(PlaneFiatOutcome::Pending) => Ok(pending_state(facts)),
            FiatPlaneResult::Open(PlaneFiatOutcome::Refused) => {
                gateway
                    .refuse_translation(principal, idempotency_key, trace, now)
                    .map_err(|error| trace.wrap(FiatError::Gateway(error.into_error())))?;
                Ok(FiatJourneyState::Refused)
            }
            FiatPlaneResult::Executed(executed) => {
                let verified = verify(&executed.canonical_receipt, &executed.authorised_batch)
                    .map_err(|_| fail(FiatError::ReceiptMismatch))?;
                let protocol = verified
                    .receipt()
                    .protocol()
                    .ok_or_else(|| fail(FiatError::ReceiptMismatch))?;
                let account_matches = match facts.class {
                    EvidenceClass::Settled => protocol.to() == facts.destination,
                    EvidenceClass::Reversed | EvidenceClass::Chargeback => {
                        protocol.from() == facts.destination
                    }
                    EvidenceClass::Authorised | EvidenceClass::Clearing => false,
                };
                if protocol.asset() != facts.asset
                    || protocol.amount() != facts.amount
                    || !account_matches
                {
                    return Err(fail(FiatError::ReceiptMismatch));
                }
                let status = gateway
                    .settle_with_receipt(
                        principal,
                        idempotency_key,
                        &executed.canonical_receipt,
                        &executed.authorised_batch,
                        trace,
                        now,
                    )
                    .map_err(|error| trace.wrap(FiatError::Gateway(error.into_error())))?;
                let TranslationStatus::ReceiptVerified { receipt_digest } = status else {
                    return Err(fail(FiatError::ReceiptRequired));
                };
                Ok(completed_state(facts.class, receipt_digest))
            }
        }
    }
}

/// Stable, redaction-safe adapter errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FiatError {
    InvalidIdentifier,
    CardDataRefused,
    InvalidEvidence,
    HoldRequired,
    UnsupportedTransition,
    ReceiptRequired,
    ReceiptMismatch,
    Gateway(GatewayError),
}

impl Display for FiatError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidIdentifier => formatter.write_str("provider identifier is invalid"),
            Self::CardDataRefused => formatter.write_str("raw card data is refused"),
            Self::InvalidEvidence => formatter.write_str("provider evidence is invalid"),
            Self::HoldRequired => formatter.write_str("provider stage requires a declared hold"),
            Self::UnsupportedTransition => {
                formatter.write_str("provider evidence cannot drive this transition")
            }
            Self::ReceiptRequired => formatter.write_str("LayerX receipt evidence is required"),
            Self::ReceiptMismatch => {
                formatter.write_str("LayerX receipt does not match provider settlement")
            }
            Self::Gateway(error) => write!(formatter, "gateway translation failed: {error}"),
        }
    }
}

impl std::error::Error for FiatError {}

fn intent_kind(class: EvidenceClass) -> Result<FiatIntentKind, FiatError> {
    match class {
        EvidenceClass::Settled => Ok(FiatIntentKind::Credit),
        EvidenceClass::Reversed => Ok(FiatIntentKind::Reverse),
        EvidenceClass::Chargeback => Ok(FiatIntentKind::Chargeback),
        EvidenceClass::Authorised | EvidenceClass::Clearing => {
            Err(FiatError::UnsupportedTransition)
        }
    }
}

fn pending_state(facts: &VerifiedProviderFacts) -> FiatJourneyState {
    match facts.class {
        EvidenceClass::Settled => FiatJourneyState::CreditPending,
        EvidenceClass::Reversed => FiatJourneyState::ReversalPending {
            hold_until: facts.hold_until,
        },
        EvidenceClass::Chargeback => FiatJourneyState::ChargebackPending {
            hold_until: facts.hold_until,
        },
        EvidenceClass::Authorised => FiatJourneyState::AuthorisedHold {
            until: facts.hold_until.unwrap_or(facts.observed_at),
        },
        EvidenceClass::Clearing => FiatJourneyState::ClearingHold {
            until: facts.hold_until.unwrap_or(facts.observed_at),
        },
    }
}

const fn completed_state(class: EvidenceClass, receipt_digest: [u8; 32]) -> FiatJourneyState {
    match class {
        EvidenceClass::Settled => FiatJourneyState::Credited { receipt_digest },
        EvidenceClass::Reversed => FiatJourneyState::Reversed { receipt_digest },
        EvidenceClass::Chargeback => FiatJourneyState::ChargedBack { receipt_digest },
        EvidenceClass::Authorised | EvidenceClass::Clearing => FiatJourneyState::Refused,
    }
}

fn request_digest(facts: &VerifiedProviderFacts, evidence_digest: [u8; 32]) -> [u8; 32] {
    digest(REQUEST_DOMAIN, facts, evidence_digest, &facts.destination)
}

fn idempotency_key(facts: &VerifiedProviderFacts) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(IDEMPOTENCY_DOMAIN);
    hash.update(facts.provider.as_str().as_bytes());
    hash.update([0]);
    hash.update(facts.settlement.as_str().as_bytes());
    hash.update([facts.rail.code(), facts.class.code()]);
    hash.finalize().into()
}

fn digest(
    domain: &[u8],
    facts: &VerifiedProviderFacts,
    evidence_digest: [u8; 32],
    suffix: &[u8],
) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(domain);
    hash.update(facts.provider.as_str().as_bytes());
    hash.update([0]);
    hash.update(facts.settlement.as_str().as_bytes());
    hash.update([facts.rail.code(), facts.class.code()]);
    hash.update(facts.amount.to_be_bytes());
    hash.update(facts.asset);
    hash.update(facts.observed_at.to_be_bytes());
    hash.update(facts.hold_until.unwrap_or(0).to_be_bytes());
    hash.update(evidence_digest);
    hash.update(suffix);
    hash.finalize().into()
}

fn looks_like_pan(value: &[u8]) -> bool {
    let digit_count = value.iter().filter(|byte| byte.is_ascii_digit()).count();
    (12..=19).contains(&digit_count)
        && value
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b' ' | b'-'))
}

fn adapter_id() -> Result<AdapterId, FiatError> {
    AdapterId::new(ADAPTER_ID).map_err(|error| FiatError::Gateway(error.into()))
}

/// Declares this generic rail boundary against a deployment-supplied,
/// content-pinned provider specification and conformance suite.
///
/// # Errors
///
/// Returns an adapter declaration error if the stable identifier is invalid.
pub fn fiat_adapter_descriptor(
    spec: PinnedSpec,
    conformance: ConformanceSuite,
) -> Result<AdapterDescriptor, FiatError> {
    Ok(AdapterDescriptor::new(adapter_id()?, spec, conformance))
}

/// Codify anchor for the card, bank and RTP adapter boundary.
#[must_use]
pub const fn interop_fiat_adapters() -> &'static str {
    "receipt-gated-card-bank-rtp-adapters"
}
