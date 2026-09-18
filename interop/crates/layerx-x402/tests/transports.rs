//! x402 v2 transport-binding conformance. The HTTP, MCP and A2A role messages
//! are the first-party suites under `interop/specs/conformance/transport-*`,
//! which the gateway deployment pins by digest; every record is a wire document
//! that this harness runs through the production encode and decode paths. The
//! fault-injected settlement cases below drive a live sequencer signer and a
//! canonical receipt encoder, so they stay in Rust and are not vectors.

use std::collections::BTreeMap;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use ed25519_dalek::{Signer as _, SigningKey};
use layerx_interop_gateway::adapter::{
    AdapterDescriptor, AdapterId, ConformanceSuite, PinnedSpec, SpecVersion,
};
use layerx_interop_gateway::gateway::TranslationStatus;
use layerx_interop_gateway::principal::PrincipalId;
use layerx_interop_gateway::trace::TraceId;
use layerx_interop_gateway::GatewayCore;
use layerx_proof::receipt::AuthorizedBatch;
use layerx_wire::encode::Encoder;
use layerx_wire::hash::receipt_digest;
use layerx_wire::limits::PROTOCOL_VERSION;
use layerx_x402::facilitator::{
    Facilitator, FacilitatorKind, FacilitatorPaymentRequest, FacilitatorPlane, FacilitatorRequest,
    FacilitatorSettlementOutcome, PlaneVerifyOutcome, SettlementIdentity, SettlementStep,
    SupportedResponse, VerifyResponse,
};
use layerx_x402::model::{
    AtomicAmount, PaymentPayload, PaymentRequired, PaymentRequirements, SettlementResponse,
    X402Error, PAYMENT_REQUIRED_HEADER, PAYMENT_RESPONSE_HEADER, PAYMENT_SIGNATURE_HEADER,
    X402_VERSION,
};
use layerx_x402::seller::ExecutedPayment;
use layerx_x402::transport::{
    decode_facilitator_request, decode_facilitator_settlement, decode_payment_payload,
    decode_payment_required, decode_settlement, decode_supported_response, decode_verify_response,
    encode_facilitator_request, encode_facilitator_settlement, encode_payment_payload,
    encode_payment_required, encode_settlement, encode_supported_response, encode_verify_response,
    TransportKind, TransportValue, TRANSPORT_MATRIX,
};
use serde_json::json;

const TRANSPORTS: [TransportKind; 3] =
    [TransportKind::Http, TransportKind::Mcp, TransportKind::A2a];

const HTTP_ROLE_SUITE: &str =
    include_str!("../../../specs/conformance/transport-http/role-messages.json");
const MCP_ROLE_SUITE: &str =
    include_str!("../../../specs/conformance/transport-mcp/role-messages.json");
const A2A_ROLE_SUITE: &str =
    include_str!("../../../specs/conformance/transport-a2a/role-messages.json");

const ROLES: [&str; 7] = [
    "payment-required",
    "payment-payload",
    "settlement-response",
    "facilitator-request",
    "verify-response",
    "facilitator-settlement",
    "supported-response",
];

fn requirements() -> PaymentRequirements {
    PaymentRequirements {
        scheme: "exact".to_owned(),
        network: "layerx:testnet".to_owned(),
        amount: AtomicAmount::from_u128(25),
        asset: "05".repeat(32),
        pay_to: "07".repeat(32),
        max_timeout_seconds: 60,
        extra: None,
    }
}

fn payload() -> PaymentPayload {
    PaymentPayload {
        x402_version: X402_VERSION,
        resource: None,
        payload: json!({"authorization": "opaque-signed-payment"}),
        accepted: requirements(),
        extensions: BTreeMap::new(),
    }
}

fn facilitator_request() -> FacilitatorRequest {
    FacilitatorRequest {
        x402_version: X402_VERSION,
        payment_payload: payload(),
        payment_requirements: requirements(),
    }
}

struct RoleVector {
    name: String,
    role: String,
    accepted: bool,
    literal: TransportValue,
}

fn suite(transport: TransportKind) -> (&'static str, &'static str, &'static str) {
    match transport {
        TransportKind::Http => (
            HTTP_ROLE_SUITE,
            "http",
            "interop/specs/conformance/transport-http/role-messages.json",
        ),
        TransportKind::Mcp => (
            MCP_ROLE_SUITE,
            "mcp",
            "interop/specs/conformance/transport-mcp/role-messages.json",
        ),
        TransportKind::A2a => (
            A2A_ROLE_SUITE,
            "a2a",
            "interop/specs/conformance/transport-a2a/role-messages.json",
        ),
    }
}

fn payment_header(declared: &str, source: &str) -> &'static str {
    match declared {
        PAYMENT_REQUIRED_HEADER => PAYMENT_REQUIRED_HEADER,
        PAYMENT_SIGNATURE_HEADER => PAYMENT_SIGNATURE_HEADER,
        PAYMENT_RESPONSE_HEADER => PAYMENT_RESPONSE_HEADER,
        other => panic!("{source}: {other} is not an x402 v2 payment header"),
    }
}

fn role_vectors(transport: TransportKind) -> Vec<RoleVector> {
    let (text, name, source) = suite(transport);
    let records: Vec<serde_json::Value> =
        serde_json::from_str(text).unwrap_or_else(|error| panic!("{source}: {error}"));
    assert!(
        !records.is_empty(),
        "{source}: a suite that carries no vector is not a conformance suite"
    );
    records
        .into_iter()
        .map(|record| {
            let vector = record["name"]
                .as_str()
                .unwrap_or_else(|| panic!("{source}: every vector is named"))
                .to_owned();
            assert_eq!(
                record["transport"].as_str(),
                Some(name),
                "{source}: {vector} declares another transport binding"
            );
            let role = record["role"]
                .as_str()
                .unwrap_or_else(|| panic!("{source}: {vector} names no role message"))
                .to_owned();
            let accepted = record["accepted"].as_bool().unwrap_or_else(|| {
                panic!("{source}: {vector} declares whether the transport accepts it")
            });
            let document = record
                .get("document")
                .cloned()
                .unwrap_or_else(|| panic!("{source}: {vector} carries no wire document"));
            let literal = match record["representation"].as_str() {
                Some("http-header") => TransportValue::HttpHeader {
                    name: payment_header(
                        record["header"]
                            .as_str()
                            .unwrap_or_else(|| panic!("{source}: {vector} names no header")),
                        source,
                    ),
                    value: STANDARD.encode(
                        serde_json::to_vec(&document)
                            .unwrap_or_else(|error| panic!("{source}: {vector}: {error}")),
                    ),
                },
                Some("json") => TransportValue::Json(document),
                other => panic!("{source}: {vector} declares the representation {other:?}"),
            };
            RoleVector {
                name: vector,
                role,
                accepted,
                literal,
            }
        })
        .collect()
}

macro_rules! round_trip {
    ($model:ty, $encode:path, $decode:path, $transport:expr, $vector:expr, $source:expr) => {{
        let name = &$vector.name;
        if !$vector.accepted {
            assert!(
                $decode($transport, &$vector.literal).is_err(),
                "{}: {name}: this representation must be refused on this transport",
                $source
            );
        } else {
            let document = match &$vector.literal {
                TransportValue::Json(value) => value.clone(),
                TransportValue::HttpHeader { value, .. } => serde_json::from_slice(
                    &STANDARD
                        .decode(value.as_bytes())
                        .unwrap_or_else(|error| panic!("{}: {name}: {error}", $source)),
                )
                .unwrap_or_else(|error| panic!("{}: {name}: {error}", $source)),
            };
            let parsed: $model = serde_json::from_value(document)
                .unwrap_or_else(|error| panic!("{}: {name}: {error}", $source));
            let encoded = $encode($transport, &parsed)
                .unwrap_or_else(|error| panic!("{}: {name}: encode: {error}", $source));
            match (&encoded, &$vector.literal) {
                (
                    TransportValue::HttpHeader { name: produced, .. },
                    TransportValue::HttpHeader {
                        name: declared,
                        value: _,
                    },
                ) => assert_eq!(produced, declared, "{}: {name}: header name", $source),
                (TransportValue::Json(_), TransportValue::Json(_)) => assert_eq!(
                    &encoded, &$vector.literal,
                    "{}: {name}: JSON representation",
                    $source
                ),
                _ => panic!(
                    "{}: {name}: the transport encoding is not the declared representation",
                    $source
                ),
            }
            assert_eq!(
                $decode($transport, &encoded),
                Ok(parsed.clone()),
                "{}: {name}: produced representation",
                $source
            );
            assert_eq!(
                $decode($transport, &$vector.literal),
                Ok(parsed),
                "{}: {name}: declared representation",
                $source
            );
        }
    }};
}

#[test]
fn every_role_round_trips_on_every_transport() {
    for transport in TRANSPORTS {
        let (_, _, source) = suite(transport);
        let vectors = role_vectors(transport);
        for role in ROLES {
            assert!(
                vectors
                    .iter()
                    .any(|vector| vector.role == role && vector.accepted),
                "{source}: {role} is not covered"
            );
        }
        for vector in &vectors {
            match vector.role.as_str() {
                "payment-required" => round_trip!(
                    PaymentRequired,
                    encode_payment_required,
                    decode_payment_required,
                    transport,
                    vector,
                    source
                ),
                "payment-payload" => round_trip!(
                    PaymentPayload,
                    encode_payment_payload,
                    decode_payment_payload,
                    transport,
                    vector,
                    source
                ),
                "settlement-response" => round_trip!(
                    SettlementResponse,
                    encode_settlement,
                    decode_settlement,
                    transport,
                    vector,
                    source
                ),
                "facilitator-request" => round_trip!(
                    FacilitatorRequest,
                    encode_facilitator_request,
                    decode_facilitator_request,
                    transport,
                    vector,
                    source
                ),
                "verify-response" => round_trip!(
                    VerifyResponse,
                    encode_verify_response,
                    decode_verify_response,
                    transport,
                    vector,
                    source
                ),
                "facilitator-settlement" => round_trip!(
                    SettlementResponse,
                    encode_facilitator_settlement,
                    decode_facilitator_settlement,
                    transport,
                    vector,
                    source
                ),
                "supported-response" => round_trip!(
                    SupportedResponse,
                    encode_supported_response,
                    decode_supported_response,
                    transport,
                    vector,
                    source
                ),
                other => panic!("{source}: {other} is not an x402 v2 role message"),
            }
        }
    }
    assert!(TRANSPORT_MATRIX
        .iter()
        .all(|row| row.buyer && row.seller && row.facilitator));
}

#[test]
fn settlement_identity_is_transport_independent_and_step_separated() {
    let principal =
        PrincipalId::new("merchant-a").unwrap_or_else(|error| panic!("principal: {error:?}"));
    let request = facilitator_request();
    let stable = [9; 32];
    let baseline = SettlementIdentity::derive(&principal, &request, stable, SettlementStep::Single)
        .unwrap_or_else(|error| panic!("identity: {error}"));
    for transport in TRANSPORTS {
        let encoded = encode_facilitator_request(transport, &request)
            .unwrap_or_else(|error| panic!("encode: {error}"));
        let decoded = decode_facilitator_request(transport, &encoded)
            .unwrap_or_else(|error| panic!("decode: {error}"));
        assert_eq!(
            SettlementIdentity::derive(&principal, &decoded, stable, SettlementStep::Single),
            Ok(baseline)
        );
    }
    let deposit =
        SettlementIdentity::derive(&principal, &request, stable, SettlementStep::EscrowDeposit)
            .unwrap_or_else(|error| panic!("deposit identity: {error}"));
    let charge =
        SettlementIdentity::derive(&principal, &request, stable, SettlementStep::EscrowCharge)
            .unwrap_or_else(|error| panic!("charge identity: {error}"));
    assert_ne!(baseline.idempotency_key, deposit.idempotency_key);
    assert_ne!(deposit.idempotency_key, charge.idempotency_key);
    assert_ne!(deposit.request_digest, charge.request_digest);
}

// --- Fault-injected settlement: exactly-once economic effect -----------------
//
// The facilitator's only successful settlement path terminates in a
// gateway-verified canonical LayerX receipt. These tests drive that real path
// with a signed receipt whose economic facts match the transport-independent
// `requirements()` above (asset `05..`, recipient `07..`, amount 25) and inject
// faults - delayed confirmation, a crashed downstream, duplicate delivery and a
// swapped receipt - to prove the economic effect lands exactly once regardless
// of how many times settlement is retried or redelivered.

const SEQUENCER_SEED: [u8; 32] = [3; 32];
const PRIMARY_ACTIVITY: [u8; 32] = [1; 32];
const ALTERNATE_ACTIVITY: [u8; 32] = [12; 32];

fn stable_identity() -> [u8; 32] {
    [9; 32]
}

fn trace() -> TraceId {
    TraceId::mint([0xa5; 16])
}

fn supported() -> SupportedResponse {
    SupportedResponse {
        kinds: vec![FacilitatorKind {
            x402_version: X402_VERSION,
            scheme: "exact".to_owned(),
            network: "layerx:testnet".to_owned(),
            extra: None,
        }],
        extensions: Vec::new(),
        signers: BTreeMap::from([(
            "layerx:*".to_owned(),
            vec!["did:layerx:facilitator".to_owned()],
        )]),
    }
}

fn facilitator() -> Facilitator {
    Facilitator::new(supported()).unwrap_or_else(|error| panic!("facilitator: {error}"))
}

fn register_x402(gateway: &mut GatewayCore, trace: &TraceId) {
    let adapter = AdapterId::new("x402").unwrap_or_else(|error| panic!("adapter id: {error}"));
    let version = SpecVersion::parse("2").unwrap_or_else(|error| panic!("version: {error}"));
    let spec = PinnedSpec::new(adapter.clone(), version, [0x7d; 32])
        .unwrap_or_else(|error| panic!("pinned spec: {error}"));
    let suite_id = AdapterId::new("x402-v2").unwrap_or_else(|error| panic!("suite id: {error}"));
    let conformance = ConformanceSuite::new(suite_id, 20, [0xc0; 32])
        .unwrap_or_else(|error| panic!("conformance suite: {error}"));
    let descriptor = AdapterDescriptor::new(adapter, spec, conformance);
    gateway
        .register_adapter(descriptor, trace, 0)
        .unwrap_or_else(|error| panic!("register x402 adapter: {error}"));
}

#[derive(Clone)]
struct ReceiptFields {
    activity_id: [u8; 32],
    previous_state_root: [u8; 32],
    resulting_state_root: [u8; 32],
    batch_id: [u8; 32],
    asset: [u8; 32],
    amount: u128,
    from: [u8; 32],
    from_before: u128,
    from_after: u128,
    to: [u8; 32],
    to_before: u128,
    to_after: u128,
}

fn receipt_fields(activity_id: [u8; 32]) -> ReceiptFields {
    ReceiptFields {
        activity_id,
        previous_state_root: [2; 32],
        resulting_state_root: [3; 32],
        batch_id: [4; 32],
        asset: [5; 32],
        amount: 25,
        from: [6; 32],
        from_before: 100,
        from_after: 75,
        to: [7; 32],
        to_before: 10,
        to_after: 35,
    }
}

fn encode_fields(fields: &ReceiptFields, signature: Option<[u8; 64]>) -> Vec<u8> {
    let mut encoder = Encoder::new(4096);
    assert_eq!(
        encoder.structure_header_version(0x5201, PROTOCOL_VERSION),
        Ok(())
    );
    assert_eq!(encoder.u16(PROTOCOL_VERSION), Ok(()));
    assert_eq!(encoder.bytes(&fields.activity_id, 32), Ok(()));
    assert_eq!(encoder.u64(9), Ok(()));
    assert_eq!(encoder.bytes(&fields.previous_state_root, 32), Ok(()));
    assert_eq!(encoder.bytes(&fields.resulting_state_root, 32), Ok(()));
    assert_eq!(encoder.bytes(&[8; 32], 32), Ok(()));
    assert_eq!(encoder.i32(0), Ok(()));
    assert_eq!(encoder.sequence_length(0, 512), Ok(()));
    assert_eq!(encoder.u128(1), Ok(()));
    assert_eq!(encoder.bytes(&fields.batch_id, 32), Ok(()));
    assert_eq!(encoder.u16(1), Ok(()));
    assert_eq!(encoder.u32(1), Ok(()));
    assert_eq!(encoder.u32(1), Ok(()));
    assert_eq!(encoder.u8(1), Ok(()));
    assert_eq!(encoder.bytes(&fields.asset, 32), Ok(()));
    assert_eq!(encoder.u128(fields.amount), Ok(()));
    assert_eq!(encoder.bytes(&fields.from, 32), Ok(()));
    assert_eq!(encoder.u128(fields.from_before), Ok(()));
    assert_eq!(encoder.u128(fields.from_after), Ok(()));
    assert_eq!(encoder.u64(1), Ok(()));
    assert_eq!(encoder.bytes(&fields.to, 32), Ok(()));
    assert_eq!(encoder.u128(fields.to_before), Ok(()));
    assert_eq!(encoder.u128(fields.to_after), Ok(()));
    assert_eq!(encoder.bytes(&[9; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&[10; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&[11; 32], 32), Ok(()));
    assert_eq!(encoder.u64(1_000), Ok(()));
    assert_eq!(encoder.u8(u8::from(signature.is_some())), Ok(()));
    if let Some(value) = signature {
        assert_eq!(encoder.bytes(&value, 64), Ok(()));
    }
    encoder.finish()
}

fn sign(fields: &ReceiptFields, signing_key: &SigningKey) -> Vec<u8> {
    let unsigned = encode_fields(fields, None);
    let digest = receipt_digest(&unsigned)
        .unwrap_or_else(|error| panic!("receipt hashing failed: {error:?}"));
    encode_fields(fields, Some(signing_key.sign(&digest).to_bytes()))
}

fn authorised(fields: &ReceiptFields, signing_key: &SigningKey) -> AuthorizedBatch {
    AuthorizedBatch::new(
        fields.batch_id,
        fields.asset,
        fields.previous_state_root,
        fields.resulting_state_root,
        signing_key.verifying_key().to_bytes(),
    )
}

fn executed(signing_key: &SigningKey, activity_id: [u8; 32]) -> ExecutedPayment {
    let fields = receipt_fields(activity_id);
    ExecutedPayment {
        canonical_receipt: sign(&fields, signing_key),
        authorised_batch: authorised(&fields, signing_key),
    }
}

/// One scripted plane behaviour per settlement attempt. Anything past the end
/// of the script defaults to a confirmed primary settlement, modelling a
/// downstream that recovers and then keeps confirming under retry.
#[derive(Clone, Copy)]
enum Planned {
    /// Settlement accepted but not yet confirmed on chain.
    Pending,
    /// The settlement authority crashed or timed out mid-call.
    Transient,
    /// Confirmed with the canonical primary receipt.
    Execute,
    /// Confirmed, but with a *different* receipt for the same identity.
    ExecuteAlternate,
}

struct FaultInjectingPlane {
    signing_key: SigningKey,
    script: Vec<Planned>,
    cursor: usize,
    settle_calls: usize,
}

impl FaultInjectingPlane {
    fn new(script: Vec<Planned>) -> Self {
        Self {
            signing_key: SigningKey::from_bytes(&SEQUENCER_SEED),
            script,
            cursor: 0,
            settle_calls: 0,
        }
    }
}

impl FacilitatorPlane for FaultInjectingPlane {
    fn verify(
        &mut self,
        _request: &FacilitatorPaymentRequest,
        _trace: &TraceId,
    ) -> Result<PlaneVerifyOutcome, X402Error> {
        Ok(PlaneVerifyOutcome::Valid {
            payer: None,
            extra: None,
        })
    }

    fn settle(
        &mut self,
        _request: FacilitatorPaymentRequest,
        _trace: &TraceId,
    ) -> Result<FacilitatorSettlementOutcome, X402Error> {
        self.settle_calls += 1;
        let planned = self
            .script
            .get(self.cursor)
            .copied()
            .unwrap_or(Planned::Execute);
        self.cursor += 1;
        match planned {
            Planned::Pending => Ok(FacilitatorSettlementOutcome::Pending {
                transaction: "pending:settlement-in-flight".to_owned(),
            }),
            Planned::Transient => Err(X402Error::Decode),
            Planned::Execute => Ok(FacilitatorSettlementOutcome::Executed(executed(
                &self.signing_key,
                PRIMARY_ACTIVITY,
            ))),
            Planned::ExecuteAlternate => Ok(FacilitatorSettlementOutcome::Executed(executed(
                &self.signing_key,
                ALTERNATE_ACTIVITY,
            ))),
        }
    }
}

fn principal() -> PrincipalId {
    PrincipalId::new("merchant-a").unwrap_or_else(|error| panic!("principal: {error:?}"))
}

fn settled_digest(gateway: &GatewayCore, principal: &PrincipalId, key: [u8; 32]) -> [u8; 32] {
    match gateway.translation(principal, key) {
        Some(TranslationStatus::ReceiptVerified { receipt_digest }) => receipt_digest,
        other => panic!("expected a receipt-verified settlement, found {other:?}"),
    }
}

#[test]
fn confirmed_settlement_records_exactly_one_receipt_verified_effect() {
    let facilitator = facilitator();
    let mut gateway = GatewayCore::new();
    let trace = trace();
    register_x402(&mut gateway, &trace);
    let principal = principal();
    let request = facilitator_request();
    let stable = stable_identity();
    let step = SettlementStep::Single;
    let identity = SettlementIdentity::derive(&principal, &request, stable, step)
        .unwrap_or_else(|error| panic!("identity: {error}"));

    let mut plane = FaultInjectingPlane::new(vec![Planned::Execute]);
    let response = facilitator
        .settle(
            &mut gateway,
            &principal,
            &request,
            stable,
            step,
            &mut plane,
            &trace,
            0,
        )
        .unwrap_or_else(|error| panic!("settle: {error}"));

    assert!(response.success);
    assert!(response.transaction.starts_with("lxp:"));
    assert_eq!(response.amount, Some(AtomicAmount::from_u128(25)));
    let digest = settled_digest(&gateway, &principal, identity.idempotency_key);
    assert_eq!(response.transaction, format!("lxp:{}", hex(&digest)));
    assert_eq!(plane.settle_calls, 1);
}

#[test]
fn duplicate_delivery_of_the_same_settlement_does_not_double_charge() {
    let facilitator = facilitator();
    let mut gateway = GatewayCore::new();
    let trace = trace();
    register_x402(&mut gateway, &trace);
    let principal = principal();
    let request = facilitator_request();
    let stable = stable_identity();
    let step = SettlementStep::Single;
    let identity = SettlementIdentity::derive(&principal, &request, stable, step)
        .unwrap_or_else(|error| panic!("identity: {error}"));

    // Every transport delivers the same economic request under one identity, so
    // three redeliveries drive three confirmed plane calls.
    let mut plane =
        FaultInjectingPlane::new(vec![Planned::Execute, Planned::Execute, Planned::Execute]);

    let mut transaction = None;
    for delivery in 0..3u64 {
        let response = facilitator
            .settle(
                &mut gateway,
                &principal,
                &request,
                stable,
                step,
                &mut plane,
                &trace,
                delivery,
            )
            .unwrap_or_else(|error| panic!("settle {delivery}: {error}"));
        assert!(response.success);
        match &transaction {
            None => transaction = Some(response.transaction.clone()),
            Some(first) => assert_eq!(&response.transaction, first),
        }
    }

    assert_eq!(plane.settle_calls, 3);
    let digest = settled_digest(&gateway, &principal, identity.idempotency_key);
    assert_eq!(
        transaction,
        Some(format!("lxp:{}", hex(&digest))),
        "every redelivery resolves to one receipt digest"
    );
}

#[test]
fn transport_independent_identity_settles_once_across_http_mcp_and_a2a() {
    let facilitator = facilitator();
    let mut gateway = GatewayCore::new();
    let trace = trace();
    register_x402(&mut gateway, &trace);
    let principal = principal();
    let request = facilitator_request();
    let stable = stable_identity();
    let step = SettlementStep::Single;
    let identity = SettlementIdentity::derive(&principal, &request, stable, step)
        .unwrap_or_else(|error| panic!("identity: {error}"));

    let mut plane = FaultInjectingPlane::new(vec![Planned::Execute; 3]);
    let mut settled = None;
    for transport in TRANSPORTS {
        // The same request arrives over a different transport each time; the
        // wire encoding round-trips but the settlement identity is unchanged.
        let encoded = encode_facilitator_request(transport, &request)
            .unwrap_or_else(|error| panic!("encode {transport:?}: {error}"));
        let decoded = decode_facilitator_request(transport, &encoded)
            .unwrap_or_else(|error| panic!("decode {transport:?}: {error}"));
        let response = facilitator
            .settle(
                &mut gateway,
                &principal,
                &decoded,
                stable,
                step,
                &mut plane,
                &trace,
                0,
            )
            .unwrap_or_else(|error| panic!("settle {transport:?}: {error}"));
        assert!(response.success);
        match &settled {
            None => settled = Some(response.transaction.clone()),
            Some(first) => assert_eq!(&response.transaction, first),
        }
    }

    let digest = settled_digest(&gateway, &principal, identity.idempotency_key);
    assert_eq!(settled, Some(format!("lxp:{}", hex(&digest))));
    assert_eq!(plane.settle_calls, 3);
}

#[test]
fn settlement_recovers_after_a_crash_without_a_second_economic_effect() {
    let facilitator = facilitator();
    let mut gateway = GatewayCore::new();
    let trace = trace();
    register_x402(&mut gateway, &trace);
    let principal = principal();
    let request = facilitator_request();
    let stable = stable_identity();
    let step = SettlementStep::Single;
    let identity = SettlementIdentity::derive(&principal, &request, stable, step)
        .unwrap_or_else(|error| panic!("identity: {error}"));

    // Attempt 1: delayed confirmation, then attempt 2 crashes, then attempt 3
    // confirms, then attempt 4 is a post-success duplicate.
    let mut plane = FaultInjectingPlane::new(vec![
        Planned::Pending,
        Planned::Transient,
        Planned::Execute,
        Planned::Execute,
    ]);

    // Delayed confirmation: no economic effect yet.
    let pending = facilitator
        .settle(
            &mut gateway,
            &principal,
            &request,
            stable,
            step,
            &mut plane,
            &trace,
            0,
        )
        .unwrap_or_else(|error| panic!("pending settle: {error}"));
    assert!(!pending.success);
    assert_eq!(pending.error_reason.as_deref(), Some("settlement_pending"));
    assert_eq!(
        gateway.translation(&principal, identity.idempotency_key),
        Some(TranslationStatus::Pending)
    );

    // Crash mid-settlement: still no economic effect, translation stays open.
    let crashed = facilitator.settle(
        &mut gateway,
        &principal,
        &request,
        stable,
        step,
        &mut plane,
        &trace,
        1,
    );
    assert!(crashed.is_err());
    assert_eq!(
        gateway.translation(&principal, identity.idempotency_key),
        Some(TranslationStatus::Pending)
    );

    // Recovery: the retry confirms exactly one receipt-verified effect.
    let confirmed = facilitator
        .settle(
            &mut gateway,
            &principal,
            &request,
            stable,
            step,
            &mut plane,
            &trace,
            2,
        )
        .unwrap_or_else(|error| panic!("recovery settle: {error}"));
    assert!(confirmed.success);
    let digest = settled_digest(&gateway, &principal, identity.idempotency_key);

    // Post-success duplicate: idempotent, no new effect, same digest.
    let duplicate = facilitator
        .settle(
            &mut gateway,
            &principal,
            &request,
            stable,
            step,
            &mut plane,
            &trace,
            3,
        )
        .unwrap_or_else(|error| panic!("duplicate settle: {error}"));
    assert!(duplicate.success);
    assert_eq!(confirmed.transaction, duplicate.transaction);
    assert_eq!(
        settled_digest(&gateway, &principal, identity.idempotency_key),
        digest,
        "the recorded economic effect is unchanged by the duplicate"
    );
    assert_eq!(plane.settle_calls, 4);
}

#[test]
fn a_swapped_receipt_for_a_settled_identity_is_refused() {
    let facilitator = facilitator();
    let mut gateway = GatewayCore::new();
    let trace = trace();
    register_x402(&mut gateway, &trace);
    let principal = principal();
    let request = facilitator_request();
    let stable = stable_identity();
    let step = SettlementStep::Single;
    let identity = SettlementIdentity::derive(&principal, &request, stable, step)
        .unwrap_or_else(|error| panic!("identity: {error}"));

    // First confirm the primary receipt, then a fault redelivers a *different*
    // receipt (same asset, recipient and amount, different activity) under the
    // already-settled identity.
    let mut plane = FaultInjectingPlane::new(vec![Planned::Execute, Planned::ExecuteAlternate]);

    let confirmed = facilitator
        .settle(
            &mut gateway,
            &principal,
            &request,
            stable,
            step,
            &mut plane,
            &trace,
            0,
        )
        .unwrap_or_else(|error| panic!("primary settle: {error}"));
    assert!(confirmed.success);
    let digest = settled_digest(&gateway, &principal, identity.idempotency_key);

    let swapped = facilitator.settle(
        &mut gateway,
        &principal,
        &request,
        stable,
        step,
        &mut plane,
        &trace,
        1,
    );
    assert!(
        swapped.is_err(),
        "a conflicting receipt under a settled identity must be refused"
    );
    assert_eq!(
        settled_digest(&gateway, &principal, identity.idempotency_key),
        digest,
        "the original economic effect is preserved unchanged"
    );
}

fn hex(bytes: &[u8; 32]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}
