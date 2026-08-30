//! Construction of complete unsigned activities from verified core state.

use layerx_types::activity::{
    ActivityBuildError, Authority, EnvelopeBuilder, TimestampBound, UnsignedEnvelope,
};
use layerx_types::amount::Amount;
use layerx_types::ids::{Did, IdempotencyKey};
use layerx_types::payload::{ActivityType, ModuleRegistry, Payload, PayloadError};
use layerx_types::result::ResultCode;
use layerx_wire::activity::encode_unsigned_envelope;
use layerx_wire::hash::payload_hash_for;
use layerx_wire::sign::preimage_unsigned;
use layerx_wire::WireError;

#[path = "disclose.rs"]
mod disclosure_binding;
#[path = "expiry.rs"]
mod lifecycle;

pub use disclosure_binding::{DisclosedPreparation, DisclosureBindingError};
pub use lifecycle::{
    ExpirationReport, LifecycleError, LifecycleState, PayloadRedaction, PreparationLifecycle,
    RetentionReport,
};

/// Domain-separated digest of the validated structured disclosure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DisclosureDigest(pub [u8; 32]);

/// Core-produced inputs required to prepare one activity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CorePreparationState {
    pub network_id: u32,
    pub account_sequence: u64,
    pub protocol_timestamp: u64,
    pub observed_head_sequence: u64,
    pub module_registry: ModuleRegistry,
}

/// Boundary that obtains preparation state from core without local guessing.
pub trait CorePreparationBoundary {
    /// Obtains the preparation state for one actor directly from core.
    ///
    /// # Errors
    ///
    /// Returns `Unavailable` when core cannot be reached, `Unverified` when
    /// returned state cannot be trusted, and `Refused` for an authenticated
    /// typed core refusal.
    fn preparation_state(&mut self, actor: &Did) -> Result<CorePreparationState, CoreStateError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoreStateError {
    Unavailable,
    Unverified,
    Refused { class: u8, result: ResultCode },
}

/// Production adapter from the authenticated LNI client into preparation.
/// Correlation identifiers are boundary-local and never supply protocol state.
pub struct ProductionCorePreparationBoundary<'a> {
    client: &'a mut layerx_client::Client,
    next_correlation_id: u64,
    last_state: Option<CorePreparationState>,
}

impl<'a> ProductionCorePreparationBoundary<'a> {
    /// Creates a production boundary with a non-zero first correlation id.
    ///
    /// # Errors
    ///
    /// Refuses zero, which is reserved for the LNI handshake.
    pub fn new(
        client: &'a mut layerx_client::Client,
        first_correlation_id: u64,
    ) -> Result<Self, CoreStateError> {
        if first_correlation_id == 0 {
            return Err(CoreStateError::Unavailable);
        }
        Ok(Self {
            client,
            next_correlation_id: first_correlation_id,
            last_state: None,
        })
    }

    /// Returns the exact node snapshot consumed by the most recent successful
    /// preparation call. Callers retain its registry for later signature
    /// verification instead of querying a second authority.
    #[must_use]
    pub const fn last_state(&self) -> Option<&CorePreparationState> {
        self.last_state.as_ref()
    }
}

impl CorePreparationBoundary for ProductionCorePreparationBoundary<'_> {
    fn preparation_state(&mut self, actor: &Did) -> Result<CorePreparationState, CoreStateError> {
        let correlation_id = self.next_correlation_id;
        if correlation_id == 0 {
            return Err(CoreStateError::Unavailable);
        }
        self.next_correlation_id = correlation_id.checked_add(1).unwrap_or(0);
        let state = self
            .client
            .preparation_state(actor, correlation_id)
            .map_err(map_preparation_state_error)?;
        let state = CorePreparationState {
            network_id: state.network_id,
            account_sequence: state.account_sequence,
            protocol_timestamp: state.protocol_timestamp,
            observed_head_sequence: state.observed_head_sequence,
            module_registry: state.module_registry,
        };
        self.last_state = Some(state.clone());
        Ok(state)
    }
}

fn map_preparation_state_error(error: layerx_client::lni::PreparationStateError) -> CoreStateError {
    use layerx_client::lni::PreparationStateError;
    match error {
        PreparationStateError::Transport(_)
        | PreparationStateError::UnavailableCapability
        | PreparationStateError::Disconnected => CoreStateError::Unavailable,
        PreparationStateError::CoreRefusal { class, result } => {
            CoreStateError::Refused { class, result }
        }
        PreparationStateError::Envelope(_)
        | PreparationStateError::InvalidCorrelation
        | PreparationStateError::InterfaceVersion(_)
        | PreparationStateError::MalformedRequest
        | PreparationStateError::MalformedResponse
        | PreparationStateError::ActorMismatch
        | PreparationStateError::Network { .. }
        | PreparationStateError::StaleSnapshot { .. } => CoreStateError::Unverified,
    }
}

/// Explicit configured defaults and hard preparation bounds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreparationDefaults {
    pub timestamp_span: u64,
    pub fee_limit: Amount,
    pub maximum_payload_bytes: usize,
}

/// Complete caller input. Every omitted bounded field has one explicit default.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrepareRequest {
    pub actor: Did,
    pub authority: Authority,
    pub activity_type: ActivityType,
    pub expected_account_sequence: Option<u64>,
    pub timestamp_bound: Option<TimestampBound>,
    pub fee_limit: Option<Amount>,
    pub idempotency_key: IdempotencyKey,
    pub payload: Vec<u8>,
    pub declared_payload_limit: usize,
}

/// Exact typed envelope plus the only canonical bytes and signer preimage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Prepared {
    pub envelope: UnsignedEnvelope,
    pub canonical_bytes: Vec<u8>,
    pub signing_preimage: [u8; 32],
    pub observed_head_sequence: u64,
    pub disclosure: layerx_crypto::disclosure::Disclosure,
    pub disclosure_digest: DisclosureDigest,
    pub audit: PreparationAuditEntry,
}

/// Preparation audit evidence emitted with every response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparationAuditEntry {
    pub idempotency_key: [u8; 32],
    pub observed_head_sequence: u64,
    pub disclosure_digest: DisclosureDigest,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PrepareError {
    Core(CoreStateError),
    StaleSequence { expected: u64, core: u64 },
    InvalidDefaults,
    TimestampDoesNotContainCore,
    TimestampBoundWidened,
    PayloadLimitExceeded { actual: usize, maximum: usize },
    Payload(PayloadError),
    Activity(ActivityBuildError),
    Wire(WireError),
    Disclosure(DisclosureBindingError),
}

/// Decodes a structured disclosure from canonical prepared bytes.
///
/// # Errors
///
/// Returns the disclosure decode failure, or a mismatch when the decoded disclosure does not
/// re-encode to the exact canonical bytes.
pub fn disclose(
    canonical_bytes: &[u8],
    registry: &ModuleRegistry,
) -> Result<DisclosedPreparation, DisclosureBindingError> {
    disclosure_binding::decode_and_bind(canonical_bytes, registry)
}

/// Revalidates that a held preparation still matches its disclosure.
///
/// # Errors
///
/// Returns a decode or re-encoding mismatch, or a digest that no longer matches the recorded
/// preparation and audit digests.
pub fn verify_disclosure_binding(prepared: &Prepared) -> Result<(), DisclosureBindingError> {
    disclosure_binding::verify_binding(prepared)
}

/// Expires every elapsed unsubmitted preparation and releases its reservations.
///
/// # Errors
///
/// Returns `Unavailable` when the lifecycle records cannot be locked, or the reservation refusal
/// that prevented a release.
pub fn expire(
    lifecycle: &PreparationLifecycle,
    limiter: &crate::budget::BudgetLimiter,
    core_batch_time: u64,
) -> Result<ExpirationReport, LifecycleError> {
    lifecycle::expire_elapsed(lifecycle, limiter, core_batch_time)
}

/// Discards terminal signed bytes while preserving every unresolved submission.
///
/// # Errors
///
/// Returns `Unavailable` when the lifecycle records cannot be locked.
pub fn retention_sweep(
    lifecycle: &PreparationLifecycle,
    current_sequence: u64,
    retention_sequences: u64,
) -> Result<RetentionReport, LifecycleError> {
    lifecycle::sweep_retention(lifecycle, current_sequence, retention_sequences)
}

/// Constructs and canonically encodes an unsigned activity from core state.
///
/// # Errors
///
/// Refuses zero-valued defaults, unavailable or unverified core state, a stale account sequence, a
/// timestamp bound that excludes core time or widens past the configured span, and an oversized
/// payload. Also returns the payload, envelope, wire, or disclosure failure raised while building
/// the canonical form.
pub fn prepare_activity(
    boundary: &mut dyn CorePreparationBoundary,
    defaults: PreparationDefaults,
    request: PrepareRequest,
) -> Result<Prepared, PrepareError> {
    if defaults.timestamp_span == 0 || defaults.maximum_payload_bytes == 0 {
        return Err(PrepareError::InvalidDefaults);
    }
    let state = boundary
        .preparation_state(&request.actor)
        .map_err(PrepareError::Core)?;
    if let Some(expected) = request.expected_account_sequence {
        if expected != state.account_sequence {
            return Err(PrepareError::StaleSequence {
                expected,
                core: state.account_sequence,
            });
        }
    }
    let timestamp_bound = match request.timestamp_bound {
        Some(bound) => {
            if bound.not_before() > state.protocol_timestamp
                || bound.not_after() < state.protocol_timestamp
            {
                return Err(PrepareError::TimestampDoesNotContainCore);
            }
            if bound.not_after().saturating_sub(bound.not_before()) > defaults.timestamp_span {
                return Err(PrepareError::TimestampBoundWidened);
            }
            bound
        }
        None => TimestampBound::new(
            state.protocol_timestamp,
            state
                .protocol_timestamp
                .saturating_add(defaults.timestamp_span),
        )
        .map_err(PrepareError::Activity)?,
    };
    let maximum_payload = defaults
        .maximum_payload_bytes
        .min(request.declared_payload_limit);
    if request.payload.len() > maximum_payload {
        return Err(PrepareError::PayloadLimitExceeded {
            actual: request.payload.len(),
            maximum: maximum_payload,
        });
    }
    let payload = Payload::new(
        &state.module_registry,
        request.activity_type,
        &request.payload,
    )
    .map_err(PrepareError::Payload)?;
    let payload_hash = payload_hash_for(&payload).map_err(PrepareError::Wire)?;
    let mut builder = EnvelopeBuilder::new();
    builder
        .protocol_version(1)
        .and_then(|builder| builder.network_id(state.network_id))
        .and_then(|builder| builder.activity_type(request.activity_type))
        .and_then(|builder| builder.actor_did(request.actor))
        .and_then(|builder| builder.authority(request.authority))
        .and_then(|builder| builder.account_sequence(state.account_sequence))
        .and_then(|builder| builder.timestamp_bound(timestamp_bound))
        .and_then(|builder| builder.idempotency_key(request.idempotency_key))
        .and_then(|builder| builder.fee_limit(request.fee_limit.unwrap_or(defaults.fee_limit)))
        .and_then(|builder| builder.payload_hash(payload_hash))
        .and_then(|builder| builder.payload(payload))
        .map_err(PrepareError::Activity)?;
    let envelope = builder.build().map_err(PrepareError::Activity)?;
    let canonical_bytes = encode_unsigned_envelope(&envelope).map_err(PrepareError::Wire)?;
    let signing_preimage = *preimage_unsigned(&envelope)
        .map_err(PrepareError::Wire)?
        .as_bytes();
    let disclosed =
        disclose(&canonical_bytes, &state.module_registry).map_err(PrepareError::Disclosure)?;
    let audit = PreparationAuditEntry {
        idempotency_key: envelope.idempotency_key().bytes(),
        observed_head_sequence: state.observed_head_sequence,
        disclosure_digest: disclosed.digest,
    };
    Ok(Prepared {
        envelope,
        canonical_bytes,
        signing_preimage,
        observed_head_sequence: state.observed_head_sequence,
        disclosure: disclosed.disclosure,
        disclosure_digest: disclosed.digest,
        audit,
    })
}
