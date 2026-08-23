use layerx_ucp::{
    Capability, CheckoutStatus, CheckoutSubmission, ExecutedUcpPayment, MerchantProfile,
    NegotiatedCapabilities, OrderMetadata, PaymentHandler, PlatformProfile, StoredOrder,
    UcpAdapter, UcpError, UcpIdempotencyKey, UcpPaymentIntent, UcpPaymentPlane, UcpPlaneResult,
};
use layerx_interop_gateway::principal::PrincipalId;
use layerx_interop_gateway::trace::TraceId;
use layerx_interop_gateway::GatewayCore;
use layerx_proof::receipt::AuthorizedBatch;
use ed25519_dalek::{Signer as _, SigningKey};
use sha2::{Digest as _, Sha256};
use std::collections::HashMap;

const UCP_VERSION: &str = "2026-04-08";
const CHECKOUT_CAPABILITY: &str = "dev.ucp.shopping.checkout";
const ORDER_CAPABILITY: &str = "dev.ucp.shopping.order";

struct UcpClient {
    profile_url: String,
    capabilities: Vec<Capability>,
    payment_handlers: Vec<PaymentHandler>,
}

impl UcpClient {
    fn new() -> Result<Self, UcpError> {
        Ok(Self {
            profile_url: "https://buyer-platform.example/ucp-profile".to_owned(),
            capabilities: vec![
                Capability::new(
                    CHECKOUT_CAPABILITY,
                    UCP_VERSION,
                    "https://ucp.dev/2026-04-08/specification/checkout",
                    "https://ucp.dev/2026-04-08/schemas/shopping/checkout.json",
                )?,
                Capability::new(
                    ORDER_CAPABILITY,
                    UCP_VERSION,
                    "https://ucp.dev/2026-04-08/specification/order",
                    "https://ucp.dev/2026-04-08/schemas/shopping/order.json",
                )?,
            ],
            payment_handlers: vec![PaymentHandler::new(
                "layerx-402",
                "2.0.0",
                "https://layerx.dev/2026-04-08/specification/402",
                "https://layerx.dev/2026-04-08/schemas/402.json",
            )?],
        })
    }

    fn platform_profile(&self) -> PlatformProfile {
        PlatformProfile {
            profile_url: self.profile_url.clone(),
            capabilities: self.capabilities.clone(),
            payment_handlers: self.payment_handlers.clone(),
        }
    }
}

struct LayerXSeller {
    gateway: GatewayCore,
    principal: PrincipalId,
    merchant_profile: MerchantProfile,
    payment_plane: TestSellerPlane,
}

impl LayerXSeller {
    fn new() -> Result<Self, UcpError> {
        let handler = PaymentHandler::new(
            "layerx-402",
            "2.0.0",
            "https://layerx.dev/2026-04-08/specification/402",
            "https://layerx.dev/2026-04-08/schemas/402.json",
        )?;

        let merchant_profile = MerchantProfile::layerx(
            "https://merchant.example/ucp-rest",
            handler,
        )?;

        let principal = PrincipalId::new("seller-merchant")
            .map_err(|_| UcpError::InvalidProfile)?;

        Ok(Self {
            gateway: layerx_interop_gateway::interop_gateway_core(),
            principal,
            merchant_profile,
            payment_plane: TestSellerPlane::new(),
        })
    }

    fn process_checkout(
        &mut self,
        submission: CheckoutSubmission,
        trace: &TraceId,
        now: u64,
    ) -> Result<CheckoutStatus, UcpError> {
        let outcome = UcpAdapter::complete_checkout(
            &mut self.gateway,
            &self.principal,
            &submission,
            &mut self.payment_plane,
            trace,
            now,
        )
        .map_err(|traced| traced.into_error())?;

        if outcome.order.is_some() {
            let order = outcome.order.unwrap();
            self.payment_plane.store_order(
                submission.clone(),
                order.id.clone(),
                order.permalink_url.clone(),
            );
        }

        Ok(outcome.status)
    }

    fn get_order(&self, order_id: &str) -> Result<layerx_ucp::UcpOrder, UcpError> {
        let stored = self
            .payment_plane
            .orders
            .get(order_id)
            .ok_or(UcpError::InvalidOrder)?;

        UcpAdapter::read_order(stored)
    }
}

struct TestSellerPlane {
    sequencer_seed: [u8; 32],
    pending: HashMap<[u8; 32], ()>,
    orders: HashMap<String, StoredOrder>,
}

impl TestSellerPlane {
    fn new() -> Self {
        Self {
            sequencer_seed: [0x88; 32],
            pending: HashMap::new(),
            orders: HashMap::new(),
        }
    }

    fn store_order(
        &mut self,
        submission: CheckoutSubmission,
        order_id: String,
        permalink_url: String,
    ) {
        if let Some((receipt, batch)) = self.get_executed_receipt(submission.idempotency_key.gateway_key()) {
            let metadata = OrderMetadata {
                order_id: order_id.clone(),
                permalink_url,
            };

            self.orders.insert(
                order_id,
                StoredOrder {
                    submission,
                    metadata,
                    canonical_receipt: receipt,
                    authorised_batch: batch,
                },
            );
        }
    }

    fn get_executed_receipt(&self, _key: [u8; 32]) -> Option<(Vec<u8>, AuthorizedBatch)> {
        None
    }
}

impl UcpPaymentPlane for TestSellerPlane {
    fn execute(
        &mut self,
        intent: &UcpPaymentIntent,
        _trace: &TraceId,
    ) -> Result<UcpPlaneResult, UcpError> {
        if self.pending.contains_key(&intent.idempotency_key) {
            self.pending.remove(&intent.idempotency_key);

            let (receipt, batch) = signed_receipt(
                200,
                intent.idempotency_key,
                intent.amount,
                intent.asset,
                intent.recipient,
                self.sequencer_seed,
            );

            let metadata = OrderMetadata {
                order_id: format!("ord_{}", hex(&intent.idempotency_key[..8])),
                permalink_url: format!(
                    "https://merchant.example/orders/{}",
                    hex(&intent.idempotency_key[..8])
                ),
            };

            return Ok(UcpPlaneResult::Executed(Box::new(ExecutedUcpPayment {
                metadata,
                canonical_receipt: receipt,
                authorised_batch: batch,
            })));
        }

        self.pending.insert(intent.idempotency_key, ());
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
            b"layerx-ucp-e2e/v1".as_slice(),
            &sequence.to_be_bytes(),
            &idempotency_key,
        ]
        .concat(),
    )
    .into();

    let previous_state_root: [u8; 32] =
        Sha256::digest([b"before".as_slice(), &activity_id].concat()).into();
    let resulting_state_root: [u8; 32] =
        Sha256::digest([b"after".as_slice(), &activity_id].concat()).into();
    let batch_id: [u8; 32] = Sha256::digest([b"batch".as_slice(), &activity_id].concat()).into();

    let signer = SigningKey::from_bytes(&sequencer_seed);
    let unsigned = encode_receipt(
        &activity_id,
        sequence,
        &previous_state_root,
        &resulting_state_root,
        &batch_id,
        &asset,
        amount,
        &recipient,
        None,
    );

    let mut digest = Sha256::new();
    digest.update(b"LXP/v1/receipt\0");
    digest.update(&unsigned);
    let signature = signer.sign(&<[u8; 32]>::from(digest.finalize()));

    let canonical_receipt = encode_receipt(
        &activity_id,
        sequence,
        &previous_state_root,
        &resulting_state_root,
        &batch_id,
        &asset,
        amount,
        &recipient,
        Some(signature.to_bytes()),
    );

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
    let sender = [0xa2; 32];
    let debit_before = 100_000_u128;
    let credit_before = 5_000_u128;

    let mut bytes = Vec::new();
    push_u16(&mut bytes, 1);
    push_u16(&mut bytes, 0x5201);
    push_u16(&mut bytes, 1);
    push_bytes(&mut bytes, activity_id);
    push_u64(&mut bytes, sequence);
    push_bytes(&mut bytes, previous_state_root);
    push_bytes(&mut bytes, resulting_state_root);
    push_bytes(&mut bytes, &[0x82; 32]);
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
    let length = u32::try_from(value.len())
        .unwrap_or_else(|_| panic!("receipt field overflow"));
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(value);
}

#[test]
fn ucp_client_completes_checkout_with_layerx_seller() {
    let client = UcpClient::new().unwrap_or_else(|error| panic!("client setup: {error}"));
    let mut seller = LayerXSeller::new().unwrap_or_else(|error| panic!("seller setup: {error}"));

    let merchant_handler = seller.merchant_profile.payment_handlers()[0].clone();
    let negotiated = NegotiatedCapabilities::negotiate(&client.platform_profile(), &merchant_handler)
        .unwrap_or_else(|error| panic!("capability negotiation: {error}"));

    assert!(negotiated.checkout(), "client and seller must agree on checkout capability");

    let asset = [0xe1; 32];
    let recipient = [0xe2; 32];
    let idempotency_key = UcpIdempotencyKey::parse("aaaabbbb-cccc-dddd-eeee-ffff00001111")
        .unwrap_or_else(|error| panic!("idempotency key: {error}"));

    let submission = CheckoutSubmission {
        checkout_id: "chk_e2e_test".to_owned(),
        currency: *b"USD",
        total_minor: 25000,
        layerx_asset: asset,
        layerx_recipient: recipient,
        idempotency_key,
        negotiated,
    };

    let trace = TraceId::mint([0xe3; 16]);

    let first_attempt = seller
        .process_checkout(submission.clone(), &trace, 100)
        .unwrap_or_else(|error| panic!("first checkout attempt: {error}"));
    assert_eq!(
        first_attempt,
        CheckoutStatus::CompleteInProgress,
        "first attempt must be pending"
    );

    let second_attempt = seller
        .process_checkout(submission.clone(), &trace, 101)
        .unwrap_or_else(|error| panic!("second checkout attempt: {error}"));
    assert_eq!(
        second_attempt,
        CheckoutStatus::Completed,
        "second attempt must complete with receipt"
    );
}

#[test]
fn ucp_client_retrieves_order_with_verified_receipt() {
    let client = UcpClient::new().unwrap_or_else(|error| panic!("client setup: {error}"));
    let mut seller = LayerXSeller::new().unwrap_or_else(|error| panic!("seller setup: {error}"));

    let merchant_handler = seller.merchant_profile.payment_handlers()[0].clone();
    let negotiated = NegotiatedCapabilities::negotiate(&client.platform_profile(), &merchant_handler)
        .unwrap_or_else(|error| panic!("negotiation: {error}"));

    let asset = [0xf1; 32];
    let recipient = [0xf2; 32];
    let idempotency_key = UcpIdempotencyKey::parse("11112222-3333-4444-5555-666677778888")
        .unwrap_or_else(|error| panic!("idempotency key: {error}"));

    let submission = CheckoutSubmission {
        checkout_id: "chk_order_retrieve".to_owned(),
        currency: *b"EUR",
        total_minor: 15000,
        layerx_asset: asset,
        layerx_recipient: recipient,
        idempotency_key,
        negotiated,
    };

    let trace = TraceId::mint([0xf3; 16]);

    seller
        .process_checkout(submission.clone(), &trace, 200)
        .unwrap_or_else(|error| panic!("first attempt: {error}"));

    let status = seller
        .process_checkout(submission.clone(), &trace, 201)
        .unwrap_or_else(|error| panic!("second attempt: {error}"));
    assert_eq!(status, CheckoutStatus::Completed);

    let order_id = format!("ord_{}", hex(&idempotency_key.gateway_key()[..8]));
    let order = seller
        .get_order(&order_id)
        .unwrap_or_else(|error| panic!("order retrieval: {error}"));

    assert_eq!(order.checkout_id, "chk_order_retrieve");
    assert_eq!(order.currency, *b"EUR");
    assert_eq!(order.total_minor, 15000);
    assert_ne!(order.receipt_digest, [0; 32], "order must carry verified receipt digest");
}

#[test]
fn ucp_client_and_seller_maintain_idempotency_across_retries() {
    let client = UcpClient::new().unwrap_or_else(|error| panic!("client setup: {error}"));
    let mut seller = LayerXSeller::new().unwrap_or_else(|error| panic!("seller setup: {error}"));

    let merchant_handler = seller.merchant_profile.payment_handlers()[0].clone();
    let negotiated = NegotiatedCapabilities::negotiate(&client.platform_profile(), &merchant_handler)
        .unwrap_or_else(|error| panic!("negotiation: {error}"));

    let asset = [0xb1; 32];
    let recipient = [0xb2; 32];
    let idempotency_key = UcpIdempotencyKey::parse("deadbeef-1234-5678-9abc-def012345678")
        .unwrap_or_else(|error| panic!("idempotency key: {error}"));

    let submission = CheckoutSubmission {
        checkout_id: "chk_idempotency".to_owned(),
        currency: *b"GBP",
        total_minor: 7500,
        layerx_asset: asset,
        layerx_recipient: recipient,
        idempotency_key,
        negotiated,
    };

    let trace = TraceId::mint([0xb3; 16]);

    seller
        .process_checkout(submission.clone(), &trace, 300)
        .unwrap_or_else(|error| panic!("attempt 1: {error}"));

    let completed = seller
        .process_checkout(submission.clone(), &trace, 301)
        .unwrap_or_else(|error| panic!("attempt 2: {error}"));
    assert_eq!(completed, CheckoutStatus::Completed);

    let replay = seller
        .process_checkout(submission.clone(), &trace, 302)
        .unwrap_or_else(|error| panic!("replay: {error}"));
    assert_eq!(
        replay,
        CheckoutStatus::Completed,
        "idempotent replay must return same completed status"
    );
}
