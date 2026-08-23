use layerx_ucp::{
    Capability, CheckoutStatus, CheckoutSubmission, MerchantProfile, NegotiatedCapabilities,
    OrderMetadata, PaymentHandler, PlatformProfile, StoredOrder, UcpAdapter, UcpError,
    UcpIdempotencyKey, UcpOrder, UcpPaymentIntent, UcpPaymentPlane, UcpPlaneResult,
};
use layerx_interop_gateway::adapter::{AdapterDescriptor, AdapterId, ConformanceSuite, PinnedSpec, SpecVersion};
use layerx_interop_gateway::principal::PrincipalId;
use layerx_interop_gateway::trace::TraceId;
use layerx_interop_gateway::GatewayCore;
use layerx_proof::merkle::leaf_hash;
use layerx_proof::receipt::{verify, AuthorizedBatch};
use ed25519_dalek::{Signer as _, SigningKey};
use sha2::{Digest as _, Sha256};
use std::collections::HashMap;

const UCP_VERSION: &str = "2026-04-08";
const CHECKOUT_CAPABILITY: &str = "dev.ucp.shopping.checkout";
const ORDER_CAPABILITY: &str = "dev.ucp.shopping.order";
const PAYMENT_HANDLER_ID: &str = "layerx-402";
const PAYMENT_HANDLER_VERSION: &str = "2.0.0";
const PAYMENT_HANDLER_SPEC: &str = "https://layerx.dev/2026-04-08/specification/402";
const PAYMENT_HANDLER_SCHEMA: &str = "https://layerx.dev/2026-04-08/schemas/402.json";

struct TestPaymentPlane {
    pending_checkouts: HashMap<[u8; 32], ()>,
    executed_checkouts: HashMap<[u8; 32], (OrderMetadata, Vec<u8>, AuthorizedBatch)>,
}

impl TestPaymentPlane {
    fn new() -> Self {
        Self {
            pending_checkouts: HashMap::new(),
            executed_checkouts: HashMap::new(),
        }
    }

    fn mark_pending(&mut self, idempotency_key: [u8; 32]) {
        self.pending_checkouts.insert(idempotency_key, ());
    }

    fn execute_checkout(
        &mut self,
        intent: &UcpPaymentIntent,
        sequencer_seed: [u8; 32],
    ) {
        let metadata = OrderMetadata {
            order_id: format!("ord_{}", hex(&intent.idempotency_key[..8])),
            permalink_url: format!("https://merchant.example/{}", hex(&intent.idempotency_key[..8])),
        };
        
        let receipt_material = signed_receipt(
            100,
            intent.idempotency_key,
            intent.amount,
            intent.asset,
            intent.recipient,
            sequencer_seed,
        );

        self.pending_checkouts.remove(&intent.idempotency_key);
        self.executed_checkouts.insert(
            intent.idempotency_key,
            (metadata, receipt_material.0, receipt_material.1),
        );
    }
}

impl UcpPaymentPlane for TestPaymentPlane {
    fn execute(
        &mut self,
        intent: &UcpPaymentIntent,
        _trace: &TraceId,
    ) -> Result<UcpPlaneResult, UcpError> {
        if let Some((metadata, receipt, batch)) = self.executed_checkouts.get(&intent.idempotency_key) {
            return Ok(UcpPlaneResult::Executed(Box::new(
                layerx_ucp::ExecutedUcpPayment {
                    metadata: metadata.clone(),
                    canonical_receipt: receipt.clone(),
                    authorised_batch: batch.clone(),
                },
            )));
        }

        if self.pending_checkouts.contains_key(&intent.idempotency_key) {
            return Ok(UcpPlaneResult::Pending);
        }

        Ok(UcpPlaneResult::Pending)
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn signed_receipt(
    sequence: u64,
    idempotency_key: [u8; 32],
    amount: u128,
    asset: [u8; 32],
    recipient: [u8; 32],
    sequencer_seed: [u8; 32],
) -> (Vec<u8>, AuthorizedBatch) {
    let activity_id: [u8; 32] = Sha256::digest(
        [
            b"layerx-ucp-activity/v1".as_slice(),
            &sequence.to_be_bytes(),
            &idempotency_key,
        ]
        .concat(),
    )
    .into();
    
    let previous_state_root: [u8; 32] = Sha256::digest([b"before".as_slice(), &activity_id].concat()).into();
    let resulting_state_root: [u8; 32] = Sha256::digest([b"after".as_slice(), &activity_id].concat()).into();
    let batch_id: [u8; 32] = Sha256::digest([b"batch".as_slice(), &activity_id].concat()).into();
    
    let signer = SigningKey::from_bytes(&sequencer_seed);
    let unsigned = encode_receipt(&activity_id, sequence, &previous_state_root, &resulting_state_root, &batch_id, &asset, amount, &recipient, None);
    
    let mut digest = Sha256::new();
    digest.update(b"LXP/v1/receipt\0");
    digest.update(&unsigned);
    let signature = signer.sign(&<[u8; 32]>::from(digest.finalize()));
    
    let canonical_receipt = encode_receipt(&activity_id, sequence, &previous_state_root, &resulting_state_root, &batch_id, &asset, amount, &recipient, Some(signature.to_bytes()));
    
    let authorised_batch = AuthorizedBatch::new(
        batch_id,
        asset,
        previous_state_root,
        resulting_state_root,
        signer.verifying_key().to_bytes(),
    );
    
    (canonical_receipt, authorised_batch)
}

fn encode_receipt(
    activity_id: &[u8; 32],
    sequence: u64,
    previous_state_root: &[u8; 32],
    resulting_state_root: &[u8; 32],
    batch_id: &[u8; 32],
    asset: &[u8; 32],
    amount: u128,
    recipient: &[u8; 32],
    signature: Option<[u8; 64]>,
) -> Vec<u8> {
    let sender = [0xa1; 32];
    let debit_before = 50_000_u128;
    let credit_before = 10_000_u128;
    
    let mut bytes = Vec::new();
    push_u16(&mut bytes, 1);
    push_u16(&mut bytes, 0x5201);
    push_u16(&mut bytes, 1);
    push_bytes(&mut bytes, activity_id);
    push_u64(&mut bytes, sequence);
    push_bytes(&mut bytes, previous_state_root);
    push_bytes(&mut bytes, resulting_state_root);
    push_bytes(&mut bytes, &[0x81; 32]);
    bytes.extend_from_slice(&0_i32.to_be_bytes());
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    bytes.extend_from_slice(&1_u128.to_be_bytes());
    push_bytes(&mut bytes, batch_id);
    push_u16(&mut bytes, 1);
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.push(1);
    push_bytes(&mut bytes, asset);
    bytes.extend_from_slice(&amount.to_be_bytes());
    push_bytes(&mut bytes, &sender);
    bytes.extend_from_slice(&debit_before.to_be_bytes());
    bytes.extend_from_slice(&(debit_before - amount).to_be_bytes());
    push_u64(&mut bytes, sequence);
    push_bytes(&mut bytes, recipient);
    bytes.extend_from_slice(&credit_before.to_be_bytes());
    bytes.extend_from_slice(&(credit_before + amount).to_be_bytes());
    push_bytes(&mut bytes, &[0x91; 32]);
    push_bytes(&mut bytes, &[0x92; 32]);
    push_bytes(&mut bytes, &[0x93; 32]);
    push_u64(&mut bytes, 1_700_000_000 + sequence);
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
    let length = u32::try_from(value.len()).unwrap_or_else(|_| panic!("receipt field overflow"));
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(value);
}

#[test]
fn merchant_profile_publishes_checkout_and_order_capabilities() {
    let handler = PaymentHandler::new(
        PAYMENT_HANDLER_ID,
        PAYMENT_HANDLER_VERSION,
        PAYMENT_HANDLER_SPEC,
        PAYMENT_HANDLER_SCHEMA,
    )
    .unwrap_or_else(|error| panic!("payment handler: {error}"));

    let profile = MerchantProfile::layerx("https://merchant.example/ucp", handler)
        .unwrap_or_else(|error| panic!("merchant profile: {error}"));

    assert_eq!(profile.version(), UCP_VERSION);
    assert_eq!(profile.rest().endpoint(), "https://merchant.example/ucp");
    assert_eq!(profile.capabilities().len(), 2);

    let checkout = profile
        .capabilities()
        .iter()
        .find(|c| c.name() == CHECKOUT_CAPABILITY)
        .unwrap_or_else(|| panic!("checkout capability missing"));
    assert_eq!(checkout.version(), UCP_VERSION);
    assert!(checkout.spec().starts_with("https://ucp.dev/"));
    assert!(checkout.schema().starts_with("https://ucp.dev/"));

    let order = profile
        .capabilities()
        .iter()
        .find(|c| c.name() == ORDER_CAPABILITY)
        .unwrap_or_else(|| panic!("order capability missing"));
    assert_eq!(order.version(), UCP_VERSION);
}

#[test]
fn capability_negotiation_requires_exact_checkout_match() {
    let merchant_handler = PaymentHandler::new(
        PAYMENT_HANDLER_ID,
        PAYMENT_HANDLER_VERSION,
        PAYMENT_HANDLER_SPEC,
        PAYMENT_HANDLER_SCHEMA,
    )
    .unwrap_or_else(|error| panic!("merchant handler: {error}"));

    let platform = PlatformProfile {
        profile_url: "https://platform.example/profile".to_owned(),
        capabilities: vec![
            Capability::new(CHECKOUT_CAPABILITY, UCP_VERSION, "https://ucp.dev/checkout", "https://ucp.dev/checkout.json")
                .unwrap_or_else(|error| panic!("checkout capability: {error}")),
        ],
        payment_handlers: vec![
            PaymentHandler::new(PAYMENT_HANDLER_ID, PAYMENT_HANDLER_VERSION, PAYMENT_HANDLER_SPEC, PAYMENT_HANDLER_SCHEMA)
                .unwrap_or_else(|error| panic!("platform handler: {error}")),
        ],
    };

    let negotiated = NegotiatedCapabilities::negotiate(&platform, &merchant_handler)
        .unwrap_or_else(|error| panic!("negotiation: {error}"));
    assert!(negotiated.checkout());
    assert!(!negotiated.order());

    let unsupported_version = PlatformProfile {
        profile_url: "https://platform.example/profile".to_owned(),
        capabilities: vec![
            Capability::new(CHECKOUT_CAPABILITY, "2025-01-01", "https://ucp.dev/old", "https://ucp.dev/old.json")
                .unwrap_or_else(|error| panic!("old capability: {error}")),
        ],
        payment_handlers: vec![merchant_handler.clone()],
    };

    let refused = NegotiatedCapabilities::negotiate(&unsupported_version, &merchant_handler);
    assert!(matches!(refused, Err(UcpError::CapabilityUnavailable)));
}

#[test]
fn checkout_completion_requires_receipt_verification() {
    let mut gateway = layerx_interop_gateway::interop_gateway_core();
    let principal = PrincipalId::new("merchant").unwrap_or_else(|error| panic!("principal: {error}"));
    let trace = TraceId::mint([0xcc; 16]);

    let asset = [0xc1; 32];
    let recipient = [0xd1; 32];
    let idempotency_key = UcpIdempotencyKey::parse("12345678-1234-5678-1234-567812345678")
        .unwrap_or_else(|error| panic!("idempotency key: {error}"));

    let handler = PaymentHandler::new(
        PAYMENT_HANDLER_ID,
        PAYMENT_HANDLER_VERSION,
        PAYMENT_HANDLER_SPEC,
        PAYMENT_HANDLER_SCHEMA,
    )
    .unwrap_or_else(|error| panic!("handler: {error}"));

    let platform = PlatformProfile {
        profile_url: "https://platform.example/profile".to_owned(),
        capabilities: vec![
            Capability::new(CHECKOUT_CAPABILITY, UCP_VERSION, "https://ucp.dev/checkout", "https://ucp.dev/checkout.json")
                .unwrap_or_else(|error| panic!("checkout: {error}")),
        ],
        payment_handlers: vec![handler.clone()],
    };

    let negotiated = NegotiatedCapabilities::negotiate(&platform, &handler)
        .unwrap_or_else(|error| panic!("negotiation: {error}"));

    let submission = CheckoutSubmission {
        checkout_id: "chk_test123".to_owned(),
        currency: *b"USD",
        total_minor: 9999,
        layerx_asset: asset,
        layerx_recipient: recipient,
        idempotency_key,
        negotiated,
    };

    let mut plane = TestPaymentPlane::new();
    let sequencer_seed = [0x55; 32];
    plane.execute_checkout(
        &UcpPaymentIntent {
            checkout_id: submission.checkout_id.clone(),
            currency: submission.currency,
            amount: submission.total_minor,
            asset: submission.layerx_asset,
            recipient: submission.layerx_recipient,
            idempotency_key: idempotency_key.gateway_key(),
        },
        sequencer_seed,
    );

    let outcome = UcpAdapter::complete_checkout(&mut gateway, &principal, &submission, &mut plane, &trace, 10)
        .unwrap_or_else(|error| panic!("checkout completion: {error}"));

    assert_eq!(outcome.status, CheckoutStatus::Completed);
    assert!(outcome.order.is_some());

    let order = outcome.order.unwrap();
    assert_eq!(order.checkout_id, "chk_test123");
    assert_eq!(order.currency, *b"USD");
    assert_eq!(order.total_minor, 9999);
    assert_ne!(order.receipt_digest, [0; 32]);
}

#[test]
fn order_state_remains_receipt_backed_or_honestly_pending() {
    let mut gateway = layerx_interop_gateway::interop_gateway_core();
    let principal = PrincipalId::new("merchant").unwrap_or_else(|error| panic!("principal: {error}"));
    let trace = TraceId::mint([0xdd; 16]);

    let asset = [0xc2; 32];
    let recipient = [0xd2; 32];
    let idempotency_key = UcpIdempotencyKey::parse("87654321-4321-8765-4321-876543218765")
        .unwrap_or_else(|error| panic!("idempotency key: {error}"));

    let handler = PaymentHandler::new(
        PAYMENT_HANDLER_ID,
        PAYMENT_HANDLER_VERSION,
        PAYMENT_HANDLER_SPEC,
        PAYMENT_HANDLER_SCHEMA,
    )
    .unwrap_or_else(|error| panic!("handler: {error}"));

    let platform = PlatformProfile {
        profile_url: "https://platform.example/profile".to_owned(),
        capabilities: vec![
            Capability::new(CHECKOUT_CAPABILITY, UCP_VERSION, "https://ucp.dev/checkout", "https://ucp.dev/checkout.json")
                .unwrap_or_else(|error| panic!("checkout: {error}")),
        ],
        payment_handlers: vec![handler.clone()],
    };

    let negotiated = NegotiatedCapabilities::negotiate(&platform, &handler)
        .unwrap_or_else(|error| panic!("negotiation: {error}"));

    let submission = CheckoutSubmission {
        checkout_id: "chk_pending".to_owned(),
        currency: *b"EUR",
        total_minor: 5000,
        layerx_asset: asset,
        layerx_recipient: recipient,
        idempotency_key,
        negotiated,
    };

    let mut plane = TestPaymentPlane::new();
    plane.mark_pending(idempotency_key.gateway_key());

    let outcome = UcpAdapter::complete_checkout(&mut gateway, &principal, &submission, &mut plane, &trace, 20)
        .unwrap_or_else(|error| panic!("checkout pending: {error}"));

    assert_eq!(outcome.status, CheckoutStatus::CompleteInProgress);
    assert!(outcome.order.is_none(), "pending checkout must not return an order");
}

#[test]
fn order_read_verifies_stored_receipt_on_every_access() {
    let asset = [0xc3; 32];
    let recipient = [0xd3; 32];
    let idempotency_key = UcpIdempotencyKey::parse("abcdef01-2345-6789-abcd-ef0123456789")
        .unwrap_or_else(|error| panic!("idempotency key: {error}"));

    let handler = PaymentHandler::new(
        PAYMENT_HANDLER_ID,
        PAYMENT_HANDLER_VERSION,
        PAYMENT_HANDLER_SPEC,
        PAYMENT_HANDLER_SCHEMA,
    )
    .unwrap_or_else(|error| panic!("handler: {error}"));

    let platform = PlatformProfile {
        profile_url: "https://platform.example/profile".to_owned(),
        capabilities: vec![
            Capability::new(CHECKOUT_CAPABILITY, UCP_VERSION, "https://ucp.dev/checkout", "https://ucp.dev/checkout.json")
                .unwrap_or_else(|error| panic!("checkout: {error}")),
            Capability::new(ORDER_CAPABILITY, UCP_VERSION, "https://ucp.dev/order", "https://ucp.dev/order.json")
                .unwrap_or_else(|error| panic!("order: {error}")),
        ],
        payment_handlers: vec![handler.clone()],
    };

    let negotiated = NegotiatedCapabilities::negotiate(&platform, &handler)
        .unwrap_or_else(|error| panic!("negotiation: {error}"));

    let submission = CheckoutSubmission {
        checkout_id: "chk_order_read".to_owned(),
        currency: *b"GBP",
        total_minor: 12500,
        layerx_asset: asset,
        layerx_recipient: recipient,
        idempotency_key,
        negotiated,
    };

    let sequencer_seed = [0x77; 32];
    let (receipt, batch) = signed_receipt(
        105,
        idempotency_key.gateway_key(),
        submission.total_minor,
        asset,
        recipient,
        sequencer_seed,
    );

    let metadata = OrderMetadata {
        order_id: "ord_read_test".to_owned(),
        permalink_url: "https://merchant.example/orders/ord_read_test".to_owned(),
    };

    let stored = StoredOrder {
        submission,
        metadata,
        canonical_receipt: receipt,
        authorised_batch: batch,
    };

    let order = UcpAdapter::read_order(&stored)
        .unwrap_or_else(|error| panic!("order read: {error}"));

    assert_eq!(order.id, "ord_read_test");
    assert_eq!(order.checkout_id, "chk_order_read");
    assert_eq!(order.currency, *b"GBP");
    assert_eq!(order.total_minor, 12500);
    assert_ne!(order.receipt_digest, [0; 32]);
}

#[test]
fn payment_handler_mismatch_refuses_negotiation() {
    let merchant_handler = PaymentHandler::new(
        PAYMENT_HANDLER_ID,
        PAYMENT_HANDLER_VERSION,
        PAYMENT_HANDLER_SPEC,
        PAYMENT_HANDLER_SCHEMA,
    )
    .unwrap_or_else(|error| panic!("merchant handler: {error}"));

    let different_handler = PaymentHandler::new(
        "different-handler",
        PAYMENT_HANDLER_VERSION,
        PAYMENT_HANDLER_SPEC,
        PAYMENT_HANDLER_SCHEMA,
    )
    .unwrap_or_else(|error| panic!("different handler: {error}"));

    let platform = PlatformProfile {
        profile_url: "https://platform.example/profile".to_owned(),
        capabilities: vec![
            Capability::new(CHECKOUT_CAPABILITY, UCP_VERSION, "https://ucp.dev/checkout", "https://ucp.dev/checkout.json")
                .unwrap_or_else(|error| panic!("checkout: {error}")),
        ],
        payment_handlers: vec![different_handler],
    };

    let refused = NegotiatedCapabilities::negotiate(&platform, &merchant_handler);
    assert!(matches!(refused, Err(UcpError::PaymentHandlerUnavailable)));
}
