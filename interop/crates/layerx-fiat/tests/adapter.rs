use ed25519_dalek::{Signer as _, SigningKey};
use layerx_fiat::{
    EvidenceClass, ExecutedFiatOutcome, ExternalId, FiatAdapter, FiatError, FiatIntent,
    FiatJourneyState, FiatPlane, FiatPlaneResult, FiatRail, PlaneFiatOutcome, ProviderEvidence,
    ProviderVerifier, TokenReference, VerifiedProviderFacts,
};
use layerx_interop_gateway::adapter::{
    AdapterDescriptor, AdapterId, ConformanceSuite, PinnedSpec, SpecVersion,
};
use layerx_interop_gateway::principal::PrincipalId;
use layerx_interop_gateway::trace::TraceId;
use layerx_interop_gateway::{interop_gateway_core, GatewayCore};
use layerx_proof::merkle::leaf_hash;
use layerx_proof::receipt::AuthorizedBatch;
use layerx_types::payload::ModuleId;
use sha2::{Digest as _, Sha256};

const ASSET: [u8; 32] = [0x21; 32];
const DESTINATION: [u8; 32] = [0x22; 32];
const PROVIDER_KEY: [u8; 32] = [0x99; 32];
const PERIOD_START: u64 = 1_700_000_000;
const WINDOW_START: u64 = 200;
const NOW: u64 = 1_735_000_000;

struct ReceiptFields {
    activity_id: [u8; 32],
    sequence: u64,
    previous_state_root: [u8; 32],
    resulting_state_root: [u8; 32],
    batch_id: [u8; 32],
    asset: [u8; 32],
    amount: u128,
    from: [u8; 32],
    to: [u8; 32],
}

struct ReceiptMaterial {
    canonical_receipt: Vec<u8>,
    authorised_batch: AuthorizedBatch,
}

fn signed_receipt(
    sequence: u64,
    idempotency_key: [u8; 32],
    amount: u128,
    from: [u8; 32],
    to: [u8; 32],
) -> ReceiptMaterial {
    let activity_id: [u8; 32] = Sha256::digest(
        [
            b"fiat-adapter-activity/v1".as_slice(),
            &sequence.to_be_bytes(),
            &idempotency_key,
        ]
        .concat(),
    )
    .into();
    let fields = ReceiptFields {
        activity_id,
        sequence,
        previous_state_root: Sha256::digest([b"before".as_slice(), &activity_id].concat()).into(),
        resulting_state_root: Sha256::digest([b"after".as_slice(), &activity_id].concat()).into(),
        batch_id: Sha256::digest([b"batch".as_slice(), &activity_id].concat()).into(),
        asset: ASSET,
        amount,
        from,
        to,
    };
    let signer = SigningKey::from_bytes(&PROVIDER_KEY);
    let unsigned = encode_receipt(&fields, None);
    let mut digest = Sha256::new();
    digest.update(b"LXP/v1/receipt\0");
    digest.update(&unsigned);
    let signature = signer.sign(&<[u8; 32]>::from(digest.finalize()));
    ReceiptMaterial {
        canonical_receipt: encode_receipt(&fields, Some(signature.to_bytes())),
        authorised_batch: AuthorizedBatch::new(
            fields.batch_id,
            fields.asset,
            fields.previous_state_root,
            fields.resulting_state_root,
            signer.verifying_key().to_bytes(),
        ),
    }
}

fn encode_receipt(fields: &ReceiptFields, signature: Option<[u8; 64]>) -> Vec<u8> {
    let from_before = 50_000_u128;
    let to_before = 10_000_u128;
    let mut bytes = Vec::new();
    push_u16(&mut bytes, 2);
    push_u16(&mut bytes, 0x5201);
    push_u16(&mut bytes, 2);
    push_bytes(&mut bytes, &fields.activity_id);
    push_u64(&mut bytes, fields.sequence);
    push_bytes(&mut bytes, &fields.previous_state_root);
    push_bytes(&mut bytes, &fields.resulting_state_root);
    push_bytes(&mut bytes, &[0x81; 32]);
    bytes.extend_from_slice(&0_i32.to_be_bytes());
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    bytes.extend_from_slice(&1_u128.to_be_bytes());
    push_bytes(&mut bytes, &fields.batch_id);
    push_u16(&mut bytes, ModuleId::Asset as u16);
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.push(1);
    push_bytes(&mut bytes, &fields.asset);
    bytes.extend_from_slice(&fields.amount.to_be_bytes());
    push_bytes(&mut bytes, &fields.from);
    bytes.extend_from_slice(&from_before.to_be_bytes());
    bytes.extend_from_slice(&(from_before - fields.amount).to_be_bytes());
    push_u64(&mut bytes, fields.sequence - WINDOW_START + 1);
    push_bytes(&mut bytes, &fields.to);
    bytes.extend_from_slice(&to_before.to_be_bytes());
    bytes.extend_from_slice(&(to_before + fields.amount).to_be_bytes());
    push_bytes(&mut bytes, &[0x91; 32]);
    push_bytes(&mut bytes, &[0x92; 32]);
    push_bytes(&mut bytes, &[0x93; 32]);
    push_u64(&mut bytes, PERIOD_START + fields.sequence);
    bytes.push(u8::from(signature.is_some()));
    if let Some(signature) = signature {
        push_bytes(&mut bytes, &signature);
    }
    bytes
}

fn push_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn push_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn push_bytes(output: &mut Vec<u8>, value: &[u8]) {
    let length = u32::try_from(value.len()).unwrap_or_else(|_| panic!("field overflow"));
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(value);
}

fn principal(name: &str) -> PrincipalId {
    PrincipalId::new(name).unwrap_or_else(|error| panic!("principal {name}: {error}"))
}

fn adapter_id() -> AdapterId {
    AdapterId::new("fiat").unwrap_or_else(|error| panic!("adapter: {error}"))
}

fn registered_gateway() -> GatewayCore {
    let mut gateway = interop_gateway_core();
    let version = SpecVersion::parse("1.0.0").unwrap_or_else(|error| panic!("version: {error}"));
    let spec = PinnedSpec::new(adapter_id(), version, [0xa1; 32])
        .unwrap_or_else(|error| panic!("spec: {error}"));
    let conformance = ConformanceSuite::new(adapter_id(), 128, [0xa2; 32])
        .unwrap_or_else(|error| panic!("conformance: {error}"));
    let descriptor = AdapterDescriptor::new(adapter_id(), spec, conformance);
    gateway
        .register_adapter(descriptor, &TraceId::mint([1; 16]), NOW)
        .unwrap_or_else(|error| panic!("register: {error}"));
    gateway
}

#[derive(Clone)]
struct SandboxVerifier {
    provider: ExternalId,
    settlement: ExternalId,
    rail: FiatRail,
    class: EvidenceClass,
    amount: u128,
    hold_until: Option<u64>,
    fault: Option<FiatError>,
}

impl ProviderVerifier for SandboxVerifier {
    fn verify(
        &self,
        _token: &TokenReference,
        _evidence: &ProviderEvidence,
        _trace: &TraceId,
    ) -> Result<VerifiedProviderFacts, FiatError> {
        if let Some(fault) = self.fault {
            return Err(fault);
        }
        Ok(VerifiedProviderFacts {
            provider: self.provider.clone(),
            settlement: self.settlement.clone(),
            rail: self.rail,
            class: self.class,
            amount: self.amount,
            asset: ASSET,
            destination: DESTINATION,
            observed_at: NOW,
            hold_until: self.hold_until,
        })
    }
}

struct SandboxPlane {
    intent_outcome: Result<FiatPlaneResult, FiatError>,
}

impl FiatPlane for SandboxPlane {
    fn execute(
        &mut self,
        _intent: FiatIntent,
        _trace: &TraceId,
    ) -> Result<FiatPlaneResult, FiatError> {
        match &self.intent_outcome {
            Ok(FiatPlaneResult::Open(outcome)) => Ok(FiatPlaneResult::Open(*outcome)),
            Ok(FiatPlaneResult::Executed(executed)) => {
                Ok(FiatPlaneResult::Executed(ExecutedFiatOutcome {
                    canonical_receipt: executed.canonical_receipt.clone(),
                    authorised_batch: executed.authorised_batch,
                }))
            }
            Err(error) => Err(*error),
        }
    }
}

fn sandbox_verifier(class: EvidenceClass, hold_until: Option<u64>) -> SandboxVerifier {
    SandboxVerifier {
        provider: ExternalId::new("provider-1").unwrap(),
        settlement: ExternalId::new("settle-001").unwrap(),
        rail: FiatRail::Card,
        class,
        amount: 5_000,
        hold_until,
        fault: None,
    }
}

fn faulty_verifier(fault: FiatError) -> SandboxVerifier {
    let mut verifier = sandbox_verifier(EvidenceClass::Settled, None);
    verifier.fault = Some(fault);
    verifier
}

fn token() -> TokenReference {
    TokenReference::new(b"tok_sandbox_card_12345".to_vec())
        .unwrap_or_else(|error| panic!("token: {error}"))
}

fn evidence() -> ProviderEvidence {
    ProviderEvidence::new(b"provider-signed-evidence-blob".to_vec())
        .unwrap_or_else(|error| panic!("evidence: {error}"))
}

#[test]
fn card_data_never_enters_layerx_components() {
    assert!(
        matches!(
            TokenReference::new(Vec::new()),
            Err(FiatError::CardDataRefused)
        ),
        "empty token must be refused"
    );
    assert!(
        matches!(
            TokenReference::new(vec![0; 513]),
            Err(FiatError::CardDataRefused)
        ),
        "oversized token must be refused"
    );
    assert!(
        matches!(
            TokenReference::new(b"4532123456789012".to_vec()),
            Err(FiatError::CardDataRefused)
        ),
        "raw 16-digit PAN must be refused"
    );
    assert!(
        matches!(
            TokenReference::new(b"4532 1234 5678 9012".to_vec()),
            Err(FiatError::CardDataRefused)
        ),
        "space-separated PAN must be refused"
    );
    assert!(
        matches!(
            TokenReference::new(b"4532-1234-5678-9012".to_vec()),
            Err(FiatError::CardDataRefused)
        ),
        "dash-separated PAN must be refused"
    );
    assert!(
        matches!(
            TokenReference::new(b"378282246310005".to_vec()),
            Err(FiatError::CardDataRefused)
        ),
        "15-digit AMEX PAN must be refused"
    );
    let token = TokenReference::new(b"tok_visa_abcdef123456".to_vec())
        .unwrap_or_else(|error| panic!("certified token must be accepted: {error}"));
    let debug = format!("{token:?}");
    assert!(
        debug.contains("[REDACTED]"),
        "token debug must redact contents"
    );
    assert!(
        !debug.contains("tok_visa"),
        "token debug must not leak the actual token bytes"
    );
}

#[test]
fn authorised_hold_requires_a_declared_hold_until() {
    let mut gateway = registered_gateway();
    let alice = principal("alice");
    let trace = TraceId::mint([2; 16]);
    let verifier = sandbox_verifier(EvidenceClass::Authorised, Some(NOW + 300));
    let mut plane = SandboxPlane {
        intent_outcome: Ok(FiatPlaneResult::Open(PlaneFiatOutcome::Pending)),
    };
    let state = FiatAdapter::apply(
        &mut gateway,
        &alice,
        &token(),
        &evidence(),
        &verifier,
        &mut plane,
        &trace,
        NOW,
    )
    .unwrap_or_else(|error| panic!("authorised hold: {error}"));
    assert_eq!(state, FiatJourneyState::AuthorisedHold { until: NOW + 300 });

    let no_hold = sandbox_verifier(EvidenceClass::Authorised, None);
    let refused = FiatAdapter::apply(
        &mut gateway,
        &principal("bob"),
        &token(),
        &evidence(),
        &no_hold,
        &mut plane,
        &trace,
        NOW,
    )
    .map_err(|traced| *traced.error());
    assert_eq!(refused, Err(FiatError::HoldRequired));
}

#[test]
fn clearing_hold_requires_a_declared_hold_until() {
    let mut gateway = registered_gateway();
    let alice = principal("alice");
    let trace = TraceId::mint([3; 16]);
    let verifier = sandbox_verifier(EvidenceClass::Clearing, Some(NOW + 600));
    let mut plane = SandboxPlane {
        intent_outcome: Ok(FiatPlaneResult::Open(PlaneFiatOutcome::Pending)),
    };
    let state = FiatAdapter::apply(
        &mut gateway,
        &alice,
        &token(),
        &evidence(),
        &verifier,
        &mut plane,
        &trace,
        NOW,
    )
    .unwrap_or_else(|error| panic!("clearing hold: {error}"));
    assert_eq!(state, FiatJourneyState::ClearingHold { until: NOW + 600 });

    let no_hold = sandbox_verifier(EvidenceClass::Clearing, None);
    let refused = FiatAdapter::apply(
        &mut gateway,
        &principal("charlie"),
        &token(),
        &evidence(),
        &no_hold,
        &mut plane,
        &trace,
        NOW,
    )
    .map_err(|traced| *traced.error());
    assert_eq!(refused, Err(FiatError::HoldRequired));
}

#[test]
fn settled_credits_only_against_verified_receipt_evidence() {
    let mut gateway = registered_gateway();
    let alice = principal("alice");
    let trace = TraceId::mint([4; 16]);
    let verifier = sandbox_verifier(EvidenceClass::Settled, None);
    let idempotency_key = [0x31; 32];
    let material = signed_receipt(250, idempotency_key, 5_000, [0x88; 32], DESTINATION);
    let mut plane = SandboxPlane {
        intent_outcome: Ok(FiatPlaneResult::Executed(ExecutedFiatOutcome {
            canonical_receipt: material.canonical_receipt.clone(),
            authorised_batch: material.authorised_batch.clone(),
        })),
    };
    let state = FiatAdapter::apply(
        &mut gateway,
        &alice,
        &token(),
        &evidence(),
        &verifier,
        &mut plane,
        &trace,
        NOW,
    )
    .unwrap_or_else(|error| panic!("settled credit: {error}"));
    let expected_digest = leaf_hash(&material.canonical_receipt)
        .unwrap_or_else(|error| panic!("receipt digest: {error:?}"));
    assert_eq!(
        state,
        FiatJourneyState::Credited {
            receipt_digest: expected_digest
        }
    );

    let mut pending_plane = SandboxPlane {
        intent_outcome: Ok(FiatPlaneResult::Open(PlaneFiatOutcome::Pending)),
    };
    let pending = FiatAdapter::apply(
        &mut gateway,
        &principal("bob"),
        &token(),
        &evidence(),
        &verifier,
        &mut pending_plane,
        &trace,
        NOW,
    )
    .unwrap_or_else(|error| panic!("pending credit: {error}"));
    assert_eq!(pending, FiatJourneyState::CreditPending);
}

#[test]
fn receipt_mismatch_refuses_credit_and_preserves_honest_state() {
    let mut gateway = registered_gateway();
    let alice = principal("alice");
    let trace = TraceId::mint([5; 16]);
    let verifier = sandbox_verifier(EvidenceClass::Settled, None);
    let material = signed_receipt(260, [0x32; 32], 3_000, [0x88; 32], DESTINATION);
    let mut plane = SandboxPlane {
        intent_outcome: Ok(FiatPlaneResult::Executed(ExecutedFiatOutcome {
            canonical_receipt: material.canonical_receipt.clone(),
            authorised_batch: material.authorised_batch.clone(),
        })),
    };
    let refused = FiatAdapter::apply(
        &mut gateway,
        &alice,
        &token(),
        &evidence(),
        &verifier,
        &mut plane,
        &trace,
        NOW,
    )
    .map_err(|traced| *traced.error());
    assert_eq!(
        refused,
        Err(FiatError::ReceiptMismatch),
        "amount mismatch between provider evidence (5_000) and receipt (3_000) must refuse"
    );
}

#[test]
fn reversal_reconciles_through_legitimate_protocol_operation() {
    let mut gateway = registered_gateway();
    let alice = principal("alice");
    let trace = TraceId::mint([6; 16]);
    let verifier = sandbox_verifier(EvidenceClass::Reversed, Some(NOW + 1_800));
    let idempotency_key = [0x33; 32];
    let material = signed_receipt(270, idempotency_key, 5_000, DESTINATION, [0x88; 32]);
    let mut plane = SandboxPlane {
        intent_outcome: Ok(FiatPlaneResult::Executed(ExecutedFiatOutcome {
            canonical_receipt: material.canonical_receipt.clone(),
            authorised_batch: material.authorised_batch.clone(),
        })),
    };
    let state = FiatAdapter::apply(
        &mut gateway,
        &alice,
        &token(),
        &evidence(),
        &verifier,
        &mut plane,
        &trace,
        NOW,
    )
    .unwrap_or_else(|error| panic!("reversal: {error}"));
    let expected_digest = leaf_hash(&material.canonical_receipt)
        .unwrap_or_else(|error| panic!("receipt digest: {error:?}"));
    assert_eq!(
        state,
        FiatJourneyState::Reversed {
            receipt_digest: expected_digest
        }
    );

    let mut pending_plane = SandboxPlane {
        intent_outcome: Ok(FiatPlaneResult::Open(PlaneFiatOutcome::Pending)),
    };
    let pending = FiatAdapter::apply(
        &mut gateway,
        &principal("bob"),
        &token(),
        &evidence(),
        &verifier,
        &mut pending_plane,
        &trace,
        NOW,
    )
    .unwrap_or_else(|error| panic!("pending reversal: {error}"));
    assert_eq!(
        pending,
        FiatJourneyState::ReversalPending {
            hold_until: Some(NOW + 1_800)
        }
    );
}

#[test]
fn chargeback_reconciles_through_legitimate_protocol_operation() {
    let mut gateway = registered_gateway();
    let alice = principal("alice");
    let trace = TraceId::mint([7; 16]);
    let verifier = sandbox_verifier(EvidenceClass::Chargeback, Some(NOW + 7_200));
    let idempotency_key = [0x34; 32];
    let material = signed_receipt(280, idempotency_key, 5_000, DESTINATION, [0x88; 32]);
    let mut plane = SandboxPlane {
        intent_outcome: Ok(FiatPlaneResult::Executed(ExecutedFiatOutcome {
            canonical_receipt: material.canonical_receipt.clone(),
            authorised_batch: material.authorised_batch.clone(),
        })),
    };
    let state = FiatAdapter::apply(
        &mut gateway,
        &alice,
        &token(),
        &evidence(),
        &verifier,
        &mut plane,
        &trace,
        NOW,
    )
    .unwrap_or_else(|error| panic!("chargeback: {error}"));
    let expected_digest = leaf_hash(&material.canonical_receipt)
        .unwrap_or_else(|error| panic!("receipt digest: {error:?}"));
    assert_eq!(
        state,
        FiatJourneyState::ChargedBack {
            receipt_digest: expected_digest
        }
    );

    let mut pending_plane = SandboxPlane {
        intent_outcome: Ok(FiatPlaneResult::Open(PlaneFiatOutcome::Pending)),
    };
    let pending = FiatAdapter::apply(
        &mut gateway,
        &principal("carol"),
        &token(),
        &evidence(),
        &verifier,
        &mut pending_plane,
        &trace,
        NOW,
    )
    .unwrap_or_else(|error| panic!("pending chargeback: {error}"));
    assert_eq!(
        pending,
        FiatJourneyState::ChargebackPending {
            hold_until: Some(NOW + 7_200)
        }
    );
}

#[test]
fn provider_fault_injection_refuses_settlement_and_surfaces_honest_state() {
    let mut gateway = registered_gateway();
    let alice = principal("alice");
    let trace = TraceId::mint([8; 16]);
    let mut plane = SandboxPlane {
        intent_outcome: Ok(FiatPlaneResult::Open(PlaneFiatOutcome::Pending)),
    };

    let invalid_evidence_verifier = faulty_verifier(FiatError::InvalidEvidence);
    let refused_evidence = FiatAdapter::apply(
        &mut gateway,
        &alice,
        &token(),
        &evidence(),
        &invalid_evidence_verifier,
        &mut plane,
        &trace,
        NOW,
    )
    .map_err(|traced| *traced.error());
    assert_eq!(refused_evidence, Err(FiatError::InvalidEvidence));

    let card_data_verifier = faulty_verifier(FiatError::CardDataRefused);
    let refused_card = FiatAdapter::apply(
        &mut gateway,
        &principal("bob"),
        &token(),
        &evidence(),
        &card_data_verifier,
        &mut plane,
        &trace,
        NOW,
    )
    .map_err(|traced| *traced.error());
    assert_eq!(refused_card, Err(FiatError::CardDataRefused));
}

#[test]
fn plane_refusal_surfaces_through_honest_journey_state() {
    let mut gateway = registered_gateway();
    let alice = principal("alice");
    let trace = TraceId::mint([9; 16]);
    let verifier = sandbox_verifier(EvidenceClass::Settled, None);
    let mut plane = SandboxPlane {
        intent_outcome: Ok(FiatPlaneResult::Open(PlaneFiatOutcome::Refused)),
    };
    let state = FiatAdapter::apply(
        &mut gateway,
        &alice,
        &token(),
        &evidence(),
        &verifier,
        &mut plane,
        &trace,
        NOW,
    )
    .unwrap_or_else(|error| panic!("plane refusal: {error}"));
    assert_eq!(state, FiatJourneyState::Refused);
}

#[test]
fn idempotency_binds_provider_settlement_to_exactly_one_outcome() {
    let mut gateway = registered_gateway();
    let alice = principal("alice");
    let trace = TraceId::mint([10; 16]);
    let verifier = sandbox_verifier(EvidenceClass::Settled, None);
    let idempotency_key = [0x35; 32];
    let material = signed_receipt(290, idempotency_key, 5_000, [0x88; 32], DESTINATION);
    let mut plane = SandboxPlane {
        intent_outcome: Ok(FiatPlaneResult::Executed(ExecutedFiatOutcome {
            canonical_receipt: material.canonical_receipt.clone(),
            authorised_batch: material.authorised_batch.clone(),
        })),
    };
    let first = FiatAdapter::apply(
        &mut gateway,
        &alice,
        &token(),
        &evidence(),
        &verifier,
        &mut plane,
        &trace,
        NOW,
    )
    .unwrap_or_else(|error| panic!("first apply: {error}"));
    let replay = FiatAdapter::apply(
        &mut gateway,
        &alice,
        &token(),
        &evidence(),
        &verifier,
        &mut plane,
        &trace,
        NOW + 10,
    )
    .unwrap_or_else(|error| panic!("replay: {error}"));
    assert_eq!(
        first, replay,
        "replayed settlement must return the same outcome"
    );
}

#[test]
fn evidence_classes_model_rail_specific_settlement_stages() {
    assert_ne!(EvidenceClass::Authorised, EvidenceClass::Clearing);
    assert_ne!(EvidenceClass::Clearing, EvidenceClass::Settled);
    assert_ne!(EvidenceClass::Settled, EvidenceClass::Reversed);
    assert_ne!(EvidenceClass::Reversed, EvidenceClass::Chargeback);
    let verifier = SandboxVerifier {
        provider: ExternalId::new("provider-bank").unwrap(),
        settlement: ExternalId::new("settle-bank-001").unwrap(),
        rail: FiatRail::Bank,
        class: EvidenceClass::Settled,
        amount: 10_000,
        hold_until: None,
        fault: None,
    };
    let facts = verifier
        .verify(&token(), &evidence(), &TraceId::mint([11; 16]))
        .unwrap_or_else(|error| panic!("bank verifier: {error}"));
    assert_eq!(facts.rail, FiatRail::Bank);
    let rtp_verifier = SandboxVerifier {
        rail: FiatRail::RealTimePayment,
        ..verifier.clone()
    };
    let rtp_facts = rtp_verifier
        .verify(&token(), &evidence(), &TraceId::mint([12; 16]))
        .unwrap_or_else(|error| panic!("rtp verifier: {error}"));
    assert_eq!(rtp_facts.rail, FiatRail::RealTimePayment);
}

#[test]
fn adapter_interfaces_are_rail_agnostic_and_provider_edge_only() {
    let card_verifier = sandbox_verifier(EvidenceClass::Settled, None);
    let bank_verifier = SandboxVerifier {
        rail: FiatRail::Bank,
        ..card_verifier.clone()
    };
    let rtp_verifier = SandboxVerifier {
        rail: FiatRail::RealTimePayment,
        ..card_verifier.clone()
    };
    let mut gateway = registered_gateway();
    let trace = TraceId::mint([13; 16]);
    let material = signed_receipt(300, [0x36; 32], 5_000, [0x88; 32], DESTINATION);
    let mut plane = SandboxPlane {
        intent_outcome: Ok(FiatPlaneResult::Executed(ExecutedFiatOutcome {
            canonical_receipt: material.canonical_receipt.clone(),
            authorised_batch: material.authorised_batch.clone(),
        })),
    };
    let card_state = FiatAdapter::apply(
        &mut gateway,
        &principal("alice"),
        &token(),
        &evidence(),
        &card_verifier,
        &mut plane,
        &trace,
        NOW,
    )
    .unwrap_or_else(|error| panic!("card adapter: {error}"));
    let bank_state = FiatAdapter::apply(
        &mut gateway,
        &principal("bob"),
        &token(),
        &evidence(),
        &bank_verifier,
        &mut plane,
        &trace,
        NOW,
    )
    .unwrap_or_else(|error| panic!("bank adapter: {error}"));
    let rtp_state = FiatAdapter::apply(
        &mut gateway,
        &principal("carol"),
        &token(),
        &evidence(),
        &rtp_verifier,
        &mut plane,
        &trace,
        NOW,
    )
    .unwrap_or_else(|error| panic!("rtp adapter: {error}"));
    assert!(
        matches!(card_state, FiatJourneyState::Credited { .. }),
        "card rail must credit"
    );
    assert!(
        matches!(bank_state, FiatJourneyState::Credited { .. }),
        "bank rail must credit"
    );
    assert!(
        matches!(rtp_state, FiatJourneyState::Credited { .. }),
        "rtp rail must credit"
    );
}
