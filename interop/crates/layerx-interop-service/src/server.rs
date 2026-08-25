use crate::config::{
    decode_hex, parse_hex32, Ap2KeyPin, Config, FiatProviderPin, VisaAgentPin, VisaTargetPin,
};
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use ed25519_dalek::{Signature, Verifier as _, VerifyingKey as Ed25519Key};
use layerx_ap2::{
    KeyResolver, KeyUse, MandateMode, MandateVerifier, ProtectedHeader, VerificationContext,
};
use layerx_fiat::{
    EvidenceClass, ExternalId, FiatAdapter, FiatJourneyState, FiatRail, ProviderEvidence,
    ProviderVerifier, TokenReference, VerifiedProviderFacts,
};
use layerx_interop_gateway::principal::PrincipalId;
use layerx_interop_gateway::server::{
    interop_gateway_routes, ExternalState, HostedAdapter, IngressTransport, InteropRoute,
};
use layerx_interop_gateway::trace::TraceId;
use layerx_platform_gateway::http::{IncomingRequest, OutgoingResponse};
use layerx_platform_gateway::store::{
    KeyRecord, Reservation, TapCredentialRecord, TapNonceConsumption,
};
use layerx_platform_gateway::{
    authenticate_gateway_key, gateway_audit_event, gateway_digest, verify_activity_operation,
    verify_submission, AccessError, AuthorityFacts,
};
use layerx_proof::receipt::{verify, AuthorizedBatch};
use layerx_ucp::{
    Capability, CheckoutSubmission, NegotiatedCapabilities, PaymentHandler, PlatformProfile,
    UcpIdempotencyKey,
};
use layerx_visa_tap::{
    prepare_trusted_intent, AgentIntent, AgentPublicKey, CredentialBinding,
    CredentialBindingStore, KeyStatus, RegisteredAgentKey, TapError, TapRequest, TapVerifier,
    TrustedAgentRegistry,
};
use layerx_x402::buyer::{BuyerPaymentPlane, PaymentBuildRequest, SupportedKind};
use layerx_x402::facilitator::FacilitatorRequest;
use layerx_x402::transport::encode_payment_required;
use layerx_x402::transport::{decode_facilitator_request, TransportKind, TransportValue};
use layerx_x402::{Buyer, Facilitator, PaymentRequired, Seller, SettlementResponse};
use p256::ecdsa::VerifyingKey as P256Key;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};
use subtle::ConstantTimeEq;

const MAX_BODY: usize = 512 * 1024;
const FIAT_EVIDENCE_SIGNATURE_DOMAIN: &[u8] =
    b"LayerX/interop/fiat/provider-evidence/v1\0";
const AP2_EXECUTION_KEY_DOMAIN: &[u8] = b"LayerX/interop/ap2/execution-key/v1\0";

pub fn route(config: &Config, request: &IncomingRequest) -> OutgoingResponse {
    if request.headers.contains_key("x-layerx-principal")
        || request.headers.contains_key("x-layerx-api-key")
    {
        return failure(400, "untrusted_identity_header", None);
    }
    let parsed = match interop_gateway_routes(&request.method, &request.path) {
        Ok(route) => route,
        Err(_) => return failure(404, "not_found", None),
    };
    match parsed {
        InteropRoute::Live => live(),
        InteropRoute::Ready => ready(config),
        InteropRoute::AdapterMetadata => metadata(config),
        route => authenticated(config, request, route),
    }
}

fn authenticated(
    config: &Config,
    request: &IncomingRequest,
    route: InteropRoute<'_>,
) -> OutgoingResponse {
    if request.body.len() > MAX_BODY {
        return failure(400, "request_too_large", None);
    }
    let authorization = match request.headers.get("authorization") {
        Some(value) => value,
        None => return failure(401, "api_key_required", None),
    };
    let record = match authenticate_gateway_key(&config.store, authorization) {
        Ok(record) => record,
        Err(AccessError::Unauthenticated) => return failure(401, "api_key_required", None),
        Err(AccessError::PersistenceUnavailable) => {
            return failure(503, "persistence_unavailable", Some(5))
        }
    };
    let trace = trace(request);
    if let InteropRoute::Resume { operation } = &route {
        return match config.store.operation(operation) {
            Ok(Some(stored))
                if stored
                    .principal
                    .as_bytes()
                    .ct_eq(record.principal_digest.as_bytes())
                    .unwrap_u8()
                    == 1 =>
            {
                stored_response(&stored.state, &stored.response, &trace, operation)
            }
            Ok(Some(_)) | Ok(None) => failure(404, "operation_not_found", None),
            Err(_) => failure(503, "persistence_unavailable", Some(5)),
        };
    }
    let observed_at = now().unwrap_or(1);
    let adapter = route.adapter().map_or("operation", HostedAdapter::surface);
    let request_digest = gateway_digest(&[
        b"interop-ingress-v1",
        request.method.as_bytes(),
        request.path.as_bytes(),
        &request.body,
    ]);
    let callback_identity = matches!(
        route,
        InteropRoute::Ap2VerifyMandates
            | InteropRoute::Ap2Execute
            | InteropRoute::VisaVerifyIntent
            | InteropRoute::VisaExecuteIntent
            | InteropRoute::FiatCallback { .. }
    );
    let idempotency = if callback_identity {
        request_digest.as_str()
    } else {
        match request.headers.get("idempotency-key") {
            Some(value) if valid_identifier(value, 128) => value,
            _ => return failure(400, "idempotency_key_required", None),
        }
    };
    let scope = gateway_digest(&[
        b"interop-operation-v1",
        record.principal_digest.as_bytes(),
        adapter.as_bytes(),
        idempotency.as_bytes(),
    ]);
    let audit = gateway_audit_event(
        &record.principal_digest,
        "interop_ingress",
        adapter,
        "attempted",
        observed_at,
    );
    match config.store.reserve(
        &record,
        &scope,
        &request_digest,
        observed_at,
        config.idempotency_seconds,
        &request_digest,
        &record.principal_digest,
        &audit,
    ) {
        Ok(Reservation::Revoked) => return failure(401, "api_key_required", None),
        Ok(Reservation::RateLimited {
            retry_after_seconds,
        }) => return failure(429, "quota_exceeded", Some(retry_after_seconds)),
        Ok(Reservation::Existing {
            digest,
            state,
            response,
            principal,
            ..
        }) => {
            if principal
                .as_bytes()
                .ct_eq(record.principal_digest.as_bytes())
                .unwrap_u8()
                != 1
            {
                return failure(404, "operation_not_found", None);
            }
            if digest
                .as_bytes()
                .ct_eq(request_digest.as_bytes())
                .unwrap_u8()
                != 1
            {
                return failure(409, "idempotency_conflict", None);
            }
            if state != "pending" {
                return stored_response(&state, &response, &trace, &scope);
            }
        }
        Ok(Reservation::Reserved) => {}
        Err(_) => return failure(503, "persistence_unavailable", Some(5)),
    }
    let principal = match PrincipalId::new(record.principal_digest.clone()) {
        Ok(value) => value,
        Err(_) => return failure(503, "persistence_unavailable", Some(5)),
    };
    let dispatched = dispatch(config, request, route, &record, &principal, &trace, &scope);
    if dispatched.durable_state == "pending" || dispatched.status == 503 {
        return dispatched.response(&trace, &scope);
    }
    let body = dispatched.body(&trace, &scope);
    let completion_audit = gateway_audit_event(
        &record.principal_digest,
        "interop_ingress",
        adapter,
        dispatched.durable_state,
        observed_at,
    );
    if config
        .store
        .complete(
            &scope,
            &request_digest,
            dispatched.durable_state,
            &hex(&body),
            dispatched.receipt_hex.as_deref().unwrap_or(""),
            dispatched.activity_id.as_deref(),
            &record.principal_digest,
            &completion_audit,
        )
        .is_err()
    {
        return failure(503, "persistence_unavailable", Some(5));
    }
    OutgoingResponse {
        status: dispatched.status,
        body,
        retry_after: None,
    }
}

struct Dispatch {
    status: u16,
    durable_state: &'static str,
    result: Value,
    error: Option<&'static str>,
    receipt_hex: Option<String>,
    activity_id: Option<String>,
}

impl Dispatch {
    fn result(status: u16, state: &'static str, result: Value) -> Self {
        Self {
            status,
            durable_state: state,
            result,
            error: None,
            receipt_hex: None,
            activity_id: None,
        }
    }

    fn error(status: u16, state: &'static str, code: &'static str) -> Self {
        Self {
            status,
            durable_state: state,
            result: Value::Null,
            error: Some(code),
            receipt_hex: None,
            activity_id: None,
        }
    }

    fn body(&self, trace: &TraceId, operation: &str) -> Vec<u8> {
        let value = self.error.map_or_else(
            || json!({ "ok": true, "operation": operation, "result": self.result, "trace": trace.as_str() }),
            |code| json!({ "ok": false, "operation": operation, "error": { "code": code }, "trace": trace.as_str() }),
        );
        value.to_string().into_bytes()
    }

    fn response(&self, trace: &TraceId, operation: &str) -> OutgoingResponse {
        OutgoingResponse {
            status: self.status,
            body: self.body(trace, operation),
            retry_after: None,
        }
    }
}

fn dispatch(
    config: &Config,
    request: &IncomingRequest,
    route: InteropRoute<'_>,
    record: &KeyRecord,
    principal: &PrincipalId,
    trace: &TraceId,
    operation: &str,
) -> Dispatch {
    match route {
        InteropRoute::Resume { .. } => {
            Dispatch::result(202, "pending", json!({ "state": "pending" }))
        }
        InteropRoute::X402Supported { transport } => x402_supported(config, transport),
        InteropRoute::X402BuyerBuild { transport } => {
            x402_buyer(config, request, transport, trace, operation)
        }
        InteropRoute::X402SellerOffer { transport } => x402_seller(request, transport),
        InteropRoute::X402Verify { transport } => {
            x402_verify(config, request, transport, principal, trace)
        }
        InteropRoute::X402Settle { transport } => x402_settle(
            config, request, transport, record, principal, trace, operation,
        ),
        InteropRoute::Ap2VerifyMandates => ap2(config, request, record, trace, operation, false),
        InteropRoute::Ap2Execute => ap2(config, request, record, trace, operation, true),
        InteropRoute::UcpComplete => ucp(config, request, record, principal, trace, operation),
        InteropRoute::VisaVerifyIntent => {
            visa(config, request, record, principal, trace, operation, false)
        }
        InteropRoute::VisaExecuteIntent => {
            visa(config, request, record, principal, trace, operation, true)
        }
        InteropRoute::FiatCallback { adapter } => fiat(
            config, request, record, principal, trace, operation, adapter,
        ),
        InteropRoute::Live | InteropRoute::Ready | InteropRoute::AdapterMetadata => {
            Dispatch::error(404, "refused", "not_found")
        }
    }
}

fn x402_supported(config: &Config, transport: IngressTransport) -> Dispatch {
    match Facilitator::new(config.manifest.x402_supported.clone()) {
        Ok(facilitator) => Dispatch::result(
            200,
            "completed",
            json!({ "transport": transport.label(), "supported": facilitator.supported() }),
        ),
        Err(_) => Dispatch::error(503, "pending", "adapter_configuration_invalid"),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BuyerRequest {
    payment_required: PaymentRequired,
    scheme_payload: Value,
}

fn x402_buyer(
    config: &Config,
    request: &IncomingRequest,
    transport: IngressTransport,
    trace: &TraceId,
    operation: &str,
) -> Dispatch {
    let body: BuyerRequest = match typed_body(request, transport) {
        Ok(value) => value,
        Err(_) => return Dispatch::error(400, "refused", "invalid_x402_buyer_request"),
    };
    let header = match encode_payment_required(TransportKind::Http, &body.payment_required) {
        Ok(TransportValue::HttpHeader { value, .. }) => value,
        _ => return Dispatch::error(400, "refused", "invalid_x402_offer"),
    };
    let supported = config
        .manifest
        .x402_supported
        .kinds
        .iter()
        .map(|kind| SupportedKind {
            scheme: kind.scheme.clone(),
            network: kind.network.clone(),
        })
        .collect();
    let buyer = match Buyer::new(supported) {
        Ok(value) => value,
        Err(_) => return Dispatch::error(503, "pending", "adapter_configuration_invalid"),
    };
    let mut plane = BuyerPlane {
        payload: Some(body.scheme_payload),
    };
    let idempotency = match parse_hex32(operation) {
        Ok(value) => value,
        Err(_) => return Dispatch::error(503, "pending", "operation_identity_invalid"),
    };
    match buyer.build_payment(&header, idempotency, &mut plane, trace) {
        Ok(built) => Dispatch::result(
            200,
            "completed",
            json!({
                "transport": transport.label(),
                "payment_header": built.header,
                "payment_payload": built.payload,
                "idempotency_key": hex(&built.idempotency_key)
            }),
        ),
        Err(_) => Dispatch::error(400, "refused", "x402_buyer_refused"),
    }
}

struct BuyerPlane {
    payload: Option<Value>,
}

impl BuyerPaymentPlane for BuyerPlane {
    fn construct(
        &mut self,
        _request: PaymentBuildRequest,
    ) -> Result<Value, layerx_x402::model::X402Error> {
        self.payload
            .take()
            .filter(Value::is_object)
            .ok_or(layerx_x402::model::X402Error::InvalidPayload)
    }
}

fn x402_seller(request: &IncomingRequest, transport: IngressTransport) -> Dispatch {
    let required: PaymentRequired = match typed_body(request, transport) {
        Ok(value) => value,
        Err(_) => return Dispatch::error(400, "refused", "invalid_x402_offer"),
    };
    let seller = match Seller::new(required) {
        Ok(value) => value,
        Err(_) => return Dispatch::error(400, "refused", "invalid_x402_offer"),
    };
    match seller.payment_required() {
        Ok(signal) => Dispatch::result(
            200,
            "completed",
            json!({
                "transport": transport.label(),
                "status": signal.status,
                "payment_required_header": signal.header,
                "payment_required": signal.body
            }),
        ),
        Err(_) => Dispatch::error(400, "refused", "x402_offer_encoding_refused"),
    }
}

fn x402_request(
    request: &IncomingRequest,
    transport: IngressTransport,
) -> Result<FacilitatorRequest, ()> {
    let value: Value = typed_body(request, transport).map_err(|_| ())?;
    decode_facilitator_request(transport_kind(transport), &TransportValue::Json(value))
        .map_err(|_| ())
}

fn x402_verify(
    config: &Config,
    request: &IncomingRequest,
    transport: IngressTransport,
    principal: &PrincipalId,
    trace: &TraceId,
) -> Dispatch {
    let parsed = match x402_request(request, transport) {
        Ok(value) => value,
        Err(()) => return Dispatch::error(400, "refused", "invalid_x402_request"),
    };
    let facilitator = match Facilitator::new(config.manifest.x402_supported.clone()) {
        Ok(value) => value,
        Err(_) => return Dispatch::error(503, "pending", "adapter_configuration_invalid"),
    };
    let supported = facilitator.supported().kinds.iter().any(|kind| {
        kind.scheme == parsed.payment_requirements.scheme
            && kind.network == parsed.payment_requirements.network
    });
    let intent_bound = parsed
        .payment_payload
        .payload
        .get("layerxActivity")
        .and_then(Value::as_str)
        .is_some();
    Dispatch::result(
        200,
        "completed",
        json!({
            "isValid": supported && intent_bound,
            "invalidReason": if supported && intent_bound { Value::Null } else if supported { json!("typed_intent_required") } else { json!("unsupported_offer") },
            "trace": trace.as_str(),
            "principal": principal.as_str()
        }),
    )
}

fn x402_settle(
    config: &Config,
    request: &IncomingRequest,
    transport: IngressTransport,
    record: &KeyRecord,
    _principal: &PrincipalId,
    trace: &TraceId,
    operation: &str,
) -> Dispatch {
    let parsed = match x402_request(request, transport) {
        Ok(value) => value,
        Err(()) => return Dispatch::error(400, "refused", "invalid_x402_request"),
    };
    let activity = parsed
        .payment_payload
        .payload
        .get("layerxActivity")
        .and_then(Value::as_str)
        .and_then(|value| decode_hex(value, MAX_BODY).ok());
    let protocol_idempotency = parsed
        .payment_payload
        .payload
        .get("layerxIdempotencyKey")
        .and_then(Value::as_str)
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let Some(activity) = activity else {
        return Dispatch::error(400, "refused", "typed_intent_required");
    };
    let Some(protocol_idempotency) = protocol_idempotency else {
        return Dispatch::error(400, "refused", "protocol_idempotency_required");
    };
    let authorization = match request.headers.get("authorization") {
        Some(value) => value,
        None => return Dispatch::error(401, "refused", "api_key_required"),
    };
    let facilitator = match Facilitator::new(config.manifest.x402_supported.clone()) {
        Ok(value) => value,
        Err(_) => return Dispatch::error(503, "pending", "adapter_configuration_invalid"),
    };
    if !facilitator.supported().kinds.iter().any(|kind| {
        kind.scheme == parsed.payment_requirements.scheme
            && kind.network == parsed.payment_requirements.network
    }) {
        return Dispatch::error(400, "refused", "unsupported_x402_offer");
    }
    let execution = Execution {
        config,
        authorization,
        activity,
        idempotency: protocol_idempotency,
        expected_signer: &record.signer_public_key,
        submitted_activity_id: None,
        trace,
    };
    match execution.submit() {
        Ok(PlaneOutcome::Pending) => {
            let response = SettlementResponse {
                success: false,
                error_reason: Some("settlement_pending".to_owned()),
                payer: None,
                transaction: operation.to_owned(),
                network: parsed.payment_requirements.network,
                amount: None,
                extensions: BTreeMap::new(),
            };
            Dispatch::result(202, "pending", json!(response))
        }
        Ok(PlaneOutcome::Refused) => {
            let response = SettlementResponse {
                success: false,
                error_reason: Some("payment_refused".to_owned()),
                payer: None,
                transaction: String::new(),
                network: parsed.payment_requirements.network,
                amount: None,
                extensions: BTreeMap::new(),
            };
            Dispatch::result(200, "refused", json!(response))
        }
        Ok(PlaneOutcome::Executed(evidence)) => {
            let verified = match verify(&evidence.receipt, &evidence.authorized) {
                Ok(value) => value,
                Err(_) => return Dispatch::error(503, "pending", "receipt_verification_failed"),
            };
            let Some(protocol) = verified.receipt().protocol() else {
                return Dispatch::error(503, "pending", "receipt_verification_failed");
            };
            let (asset, recipient) = match parsed.payment_requirements.layerx_facts() {
                Ok(value) => value,
                Err(_) => return Dispatch::error(400, "refused", "unsupported_x402_offer"),
            };
            if protocol.asset() != asset
                || protocol.to() != recipient
                || protocol.amount() != parsed.payment_requirements.amount.value()
            {
                return Dispatch::error(503, "pending", "receipt_intent_mismatch");
            }
            let receipt_digest = evidence.verified.receipt_digest();
            let mut extensions = BTreeMap::new();
            extensions.insert(
                "layerx".to_owned(),
                json!({
                    "receipt": STANDARD.encode(&evidence.receipt),
                    "receiptDigest": hex(&receipt_digest),
                    "verificationLevel": evidence.verified.verification_level()
                }),
            );
            let response = SettlementResponse {
                success: true,
                error_reason: None,
                payer: Some(hex(&protocol.from())),
                transaction: format!("lxp:{}", hex(&receipt_digest)),
                network: parsed.payment_requirements.network,
                amount: Some(parsed.payment_requirements.amount),
                extensions,
            };
            if response.validate_wire().is_err() {
                return Dispatch::error(503, "pending", "settlement_encoding_failed");
            }
            let mut result = Dispatch::result(200, "completed", json!(response));
            result.receipt_hex = Some(hex(&evidence.receipt));
            result.activity_id = Some(hex(&evidence.verified.activity_id()));
            result
        }
        Err(_) => Dispatch::error(503, "pending", "settlement_unavailable"),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ap2Request {
    checkout_presentation: String,
    payment_presentation: String,
    nonce: String,
    #[serde(default)]
    activity: String,
}

fn ap2(
    config: &Config,
    request: &IncomingRequest,
    record: &KeyRecord,
    trace: &TraceId,
    _operation: &str,
    execute: bool,
) -> Dispatch {
    let body: Ap2Request = match direct_body(request) {
        Ok(value) => value,
        Err(_) => return Dispatch::error(400, "refused", "invalid_ap2_request"),
    };
    let resolver = Ap2Resolver {
        keys: &config.manifest.ap2_keys,
    };
    if parse_hex32(&record.principal_digest).is_err()
        || record.principal_digest != record.principal_digest.to_ascii_lowercase()
    {
        return Dispatch::error(503, "pending", "authenticated_principal_invalid");
    }
    let server_now = match now() {
        Ok(value) => value,
        Err(_) => return Dispatch::error(503, "pending", "clock_unavailable"),
    };
    let mut matched = None;
    for binding in config
        .manifest
        .ap2_assets
        .iter()
        .filter(|binding| binding.principal_digest == record.principal_digest)
    {
        let context = VerificationContext {
            now: server_now,
            clock_skew_seconds: 0,
            expected_audience: &binding.audience,
            expected_nonce: &body.nonce,
            currency_minor_exponent: binding.minor_unit_exponent,
            usage: None,
        };
        let Ok(verified) = MandateVerifier::new(&resolver).verify(
            &body.checkout_presentation,
            &body.payment_presentation,
            &context,
        ) else {
            continue;
        };
        if verified.amount().currency() != binding.currency.as_str() {
            continue;
        }
        if matched.replace((binding, verified)).is_some() {
            return Dispatch::error(400, "refused", "asset_binding_ambiguous");
        }
    }
    let Some((binding, verified)) = matched else {
        return Dispatch::error(400, "refused", "mandate_verification_refused");
    };
    if !execute {
        let mode = match verified.mode() {
            MandateMode::Direct => "direct",
            MandateMode::Autonomous => "autonomous",
        };
        return Dispatch::result(
            200,
            "completed",
            json!({
                "state": "mandate-verified",
                "mode": mode,
                "transaction_id": verified.transaction_id(),
                "checkout_id": verified.checkout_id(),
                "currency": verified.amount().currency(),
                "minor_units": verified.amount().minor_units().to_string(),
                "execution_at": verified.execution_at()
            }),
        );
    }
    if verified.amount().currency().len() != 3 || body.activity.is_empty() {
        return Dispatch::error(400, "refused", "typed_intent_required");
    }
    let activity = match decode_hex(&body.activity, MAX_BODY) {
        Ok(value) => value,
        Err(_) => return Dispatch::error(400, "refused", "typed_intent_required"),
    };
    let asset = match parse_hex32(&binding.asset) {
        Ok(value) => value,
        Err(_) => return Dispatch::error(400, "refused", "asset_binding_invalid"),
    };
    let payer = match parse_hex32(&binding.payer_account) {
        Ok(value) => value,
        Err(_) => return Dispatch::error(400, "refused", "account_binding_invalid"),
    };
    let payee = match parse_hex32(&binding.payee_account) {
        Ok(value) => value,
        Err(_) => return Dispatch::error(400, "refused", "account_binding_invalid"),
    };
    let authorization = request
        .headers
        .get("authorization")
        .map(String::as_str)
        .unwrap_or("");
    let activity_idempotency_key = ap2_execution_key(record, &verified);
    let execution = Execution {
        config,
        authorization,
        activity,
        idempotency: &activity_idempotency_key,
        expected_signer: &record.signer_public_key,
        submitted_activity_id: None,
        trace,
    };
    match execution.submit() {
        Ok(PlaneOutcome::Pending) => Dispatch::result(
            202,
            "pending",
            json!({ "state": ExternalState::Pending.label() }),
        ),
        Ok(PlaneOutcome::Refused) => Dispatch::result(
            200,
            "refused",
            json!({ "state": ExternalState::Refused.label() }),
        ),
        Ok(PlaneOutcome::Executed(evidence)) => {
            let receipt = match verify(&evidence.receipt, &evidence.authorized) {
                Ok(value) => value,
                Err(_) => return Dispatch::error(503, "pending", "receipt_verification_failed"),
            };
            let Some(protocol) = receipt.receipt().protocol() else {
                return Dispatch::error(503, "pending", "receipt_verification_failed");
            };
            let atomic_units = match binding.atomic_units_per_minor_unit.parse::<u128>() {
                Ok(value) => value,
                Err(_) => return Dispatch::error(503, "pending", "adapter_configuration_invalid"),
            };
            let amount = match verified.amount().minor_units().checked_mul(atomic_units) {
                Some(value) if value > 0 => value,
                _ => return Dispatch::error(400, "refused", "asset_binding_invalid"),
            };
            if protocol.asset() != asset
                || protocol.from() != payer
                || protocol.to() != payee
                || protocol.amount() != amount
            {
                return Dispatch::error(503, "pending", "receipt_intent_mismatch");
            }
            let mut result = Dispatch::result(
                200,
                "completed",
                json!({
                    "state": ExternalState::ReceiptVerified.label(),
                    "transaction_id": verified.transaction_id(),
                    "checkout_id": verified.checkout_id(),
                    "receipt_digest": hex(&evidence.verified.receipt_digest())
                }),
            );
            result.receipt_hex = Some(hex(&evidence.receipt));
            result.activity_id = Some(hex(&evidence.verified.activity_id()));
            result
        }
        Err(_) => Dispatch::error(503, "pending", "settlement_unavailable"),
    }
}

fn ap2_execution_key(record: &KeyRecord, verified: &layerx_ap2::VerifiedMandates) -> String {
    let mut digest = Sha256::new();
    digest.update(AP2_EXECUTION_KEY_DOMAIN);
    digest.update(record.principal_digest.as_bytes());
    digest.update([0]);
    digest.update(verified.checkout_receipt_reference().as_bytes());
    digest.update([0]);
    digest.update(verified.payment_receipt_reference().as_bytes());
    hex(&digest.finalize())
}

struct Ap2Resolver<'a> {
    keys: &'a [Ap2KeyPin],
}

impl KeyResolver for Ap2Resolver<'_> {
    fn resolve(
        &self,
        usage: KeyUse,
        header: &ProtectedHeader,
    ) -> Result<P256Key, layerx_ap2::Ap2Error> {
        if header.certificate_chain().is_some() {
            return Err(layerx_ap2::Ap2Error::KeyResolution);
        }
        let use_case = match usage {
            KeyUse::CheckoutMandateIssuer => "checkout-mandate",
            KeyUse::PaymentMandateIssuer => "payment-mandate",
            KeyUse::MerchantCheckout => "merchant-checkout",
        };
        let kid = header.key_id().ok_or(layerx_ap2::Ap2Error::KeyResolution)?;
        let pin = self
            .keys
            .iter()
            .find(|pin| pin.use_case == use_case && pin.key_id == kid)
            .ok_or(layerx_ap2::Ap2Error::KeyResolution)?;
        let bytes = decode_hex(&pin.public_key_sec1, 65)
            .map_err(|_| layerx_ap2::Ap2Error::KeyResolution)?;
        P256Key::from_sec1_bytes(&bytes).map_err(|_| layerx_ap2::Ap2Error::KeyResolution)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UcpRequest {
    checkout_id: String,
    currency: String,
    total_minor: String,
    asset: String,
    recipient: String,
    idempotency_key: String,
    profile_url: String,
    handler_id: String,
    handler_version: String,
    handler_spec: String,
    handler_schema: String,
    activity: String,
    order_id: String,
    permalink_url: String,
}

fn ucp(
    config: &Config,
    request: &IncomingRequest,
    record: &KeyRecord,
    _principal: &PrincipalId,
    trace: &TraceId,
    _operation: &str,
) -> Dispatch {
    let body: UcpRequest = match direct_body(request) {
        Ok(value) => value,
        Err(_) => return Dispatch::error(400, "refused", "invalid_ucp_request"),
    };
    let handler = match PaymentHandler::new(
        &body.handler_id,
        &body.handler_version,
        &body.handler_spec,
        &body.handler_schema,
    ) {
        Ok(value) => value,
        Err(_) => return Dispatch::error(400, "refused", "ucp_profile_refused"),
    };
    let checkout = match Capability::new(
        "dev.ucp.shopping.checkout",
        "2026-04-08",
        "https://ucp.dev/2026-04-08/specification/checkout",
        "https://ucp.dev/2026-04-08/schemas/shopping/checkout.json",
    ) {
        Ok(value) => value,
        Err(_) => return Dispatch::error(503, "pending", "adapter_configuration_invalid"),
    };
    let order = match Capability::new(
        "dev.ucp.shopping.order",
        "2026-04-08",
        "https://ucp.dev/2026-04-08/specification/order",
        "https://ucp.dev/2026-04-08/schemas/shopping/order.json",
    ) {
        Ok(value) => value,
        Err(_) => return Dispatch::error(503, "pending", "adapter_configuration_invalid"),
    };
    let platform = PlatformProfile {
        profile_url: body.profile_url,
        capabilities: vec![checkout, order],
        payment_handlers: vec![handler.clone()],
    };
    let negotiated = match NegotiatedCapabilities::negotiate(&platform, &handler) {
        Ok(value) => value,
        Err(_) => return Dispatch::error(400, "refused", "ucp_capability_refused"),
    };
    let currency: [u8; 3] = match body.currency.as_bytes().try_into() {
        Ok(value) => value,
        Err(_) => return Dispatch::error(400, "refused", "ucp_currency_invalid"),
    };
    let submission = CheckoutSubmission {
        checkout_id: body.checkout_id,
        currency,
        total_minor: match body.total_minor.parse::<u128>() {
            Ok(value) => value,
            Err(_) => return Dispatch::error(400, "refused", "ucp_amount_invalid"),
        },
        layerx_asset: match parse_hex32(&body.asset) {
            Ok(value) => value,
            Err(_) => return Dispatch::error(400, "refused", "ucp_asset_invalid"),
        },
        layerx_recipient: match parse_hex32(&body.recipient) {
            Ok(value) => value,
            Err(_) => return Dispatch::error(400, "refused", "ucp_recipient_invalid"),
        },
        idempotency_key: match UcpIdempotencyKey::parse(&body.idempotency_key) {
            Ok(value) => value,
            Err(_) => return Dispatch::error(400, "refused", "ucp_idempotency_invalid"),
        },
        negotiated,
    };
    let activity = match decode_hex(&body.activity, MAX_BODY) {
        Ok(value) => value,
        Err(_) => return Dispatch::error(400, "refused", "typed_intent_required"),
    };
    let authorization = request
        .headers
        .get("authorization")
        .map(String::as_str)
        .unwrap_or("");
    let protocol_idempotency = hex(&submission.idempotency_key.gateway_key());
    let execution = Execution {
        config,
        authorization,
        activity,
        idempotency: &protocol_idempotency,
        expected_signer: &record.signer_public_key,
        submitted_activity_id: None,
        trace,
    };
    match execution.submit() {
        Ok(PlaneOutcome::Pending) => Dispatch::result(
            202,
            "pending",
            json!({ "state": ExternalState::Pending.label() }),
        ),
        Ok(PlaneOutcome::Refused) => Dispatch::result(
            200,
            "refused",
            json!({ "state": ExternalState::Refused.label() }),
        ),
        Ok(PlaneOutcome::Executed(evidence)) => {
            let verified = match verify(&evidence.receipt, &evidence.authorized) {
                Ok(value) => value,
                Err(_) => return Dispatch::error(503, "pending", "receipt_verification_failed"),
            };
            let Some(protocol) = verified.receipt().protocol() else {
                return Dispatch::error(503, "pending", "receipt_verification_failed");
            };
            if protocol.asset() != submission.layerx_asset
                || protocol.to() != submission.layerx_recipient
                || protocol.amount() != submission.total_minor
            {
                return Dispatch::error(503, "pending", "receipt_intent_mismatch");
            }
            if body.order_id.is_empty()
                || body.order_id.len() > 256
                || !body.permalink_url.starts_with("https://")
                || body.permalink_url.len() > 256
            {
                return Dispatch::error(400, "refused", "ucp_order_invalid");
            }
            let receipt_digest = evidence.verified.receipt_digest();
            let mut result = Dispatch::result(
                200,
                "completed",
                json!({
                    "state": ExternalState::ReceiptVerified.label(),
                    "order": {
                        "id": body.order_id, "checkout_id": submission.checkout_id,
                        "permalink_url": body.permalink_url,
                        "currency": String::from_utf8_lossy(&submission.currency),
                        "total_minor": submission.total_minor.to_string(),
                        "receipt_digest": hex(&receipt_digest)
                    }
                }),
            );
            result.receipt_hex = Some(hex(&evidence.receipt));
            result.activity_id = Some(hex(&evidence.verified.activity_id()));
            result
        }
        Err(_) => Dispatch::error(503, "pending", "settlement_unavailable"),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VisaRequest {
    authority: String,
    path: String,
    signature_input: String,
    signature: String,
    #[serde(default)]
    activity: String,
}

struct VisaActivityBinding {
    canonical: Vec<u8>,
    activity_id: [u8; 32],
    idempotency_key: String,
}

fn visa(
    config: &Config,
    request: &IncomingRequest,
    record: &KeyRecord,
    principal: &PrincipalId,
    trace: &TraceId,
    operation: &str,
    execute: bool,
) -> Dispatch {
    let body: VisaRequest = match direct_body(request) {
        Ok(value) => value,
        Err(_) => return Dispatch::error(400, "refused", "invalid_visa_tap_request"),
    };
    let tap = match TapRequest::parse(
        body.authority,
        body.path,
        &body.signature_input,
        &body.signature,
    ) {
        Ok(value) => value,
        Err(_) => return Dispatch::error(400, "refused", "visa_tap_refused"),
    };
    let target = match config
        .manifest
        .visa_targets
        .iter()
        .find(|target| target.principal_digest.as_str() == record.principal_digest)
    {
        Some(value) => value,
        None => return Dispatch::error(400, "refused", "visa_tap_target_unavailable"),
    };
    if require_visa_target(&tap, target).is_err() {
        return Dispatch::error(400, "refused", "visa_tap_target_mismatch");
    }
    let registry = VisaRegistry {
        pins: &config.manifest.visa_agents,
    };
    let observed_at = match now() {
        Ok(value) => value,
        Err(_) => return Dispatch::error(503, "pending", "server_clock_unavailable"),
    };
    let verified = match TapVerifier::verify_credential(
        &tap,
        &registry,
        observed_at,
        config.tap_clock_skew_seconds,
    ) {
        Ok(value) => value,
        Err(_) => return Dispatch::error(400, "refused", "visa_tap_refused"),
    };
    let layerx_agent = match verified.layerx_agent {
        Some(value) => value,
        None => return Dispatch::error(400, "refused", "layerx_agent_binding_required"),
    };
    let signer_public_key = match parse_hex32(&record.signer_public_key) {
        Ok(value) => value,
        Err(_) => return Dispatch::error(503, "pending", "persistence_unavailable"),
    };
    if let Err(code) = require_visa_actor(layerx_agent, signer_public_key) {
        return Dispatch::error(400, "refused", code);
    }
    if let Err(code) = require_visa_route(execute, verified.intent, &body.activity) {
        return Dispatch::error(400, "refused", code);
    }
    let activity_binding = if execute {
        let canonical = match decode_hex(&body.activity, MAX_BODY) {
            Ok(value) if !value.is_empty() => value,
            _ => return Dispatch::error(400, "refused", "typed_intent_required"),
        };
        let submission = match verify_submission(
            &canonical,
            &config.modules,
            config.protocol_version,
            config.protocol_network_id,
            &signer_public_key,
        ) {
            Ok(value) => value,
            Err(_) => return Dispatch::error(400, "refused", "activity_authorization_refused"),
        };
        Some(VisaActivityBinding {
            canonical,
            activity_id: submission.activity_id(),
            idempotency_key: hex(&submission.idempotency_key()),
        })
    } else {
        None
    };
    let replay_until = match verified
        .expires_at
        .checked_add(config.tap_clock_skew_seconds)
    {
        Some(value) if value > observed_at => value,
        _ => return Dispatch::error(400, "refused", "visa_tap_refused"),
    };
    let tap_audit = gateway_audit_event(
        &record.principal_digest,
        "visa_tap_nonce",
        &verified.key_id,
        "attempted",
        observed_at,
    );
    let mut bindings = DurableVisaBinding {
        store: &config.store,
        nonce: tap.nonce(),
        intent: verified.intent,
        activity_id: activity_binding.as_ref().map(|binding| binding.activity_id),
        signer_public_key,
        target_authority: tap.authority(),
        target_path: tap.path(),
        operation_identity: operation,
        credential_expires_at: verified.expires_at,
        replay_until,
        consumed_at: observed_at,
        audit_event: &tap_audit,
    };
    let intent =
        match prepare_trusted_intent(principal, layerx_agent, &verified, &mut bindings, trace) {
            Ok(value) => value,
            Err(TapError::Replay) => {
                return Dispatch::error(409, "refused", "visa_tap_replayed")
            }
            Err(TapError::StorageRefused) => {
                return Dispatch::error(503, "pending", "persistence_unavailable")
            }
            Err(_) => return Dispatch::error(400, "refused", "visa_tap_refused"),
        };
    if !execute {
        return Dispatch::result(
            200,
            "completed",
            json!({
                "state": "credential-verified", "agent_id": intent.trusted_agent_id,
                "layerx_agent": hex(&intent.layerx_agent), "credential_evidence": hex(&intent.credential_evidence)
            }),
        );
    }
    let activity = match activity_binding {
        Some(value) => value,
        None => return Dispatch::error(400, "refused", "typed_intent_required"),
    };
    let activity_id = activity.activity_id;
    let idempotency_key = activity.idempotency_key;
    let canonical = activity.canonical;
    let authorization = request
        .headers
        .get("authorization")
        .map(String::as_str)
        .unwrap_or("");
    match (Execution {
        config,
        authorization,
        activity: canonical,
        idempotency: &idempotency_key,
        expected_signer: &record.signer_public_key,
        submitted_activity_id: Some(activity_id),
        trace,
    })
    .submit()
    {
        Ok(PlaneOutcome::Pending) => Dispatch::result(
            202,
            "pending",
            json!({ "state": ExternalState::Pending.label() }),
        ),
        Ok(PlaneOutcome::Refused) => Dispatch::result(
            200,
            "refused",
            json!({ "state": ExternalState::Refused.label() }),
        ),
        Ok(PlaneOutcome::Executed(evidence)) => {
            let mut result = Dispatch::result(
                200,
                "completed",
                json!({ "state": ExternalState::ReceiptVerified.label(), "receipt_digest": hex(&evidence.verified.receipt_digest()) }),
            );
            result.receipt_hex = Some(hex(&evidence.receipt));
            result.activity_id = Some(hex(&evidence.verified.activity_id()));
            result
        }
        Err(_) => Dispatch::error(503, "pending", "settlement_unavailable"),
    }
}

fn require_visa_actor(
    layerx_agent: [u8; 32],
    signer_public_key: [u8; 32],
) -> Result<(), &'static str> {
    if layerx_agent == signer_public_key {
        Ok(())
    } else {
        Err("layerx_agent_signer_mismatch")
    }
}

fn require_visa_target(tap: &TapRequest, target: &VisaTargetPin) -> Result<(), &'static str> {
    if tap.authority() == target.authority.as_str() && tap.path() == target.path.as_str() {
        Ok(())
    } else {
        Err("visa_tap_target_mismatch")
    }
}

fn require_visa_route(
    execute: bool,
    intent: AgentIntent,
    activity: &str,
) -> Result<(), &'static str> {
    if execute && intent != AgentIntent::Pay {
        return Err("payer_credential_required");
    }
    if execute && activity.is_empty() {
        return Err("typed_intent_required");
    }
    if !execute && !activity.is_empty() {
        return Err("unexpected_activity_decoration");
    }
    Ok(())
}

struct VisaRegistry<'a> {
    pins: &'a [VisaAgentPin],
}

impl TrustedAgentRegistry for VisaRegistry<'_> {
    fn resolve(
        &self,
        key_id: &str,
        now: u64,
    ) -> Result<RegisteredAgentKey, layerx_visa_tap::TapError> {
        let pin = self
            .pins
            .iter()
            .find(|pin| pin.key_id == key_id)
            .ok_or(layerx_visa_tap::TapError::UnknownKey)?;
        let status = match pin.status.as_str() {
            "active" => KeyStatus::Active,
            "revoked" => return Err(layerx_visa_tap::TapError::Revoked),
            _ => return Err(layerx_visa_tap::TapError::RegistryUnavailable),
        };
        if pin.expires_at <= now {
            return Err(layerx_visa_tap::TapError::ExpiredKey);
        }
        let key = match pin.algorithm.as_str() {
            "ed25519" => AgentPublicKey::Ed25519(
                parse_hex32(&pin.public_key)
                    .map_err(|_| layerx_visa_tap::TapError::RegistryUnavailable)?,
            ),
            "rsa-pss-sha256" => AgentPublicKey::RsaPssSha256Pem(
                STANDARD
                    .decode(&pin.public_key)
                    .map_err(|_| layerx_visa_tap::TapError::RegistryUnavailable)?,
            ),
            _ => return Err(layerx_visa_tap::TapError::RegistryUnavailable),
        };
        Ok(RegisteredAgentKey {
            key_id: pin.key_id.clone(),
            agent_id: pin.agent_id.clone(),
            agent_domain: pin.agent_domain.clone(),
            layerx_agent: Some(
                parse_hex32(&pin.layerx_agent)
                    .map_err(|_| layerx_visa_tap::TapError::RegistryUnavailable)?,
            ),
            key,
            status,
            expires_at: pin.expires_at,
        })
    }
}

struct DurableVisaBinding<'a> {
    store: &'a layerx_platform_gateway::store::RedisStore,
    nonce: &'a str,
    intent: AgentIntent,
    activity_id: Option<[u8; 32]>,
    signer_public_key: [u8; 32],
    target_authority: &'a str,
    target_path: &'a str,
    operation_identity: &'a str,
    credential_expires_at: u64,
    replay_until: u64,
    consumed_at: u64,
    audit_event: &'a str,
}

impl CredentialBindingStore for DurableVisaBinding<'_> {
    fn put(
        &mut self,
        principal: &PrincipalId,
        binding: &CredentialBinding,
        _trace: &TraceId,
    ) -> Result<(), TapError> {
        let record = TapCredentialRecord {
            principal_digest: principal.as_str().to_owned(),
            key_id: binding.key_id.clone(),
            layerx_agent: hex(&binding.layerx_agent),
            trusted_agent_id: binding.trusted_agent_id.clone(),
            trusted_agent_domain: binding.trusted_agent_domain.clone(),
            intent: match self.intent {
                AgentIntent::Browse => "browse",
                AgentIntent::Pay => "pay",
            }
            .to_owned(),
            evidence_digest: hex(&binding.evidence_digest),
            activity_id: self.activity_id.map(|value| hex(&value)),
            signer_public_key: hex(&self.signer_public_key),
            target_authority: self.target_authority.to_owned(),
            target_path: self.target_path.to_owned(),
            operation_identity: self.operation_identity.to_owned(),
            credential_expires_at: self.credential_expires_at,
        };
        match self.store.consume_tap_nonce(
            &binding.key_id,
            self.nonce,
            &record,
            self.consumed_at,
            self.replay_until,
            self.audit_event,
        ) {
            Ok(
                TapNonceConsumption::Consumed { .. }
                | TapNonceConsumption::AlreadyConsumed { .. },
            ) => Ok(()),
            Ok(TapNonceConsumption::Replay) => Err(TapError::Replay),
            Err(_) => Err(TapError::StorageRefused),
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FiatFacts {
    provider: String,
    settlement: String,
    token_reference_sha256: String,
    rail: String,
    class: String,
    amount: String,
    asset: String,
    destination: String,
    observed_at: u64,
    hold_until: Option<u64>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FiatEvidenceEnvelope {
    facts: FiatFacts,
    signature: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FiatCallback {
    token_reference: String,
    evidence: FiatEvidenceEnvelope,
    #[serde(default)]
    activity: String,
}

fn fiat(
    config: &Config,
    request: &IncomingRequest,
    record: &KeyRecord,
    _principal: &PrincipalId,
    trace: &TraceId,
    _operation: &str,
    adapter: HostedAdapter,
) -> Dispatch {
    let body: FiatCallback = match direct_body(request) {
        Ok(value) => value,
        Err(_) => return Dispatch::error(400, "refused", "invalid_provider_callback"),
    };
    let token = match TokenReference::new(body.token_reference.into_bytes()) {
        Ok(value) => value,
        Err(_) => return Dispatch::error(400, "refused", "provider_callback_refused"),
    };
    let evidence_bytes = match serde_json::to_vec(&body.evidence) {
        Ok(value) => value,
        Err(_) => return Dispatch::error(400, "refused", "provider_callback_refused"),
    };
    let evidence = match ProviderEvidence::new(evidence_bytes) {
        Ok(value) => value,
        Err(_) => return Dispatch::error(400, "refused", "provider_callback_refused"),
    };
    let verifier = FiatEvidenceVerifier {
        pins: &config.manifest.fiat_providers,
        expected_rail: adapter,
    };
    let activity = decode_hex(&body.activity, MAX_BODY)
        .ok()
        .filter(|value| !value.is_empty());
    let authorization = request
        .headers
        .get("authorization")
        .map(String::as_str)
        .unwrap_or("");
    let facts = match FiatAdapter::verify_evidence(&token, &evidence, &verifier, trace) {
        Ok(value) => value,
        Err(_) => return Dispatch::error(400, "refused", "provider_callback_refused"),
    };
    match facts.class {
        EvidenceClass::Authorised => fiat_state(FiatJourneyState::AuthorisedHold {
            until: facts.hold_until.unwrap_or(facts.observed_at),
        }),
        EvidenceClass::Clearing => fiat_state(FiatJourneyState::ClearingHold {
            until: facts.hold_until.unwrap_or(facts.observed_at),
        }),
        EvidenceClass::Settled | EvidenceClass::Reversed | EvidenceClass::Chargeback => {
            let Some(activity) = activity else {
                return Dispatch::error(400, "refused", "typed_intent_required");
            };
            let protocol_idempotency = hex(&FiatAdapter::idempotency_key(&facts));
            let execution = Execution {
                config,
                authorization,
                activity,
                idempotency: &protocol_idempotency,
                expected_signer: &record.signer_public_key,
                submitted_activity_id: None,
                trace,
            };
            match execution.submit() {
                Ok(PlaneOutcome::Pending) => fiat_state(match facts.class {
                    EvidenceClass::Settled => FiatJourneyState::CreditPending,
                    EvidenceClass::Reversed => FiatJourneyState::ReversalPending {
                        hold_until: facts.hold_until,
                    },
                    EvidenceClass::Chargeback => FiatJourneyState::ChargebackPending {
                        hold_until: facts.hold_until,
                    },
                    EvidenceClass::Authorised | EvidenceClass::Clearing => {
                        FiatJourneyState::Refused
                    }
                }),
                Ok(PlaneOutcome::Refused) => fiat_state(FiatJourneyState::Refused),
                Ok(PlaneOutcome::Executed(executed)) => {
                    let verified = match verify(&executed.receipt, &executed.authorized) {
                        Ok(value) => value,
                        Err(_) => {
                            return Dispatch::error(503, "pending", "receipt_verification_failed")
                        }
                    };
                    let Some(protocol) = verified.receipt().protocol() else {
                        return Dispatch::error(503, "pending", "receipt_verification_failed");
                    };
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
                        return Dispatch::error(503, "pending", "receipt_intent_mismatch");
                    }
                    let digest = executed.verified.receipt_digest();
                    let state = match facts.class {
                        EvidenceClass::Settled => FiatJourneyState::Credited {
                            receipt_digest: digest,
                        },
                        EvidenceClass::Reversed => FiatJourneyState::Reversed {
                            receipt_digest: digest,
                        },
                        EvidenceClass::Chargeback => FiatJourneyState::ChargedBack {
                            receipt_digest: digest,
                        },
                        EvidenceClass::Authorised | EvidenceClass::Clearing => {
                            FiatJourneyState::Refused
                        }
                    };
                    let mut result = fiat_state(state);
                    result.receipt_hex = Some(hex(&executed.receipt));
                    result.activity_id = Some(hex(&executed.verified.activity_id()));
                    result
                }
                Err(_) => Dispatch::error(503, "pending", "settlement_unavailable"),
            }
        }
    }
}

fn fiat_state(state: FiatJourneyState) -> Dispatch {
    match state {
        FiatJourneyState::AuthorisedHold { until } => Dispatch::result(
            200,
            "completed",
            json!({ "state": "authorised-hold", "until": until }),
        ),
        FiatJourneyState::ClearingHold { until } => Dispatch::result(
            200,
            "completed",
            json!({ "state": "clearing-hold", "until": until }),
        ),
        FiatJourneyState::CreditPending => Dispatch::result(
            202,
            "pending",
            json!({ "state": ExternalState::Pending.label() }),
        ),
        FiatJourneyState::ReversalPending { hold_until } => Dispatch::result(
            202,
            "pending",
            json!({ "state": ExternalState::ReversalPending.label(), "hold_until": hold_until }),
        ),
        FiatJourneyState::ChargebackPending { hold_until } => Dispatch::result(
            202,
            "pending",
            json!({ "state": "chargeback-pending", "hold_until": hold_until }),
        ),
        FiatJourneyState::Credited { receipt_digest } => Dispatch::result(
            200,
            "completed",
            json!({ "state": ExternalState::ReceiptVerified.label(), "receipt_digest": hex(&receipt_digest) }),
        ),
        FiatJourneyState::Reversed { receipt_digest } => Dispatch::result(
            200,
            "completed",
            json!({ "state": ExternalState::Reversed.label(), "receipt_digest": hex(&receipt_digest) }),
        ),
        FiatJourneyState::ChargedBack { receipt_digest } => Dispatch::result(
            200,
            "completed",
            json!({ "state": "charged-back", "receipt_digest": hex(&receipt_digest) }),
        ),
        FiatJourneyState::Refused => Dispatch::result(
            200,
            "refused",
            json!({ "state": ExternalState::Refused.label() }),
        ),
    }
}

struct FiatEvidenceVerifier<'a> {
    pins: &'a [FiatProviderPin],
    expected_rail: HostedAdapter,
}

impl ProviderVerifier for FiatEvidenceVerifier<'_> {
    fn verify(
        &self,
        token: &TokenReference,
        evidence: &ProviderEvidence,
        _trace: &TraceId,
    ) -> Result<VerifiedProviderFacts, layerx_fiat::FiatError> {
        let envelope: FiatEvidenceEnvelope = serde_json::from_slice(evidence.canonical())
            .map_err(|_| layerx_fiat::FiatError::InvalidEvidence)?;
        let pin = self
            .pins
            .iter()
            .find(|pin| pin.provider == envelope.facts.provider)
            .ok_or(layerx_fiat::FiatError::InvalidEvidence)?;
        let public = parse_hex32(&pin.public_key_ed25519)
            .map_err(|_| layerx_fiat::FiatError::InvalidEvidence)?;
        let key =
            Ed25519Key::from_bytes(&public).map_err(|_| layerx_fiat::FiatError::InvalidEvidence)?;
        let signature_bytes = decode_hex(&envelope.signature, 64)
            .map_err(|_| layerx_fiat::FiatError::InvalidEvidence)?;
        let signature = Signature::from_slice(&signature_bytes)
            .map_err(|_| layerx_fiat::FiatError::InvalidEvidence)?;
        let canonical = serde_json::to_vec(&envelope.facts)
            .map_err(|_| layerx_fiat::FiatError::InvalidEvidence)?;
        let mut signed = Vec::with_capacity(FIAT_EVIDENCE_SIGNATURE_DOMAIN.len() + canonical.len());
        signed.extend_from_slice(FIAT_EVIDENCE_SIGNATURE_DOMAIN);
        signed.extend_from_slice(&canonical);
        key.verify(&signed, &signature)
            .map_err(|_| layerx_fiat::FiatError::InvalidEvidence)?;
        let expected_token_digest = parse_hex32(&envelope.facts.token_reference_sha256)
            .map_err(|_| layerx_fiat::FiatError::InvalidEvidence)?;
        let actual_token_digest: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        if expected_token_digest
            .ct_eq(&actual_token_digest)
            .unwrap_u8()
            != 1
        {
            return Err(layerx_fiat::FiatError::InvalidEvidence);
        }
        let rail = match envelope.facts.rail.as_str() {
            "card" if self.expected_rail == HostedAdapter::FiatCard => FiatRail::Card,
            "bank" if self.expected_rail == HostedAdapter::FiatBank => FiatRail::Bank,
            "rtp" if self.expected_rail == HostedAdapter::FiatRtp => FiatRail::RealTimePayment,
            _ => return Err(layerx_fiat::FiatError::InvalidEvidence),
        };
        let class = match envelope.facts.class.as_str() {
            "authorised" => EvidenceClass::Authorised,
            "clearing" => EvidenceClass::Clearing,
            "settled" => EvidenceClass::Settled,
            "reversed" => EvidenceClass::Reversed,
            "chargeback" => EvidenceClass::Chargeback,
            _ => return Err(layerx_fiat::FiatError::InvalidEvidence),
        };
        Ok(VerifiedProviderFacts {
            provider: ExternalId::new(envelope.facts.provider)?,
            settlement: ExternalId::new(envelope.facts.settlement)?,
            rail,
            class,
            amount: envelope
                .facts
                .amount
                .parse()
                .map_err(|_| layerx_fiat::FiatError::InvalidEvidence)?,
            asset: parse_hex32(&envelope.facts.asset)
                .map_err(|_| layerx_fiat::FiatError::InvalidEvidence)?,
            destination: parse_hex32(&envelope.facts.destination)
                .map_err(|_| layerx_fiat::FiatError::InvalidEvidence)?,
            observed_at: envelope.facts.observed_at,
            hold_until: envelope.facts.hold_until,
        })
    }
}

struct Execution<'a> {
    config: &'a Config,
    authorization: &'a str,
    activity: Vec<u8>,
    idempotency: &'a str,
    expected_signer: &'a str,
    submitted_activity_id: Option<[u8; 32]>,
    trace: &'a TraceId,
}

enum PlaneOutcome {
    Pending,
    Refused,
    Executed(ExecutionEvidence),
}

struct ExecutionEvidence {
    receipt: Vec<u8>,
    authorized: AuthorizedBatch,
    verified: layerx_platform_gateway::VerifiedOperation,
}

impl Execution<'_> {
    fn submit(&self) -> Result<PlaneOutcome, String> {
        if parse_hex32(self.expected_signer).is_err() {
            return Err("authenticated signer binding is invalid".to_owned());
        }
        let upstream = self.config.client.request_authorized_traced(
            &self.config.hosted_gateway,
            "POST",
            "/v1/activities",
            self.authorization,
            Some(self.idempotency),
            "application/octet-stream",
            &self.activity,
            Some(self.trace.as_str()),
        )?;
        if upstream.status == 202 {
            return Ok(PlaneOutcome::Pending);
        }
        if (400..500).contains(&upstream.status) {
            return Ok(PlaneOutcome::Refused);
        }
        if upstream.status != 200 || upstream.content_type != "application/json" {
            return Err("hosted gateway is unavailable".to_owned());
        }
        let response: HostedActivity = serde_json::from_slice(&upstream.body)
            .map_err(|_| "hosted gateway response is invalid".to_owned())?;
        if !response.ok || response.result.receipt.is_empty() {
            return Err("hosted gateway response lacks receipt evidence".to_owned());
        }
        let receipt = decode_hex(&response.result.receipt, MAX_BODY)?;
        let authority = authority(self.config, &response.result.activity_id, self.trace)?;
        let authorized = AuthorizedBatch::new(
            authority.batch_id,
            authority.asset,
            authority.previous_state_root,
            authority.resulting_state_root,
            authority.sequencer_public_key,
        );
        let facts = AuthorityFacts::new(
            authority.batch_id,
            authority.asset,
            authority.previous_state_root,
            authority.resulting_state_root,
            authority.sequencer_public_key,
        );
        let expected = parse_hex32(&response.result.activity_id)?;
        let verified = verify_activity_operation(
            &receipt,
            facts,
            &self.config.trusted_sequencer_key,
            Some(expected),
        )
        .map_err(|_| "independent receipt verification failed".to_owned())?;
        if let Some(submitted) = self.submitted_activity_id {
            require_receipt_activity(submitted, verified.activity_id())?;
        }
        let independently_verified = verify(&receipt, &authorized)
            .map_err(|_| "independent receipt verification failed".to_owned())?;
        let protocol = independently_verified
            .receipt()
            .protocol()
            .ok_or_else(|| "independent receipt verification failed".to_owned())?;
        if !response
            .result
            .batch_id
            .eq_ignore_ascii_case(&hex(&protocol.batch_id()))
            || response.result.global_sequence != protocol.global_sequence()
            || response.result.result_code != protocol.result_code()
            || !response
                .result
                .state_root
                .eq_ignore_ascii_case(&hex(&protocol.resulting_state_root()))
        {
            return Err("hosted gateway response conflicts with verified receipt".to_owned());
        }
        Ok(PlaneOutcome::Executed(ExecutionEvidence {
            receipt,
            authorized,
            verified,
        }))
    }
}

fn require_receipt_activity(
    submitted_activity_id: [u8; 32],
    receipt_activity_id: [u8; 32],
) -> Result<(), String> {
    if submitted_activity_id == receipt_activity_id {
        Ok(())
    } else {
        Err("verified receipt does not match the submitted activity".to_owned())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HostedActivity {
    ok: bool,
    result: HostedResult,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HostedResult {
    activity_id: String,
    batch_id: String,
    global_sequence: u64,
    result_code: i32,
    state_root: String,
    receipt: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorityResponse {
    activity_id: String,
    batch_id: String,
    asset: String,
    previous_state_root: String,
    resulting_state_root: String,
    sequencer_public_key: String,
    network_id: String,
    wire_version: String,
}

struct AuthorizedFacts {
    batch_id: [u8; 32],
    asset: [u8; 32],
    previous_state_root: [u8; 32],
    resulting_state_root: [u8; 32],
    sequencer_public_key: [u8; 32],
}

fn authority(
    config: &Config,
    activity_id: &str,
    trace: &TraceId,
) -> Result<AuthorizedFacts, String> {
    let authorization = format!("Bearer {}", config.receipt_authority_token.as_str());
    let response = config.client.request_authorized_traced(
        &config.receipt_authority,
        "GET",
        &format!("/v1/authorized-batches/by-activity/{activity_id}"),
        &authorization,
        None,
        "application/json",
        &[],
        Some(trace.as_str()),
    )?;
    if response.status != 200 || response.content_type != "application/json" {
        return Err("receipt authority is unavailable".to_owned());
    }
    let facts: AuthorityResponse = serde_json::from_slice(&response.body)
        .map_err(|_| "receipt authority response is invalid".to_owned())?;
    if !facts.activity_id.eq_ignore_ascii_case(activity_id)
        || facts.network_id != config.network_id
        || facts.wire_version != config.wire_version
    {
        return Err("receipt authority scope mismatch".to_owned());
    }
    Ok(AuthorizedFacts {
        batch_id: parse_hex32(&facts.batch_id)?,
        asset: parse_hex32(&facts.asset)?,
        previous_state_root: parse_hex32(&facts.previous_state_root)?,
        resulting_state_root: parse_hex32(&facts.resulting_state_root)?,
        sequencer_public_key: parse_hex32(&facts.sequencer_public_key)?,
    })
}

fn direct_body<T: DeserializeOwned>(request: &IncomingRequest) -> Result<T, ()> {
    if request.headers.get("content-type").map(String::as_str) != Some("application/json") {
        return Err(());
    }
    serde_json::from_slice(&request.body).map_err(|_| ())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct McpEnvelope<T> {
    jsonrpc: String,
    method: String,
    params: T,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct A2aEnvelope<T> {
    task_id: String,
    context_id: String,
    input: T,
}

fn typed_body<T: DeserializeOwned>(
    request: &IncomingRequest,
    transport: IngressTransport,
) -> Result<T, ()> {
    match transport {
        IngressTransport::Http => direct_body(request),
        IngressTransport::Mcp => {
            let envelope: McpEnvelope<T> = direct_body(request)?;
            if envelope.jsonrpc != "2.0" || !envelope.method.starts_with("layerx.x402.") {
                return Err(());
            }
            Ok(envelope.params)
        }
        IngressTransport::A2a => {
            let envelope: A2aEnvelope<T> = direct_body(request)?;
            if !valid_identifier(&envelope.task_id, 128)
                || !valid_identifier(&envelope.context_id, 128)
            {
                return Err(());
            }
            Ok(envelope.input)
        }
    }
}

fn transport_kind(transport: IngressTransport) -> TransportKind {
    match transport {
        IngressTransport::Http => TransportKind::Http,
        IngressTransport::Mcp => TransportKind::Mcp,
        IngressTransport::A2a => TransportKind::A2a,
    }
}

fn live() -> OutgoingResponse {
    json_response(
        200,
        json!({ "status": "live", "service": "layerx-interop-gateway", "package_semver": env!("CARGO_PKG_VERSION") }),
    )
}

fn ready(config: &Config) -> OutgoingResponse {
    let durable = config.store.ready();
    let hosted = hosted_ready(config);
    let authority = dependency_ready(
        config,
        &config.receipt_authority,
        config.receipt_authority_token.as_str(),
    );
    let ready = durable && hosted && authority;
    json_response(
        if ready { 200 } else { 503 },
        json!({
            "status": if ready { "ready" } else { "degraded" },
            "components": { "durable_gateway_store": readiness(durable), "hosted_gateway": readiness(hosted), "receipt_authority": readiness(authority) }
        }),
    )
}

fn metadata(config: &Config) -> OutgoingResponse {
    let durable = config.store.ready();
    let hosted = hosted_ready(config);
    let authority = dependency_ready(
        config,
        &config.receipt_authority,
        config.receipt_authority_token.as_str(),
    );
    let adapters: Vec<_> = config.manifest.adapters.values().map(|registered| {
        let descriptor = &registered.descriptor;
        json!({
            "id": descriptor.id().as_str(), "specification": descriptor.spec().protocol().as_str(),
            "version": descriptor.spec().version().as_str(), "specification_sha256": hex(&descriptor.spec().document_digest()),
            "conformance_suite": descriptor.conformance().suite().as_str(), "conformance_vectors": descriptor.conformance().vector_count(),
            "conformance_sha256": hex(&descriptor.conformance().suite_digest()), "evidence_policy": registered.evidence.label(),
            "readiness": { "ingress": readiness(durable), "settlement": readiness(hosted), "receipt_verification": readiness(authority) }
        })
    }).collect();
    let transports: Vec<_> = config.manifest.transports.values().map(|pin| json!({
        "id": pin.id, "version": pin.version, "specification_sha256": pin.specification_sha256,
        "conformance_sha256": pin.conformance_sha256
    })).collect();
    json_response(
        200,
        json!({ "adapters": adapters, "transports": transports }),
    )
}

fn dependency_ready(
    config: &Config,
    endpoint: &layerx_platform_gateway::http::Endpoint,
    token: &str,
) -> bool {
    config
        .client
        .request(
            endpoint,
            "GET",
            "/readyz",
            token,
            None,
            "application/json",
            &[],
        )
        .is_ok_and(|response| response.status == 200 && response.content_type == "application/json")
}

fn hosted_ready(config: &Config) -> bool {
    let Ok(response) = config.client.request(
        &config.hosted_gateway,
        "GET",
        "/v1/status",
        "readiness",
        None,
        "application/json",
        &[],
    ) else {
        return false;
    };
    let Ok(status) = serde_json::from_slice::<Value>(&response.body) else {
        return false;
    };
    response.status == 200
        && response.content_type == "application/json"
        && status
            .pointer("/services/hosted_gateway")
            .and_then(Value::as_str)
            != Some("unavailable")
        && status
            .pointer("/services/testnet_core")
            .and_then(Value::as_str)
            == Some("available")
        && status
            .pointer("/services/receipt_authority")
            .and_then(Value::as_str)
            == Some("available")
}

const fn readiness(value: bool) -> &'static str {
    if value {
        "ready"
    } else {
        "unavailable"
    }
}

fn stored_response(
    state: &str,
    stored: &str,
    trace: &TraceId,
    operation: &str,
) -> OutgoingResponse {
    match decode_hex(stored, MAX_BODY) {
        Ok(body) if !body.is_empty() => OutgoingResponse {
            status: 200,
            body,
            retry_after: None,
        },
        _ if state == "pending" => json_response(
            202,
            json!({ "ok": true, "operation": operation, "result": { "state": "pending" }, "trace": trace.as_str() }),
        ),
        _ => failure(503, "persistence_unavailable", Some(5)),
    }
}

fn failure(status: u16, code: &str, retry_after: Option<u64>) -> OutgoingResponse {
    OutgoingResponse {
        status,
        body: json!({ "ok": false, "error": { "code": code } })
            .to_string()
            .into_bytes(),
        retry_after,
    }
}

fn json_response(status: u16, value: Value) -> OutgoingResponse {
    OutgoingResponse {
        status,
        body: value.to_string().into_bytes(),
        retry_after: None,
    }
}

fn trace(request: &IncomingRequest) -> TraceId {
    let mut digest = Sha256::new();
    digest.update(request.method.as_bytes());
    digest.update([0]);
    digest.update(request.path.as_bytes());
    digest.update([0]);
    digest.update(&request.body);
    digest.update(now().unwrap_or(1).to_be_bytes());
    let output = digest.finalize();
    let mut entropy = [0_u8; 16];
    entropy.copy_from_slice(&output[..16]);
    TraceId::from_inbound(
        request.headers.get("x-trace-id").map(String::as_str),
        entropy,
    )
}

fn now() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .map_err(|_| "system clock precedes Unix epoch".to_owned())
}

fn valid_identifier(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(char::from(DIGITS[usize::from(byte >> 4)]));
        value.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer as _, SigningKey};

    fn parsed_tap(authority: &str, path: &str) -> TapRequest {
        TapRequest::parse(
            authority,
            path,
            "sig2=(\"@authority\" \"@path\");created=1;keyid=\"tap-key-1\";alg=\"Ed25519\";expires=100;nonce=\"target-nonce\";tag=\"agent-payer-auth\"",
            "sig2=:AA==:",
        )
        .unwrap_or_else(|error| panic!("TAP target fixture must parse: {error}"))
    }

    fn visa_pin(status: &str, expires_at: u64) -> VisaAgentPin {
        VisaAgentPin {
            key_id: "tap-key-1".to_owned(),
            agent_id: "trusted-agent-1".to_owned(),
            agent_domain: "https://agent.example".to_owned(),
            layerx_agent: "11".repeat(32),
            algorithm: "ed25519".to_owned(),
            public_key: "22".repeat(32),
            status: status.to_owned(),
            expires_at,
        }
    }

    fn visa_wire() -> serde_json::Value {
        serde_json::json!({
            "authority": "shop.example",
            "path": "/checkout",
            "signature_input": "sig2=(\"@authority\" \"@path\")",
            "signature": "sig2=:AA==:"
        })
    }

    fn signed_fiat_evidence(
        signing: &SigningKey,
        token: &TokenReference,
    ) -> ProviderEvidence {
        let facts = FiatFacts {
            provider: "provider-001".to_owned(),
            settlement: "settlement-001".to_owned(),
            token_reference_sha256: hex(&Sha256::digest(token.as_bytes())),
            rail: "card".to_owned(),
            class: "settled".to_owned(),
            amount: "10000".to_owned(),
            asset: "41".repeat(32),
            destination: "42".repeat(32),
            observed_at: 1_700_000_000,
            hold_until: None,
        };
        let canonical = serde_json::to_vec(&facts).expect("fiat facts serialize");
        let mut signed = FIAT_EVIDENCE_SIGNATURE_DOMAIN.to_vec();
        signed.extend_from_slice(&canonical);
        let signature = signing.sign(&signed);
        ProviderEvidence::new(
            serde_json::to_vec(&FiatEvidenceEnvelope {
                facts,
                signature: hex(&signature.to_bytes()),
            })
            .expect("fiat evidence serializes"),
        )
        .expect("fiat evidence is bounded")
    }

    #[test]
    fn visa_wire_has_no_caller_time_or_trust_override() {
        assert!(serde_json::from_value::<VisaRequest>(visa_wire()).is_ok());
        let mut caller_time = visa_wire();
        caller_time["now"] = serde_json::json!(u64::MAX);
        assert!(serde_json::from_value::<VisaRequest>(caller_time).is_err());
        let mut trust_override = visa_wire();
        trust_override["verified"] = serde_json::json!(true);
        assert!(serde_json::from_value::<VisaRequest>(trust_override).is_err());
    }

    #[test]
    fn unrelated_activity_decoration_and_browse_execution_are_refused() {
        assert_eq!(
            require_visa_route(false, AgentIntent::Browse, "00"),
            Err("unexpected_activity_decoration")
        );
        assert_eq!(
            require_visa_route(true, AgentIntent::Browse, "00"),
            Err("payer_credential_required")
        );
    }

    #[test]
    fn visa_target_refuses_cross_authority_and_cross_path_credentials() {
        let target = VisaTargetPin {
            principal_digest: "33".repeat(32),
            authority: "shop.example".to_owned(),
            path: "/checkout".to_owned(),
        };
        assert!(require_visa_target(&parsed_tap("shop.example", "/checkout"), &target).is_ok());
        assert!(require_visa_target(&parsed_tap("other.example", "/checkout"), &target).is_err());
        assert!(require_visa_target(&parsed_tap("shop.example", "/other"), &target).is_err());
    }

    #[test]
    fn visa_registry_preserves_revocation_and_expiry_instead_of_synthesizing_active() {
        let revoked = vec![visa_pin("revoked", 100)];
        let revoked_registry = VisaRegistry { pins: &revoked };
        assert_eq!(
            TapVerifier::verify_credential(
                &parsed_tap("shop.example", "/checkout"),
                &revoked_registry,
                1,
                0,
            ),
            Err(TapError::Revoked)
        );
        let expired = vec![visa_pin("active", 1)];
        let expired_registry = VisaRegistry { pins: &expired };
        assert_eq!(
            TapVerifier::verify_credential(
                &parsed_tap("shop.example", "/checkout"),
                &expired_registry,
                2,
                0,
            ),
            Err(TapError::ExpiredKey)
        );
        assert_eq!(
            revoked_registry.resolve("unknown-key", 1),
            Err(TapError::UnknownKey)
        );
    }

    #[test]
    fn tap_agent_must_be_the_authenticated_activity_signer() {
        assert!(require_visa_actor([0x41; 32], [0x41; 32]).is_ok());
        assert_eq!(
            require_visa_actor([0x41; 32], [0x42; 32]),
            Err("layerx_agent_signer_mismatch")
        );
    }

    #[test]
    fn receipt_must_match_the_exact_submitted_activity() {
        assert!(require_receipt_activity([0x51; 32], [0x51; 32]).is_ok());
        assert!(require_receipt_activity([0x51; 32], [0x52; 32]).is_err());
    }

    #[test]
    fn fiat_provider_signature_binds_the_opaque_token_reference() {
        let signing = SigningKey::from_bytes(&[0x61; 32]);
        let token = TokenReference::new(b"provider-token-a".to_vec())
            .expect("opaque provider token is valid");
        let substituted = TokenReference::new(b"provider-token-b".to_vec())
            .expect("substitute provider token is valid");
        let evidence = signed_fiat_evidence(&signing, &token);
        let pins = [FiatProviderPin {
            provider: "provider-001".to_owned(),
            public_key_ed25519: hex(signing.verifying_key().as_bytes()),
        }];
        let verifier = FiatEvidenceVerifier {
            pins: &pins,
            expected_rail: HostedAdapter::FiatCard,
        };
        let trace = TraceId::mint([0x62; 16]);
        assert!(verifier.verify(&token, &evidence, &trace).is_ok());
        assert_eq!(
            verifier.verify(&substituted, &evidence, &trace),
            Err(layerx_fiat::FiatError::InvalidEvidence)
        );
    }

    #[test]
    fn fiat_callback_refuses_caller_selected_economic_identity() {
        let callback = serde_json::json!({
            "token_reference": "provider-token-a",
            "evidence": {
                "facts": {
                    "provider": "provider-001",
                    "settlement": "settlement-001",
                    "token_reference_sha256": "11".repeat(32),
                    "rail": "card",
                    "class": "settled",
                    "amount": "10000",
                    "asset": "41".repeat(32),
                    "destination": "42".repeat(32),
                    "observed_at": 1_700_000_000,
                    "hold_until": null
                },
                "signature": "22".repeat(64)
            },
            "activity": "00",
            "activity_idempotency_key": "33".repeat(32)
        });
        assert!(serde_json::from_value::<FiatCallback>(callback).is_err());
    }

    #[test]
    fn ap2_request_refuses_caller_clock_audience_and_economic_identity() {
        let mut request = serde_json::json!({
            "checkout_presentation": "checkout",
            "payment_presentation": "payment",
            "nonce": "merchant-issued-nonce",
            "activity": "00"
        });
        assert!(serde_json::from_value::<Ap2Request>(request.clone()).is_ok());
        for field in [
            "now",
            "clock_skew_seconds",
            "audience",
            "currency_minor_exponent",
            "activity_idempotency_key",
        ] {
            request[field] = serde_json::json!(1);
            assert!(serde_json::from_value::<Ap2Request>(request.clone()).is_err());
            request.as_object_mut().expect("AP2 request is an object").remove(field);
        }
    }
}
