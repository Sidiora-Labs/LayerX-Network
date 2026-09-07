use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use layerx_ap2::{
    Ap2Error, KeyResolver, KeyUse, MandateMode, MandateVerifier, ProtectedHeader,
    VerificationContext,
};
use p256::ecdsa::signature::Signer as _;
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};

const NOW: u64 = 1_700_000_000;
const ISSUER_KID: &str = "issuer-001";
const MERCHANT_KID: &str = "merchant-001";
const ALTERNATE_ISSUER_KID: &str = "issuer-002";

struct TestKeyResolver {
    issuer: VerifyingKey,
    merchant: VerifyingKey,
}

impl TestKeyResolver {
    fn authentic() -> Self {
        Self {
            issuer: *issuer_key().verifying_key(),
            merchant: *merchant_key().verifying_key(),
        }
    }
}

impl KeyResolver for TestKeyResolver {
    fn resolve(&self, usage: KeyUse, header: &ProtectedHeader) -> Result<VerifyingKey, Ap2Error> {
        match (usage, header.key_id()) {
            (KeyUse::CheckoutMandateIssuer | KeyUse::PaymentMandateIssuer, Some(ISSUER_KID)) => {
                Ok(self.issuer)
            }
            (KeyUse::PaymentMandateIssuer, Some(ALTERNATE_ISSUER_KID)) => {
                Ok(*alternate_issuer_key().verifying_key())
            }
            (KeyUse::MerchantCheckout, Some(MERCHANT_KID)) => Ok(self.merchant),
            _ => Err(Ap2Error::KeyResolution),
        }
    }
}

fn issuer_key() -> SigningKey {
    SigningKey::from_bytes((&[7_u8; 32]).into())
        .unwrap_or_else(|error| panic!("valid issuer scalar: {error:?}"))
}
fn merchant_key() -> SigningKey {
    SigningKey::from_bytes((&[11_u8; 32]).into())
        .unwrap_or_else(|error| panic!("valid merchant scalar: {error:?}"))
}
fn agent_key() -> SigningKey {
    SigningKey::from_bytes((&[19_u8; 32]).into())
        .unwrap_or_else(|error| panic!("valid agent scalar: {error:?}"))
}
fn alternate_issuer_key() -> SigningKey {
    SigningKey::from_bytes((&[23_u8; 32]).into())
        .unwrap_or_else(|error| panic!("valid alternate issuer scalar: {error:?}"))
}

fn context() -> VerificationContext<'static> {
    VerificationContext {
        now: NOW,
        clock_skew_seconds: 300,
        expected_audience: "test-merchant-api",
        expected_nonce: "nonce-abc123xyz",
        currency_minor_exponent: 2,
        usage: None,
    }
}

fn vector(source: &str, expected_generator: &str) -> Value {
    let value: Value = serde_json::from_str(source)
        .unwrap_or_else(|error| panic!("golden vector specification is valid JSON: {error:?}"));
    assert_eq!(value["generator"], expected_generator);
    assert!(!source.to_ascii_lowercase().contains("placeholder"));
    value
}

fn jws(header: &Value, payload: &Value, key: &SigningKey) -> String {
    let header = URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(header).unwrap_or_else(|error| panic!("serializable header: {error:?}")),
    );
    let payload = URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(payload)
            .unwrap_or_else(|error| panic!("serializable payload: {error:?}")),
    );
    let signing_input = format!("{header}.{payload}");
    let signature: Signature = key.sign(signing_input.as_bytes());
    format!(
        "{signing_input}.{}",
        URL_SAFE_NO_PAD.encode(signature.to_bytes())
    )
}

fn root(mandate: Value) -> String {
    let mut payload = json!({"iat": NOW - 60, "exp": NOW + 3_600});
    payload["delegate_payload"] = Value::Array(vec![mandate]);
    format!(
        "{}~",
        jws(
            &json!({"alg":"ES256","typ":"dc+sd-jwt","kid":ISSUER_KID}),
            &payload,
            &issuer_key()
        )
    )
}

fn alternate_root(mandate: Value) -> String {
    let mut payload = json!({"iat": NOW - 60, "exp": NOW + 3_600});
    payload["delegate_payload"] = Value::Array(vec![mandate]);
    format!(
        "{}~",
        jws(
            &json!({"alg":"ES256","typ":"dc+sd-jwt","kid":ALTERNATE_ISSUER_KID}),
            &payload,
            &alternate_issuer_key()
        )
    )
}

fn agent_jwk() -> Value {
    let key = agent_key();
    let point = key.verifying_key().to_encoded_point(false);
    json!({"kty":"EC","crv":"P-256","x":URL_SAFE_NO_PAD.encode(point.x().unwrap_or_else(|| panic!("x coordinate"))),"y":URL_SAFE_NO_PAD.encode(point.y().unwrap_or_else(|| panic!("y coordinate")))})
}

fn key_bound(open: &str, mandate: Value) -> String {
    let mut payload = json!({"iat":NOW-30,"exp":NOW+1_800,"aud":"test-merchant-api","nonce":"nonce-abc123xyz","sd_hash":URL_SAFE_NO_PAD.encode(Sha256::digest(open.as_bytes()))});
    payload["delegate_payload"] = Value::Array(vec![mandate]);
    format!(
        "{}~",
        jws(
            &json!({"alg":"ES256","typ":"kb+sd-jwt"}),
            &payload,
            &agent_key()
        )
    )
}

fn merchant_checkout(amount: u128, id: &str) -> String {
    jws(
        &json!({"alg":"ES256","typ":"JWT","kid":MERCHANT_KID}),
        &json!({
            "id":id,"merchant":{"id":"merchant-001","name":"Test Merchant"},
            "line_items":[{"id":"line-001","item":{"id":"sku-001","title":"LayerX node credit","price":amount},"quantity":1,"totals":[{"type":"total","amount":amount}]}],
            "status":"completed","currency":"USD","totals":[{"type":"total","amount":amount}],"links":[]
        }),
        &merchant_key(),
    )
}

fn closed_checkout(checkout_jwt: &str) -> Value {
    json!({"vct":"mandate.checkout.1","checkout_jwt":checkout_jwt,"checkout_hash":URL_SAFE_NO_PAD.encode(Sha256::digest(checkout_jwt.as_bytes())),"iat":NOW-30,"exp":NOW+1_800})
}

fn closed_payment(checkout_jwt: &str, amount: u128) -> Value {
    json!({"vct":"mandate.payment.1","transaction_id":URL_SAFE_NO_PAD.encode(Sha256::digest(checkout_jwt.as_bytes())),"payee":{"id":"merchant-001","name":"Test Merchant"},"payment_amount":{"amount":amount,"currency":"USD"},"payment_instrument":{"id":"card-001","type":"card"},"iat":NOW-30,"exp":NOW+1_800})
}

fn direct_pair(amount: u128) -> (String, String) {
    let checkout_jwt = merchant_checkout(amount, "checkout-12345");
    (
        root(closed_checkout(&checkout_jwt)),
        root(closed_payment(&checkout_jwt, amount)),
    )
}

fn autonomous_pair(amount: u128, maximum: u128) -> (String, String) {
    let open_checkout = root(
        json!({"vct":"mandate.checkout.open.1","constraints":[{"type":"checkout.line_items","items":[{"id":"requested-credit","acceptable_items":[{"id":"sku-001","title":"LayerX node credit"}],"quantity":1}]}],"cnf":{"jwk":agent_jwk()},"iat":NOW-60,"exp":NOW+3_600}),
    );
    let reference = URL_SAFE_NO_PAD.encode(Sha256::digest(open_checkout.as_bytes()));
    let open_payment = root(
        json!({"vct":"mandate.payment.open.1","constraints":[{"type":"payment.reference","conditional_transaction_id":reference},{"type":"payment.amount_range","currency":"USD","min":1,"max":maximum}],"cnf":{"jwk":agent_jwk()},"iat":NOW-60,"exp":NOW+3_600}),
    );
    let checkout_jwt = merchant_checkout(amount, "checkout-67890");
    let checkout = key_bound(&open_checkout, closed_checkout(&checkout_jwt));
    let payment = key_bound(&open_payment, closed_payment(&checkout_jwt, amount));
    (
        format!("{open_checkout}~~{checkout}"),
        format!("{open_payment}~~{payment}"),
    )
}

fn corrupt_signature(presentation: &str) -> String {
    let first = presentation
        .find('.')
        .unwrap_or_else(|| panic!("header separator"));
    let start = presentation[first + 1..]
        .find('.')
        .map_or_else(|| panic!("payload separator"), |next| first + next + 2);
    let mut bytes = presentation.as_bytes().to_vec();
    bytes[start] = if bytes[start] == b'A' { b'B' } else { b'A' };
    String::from_utf8(bytes)
        .unwrap_or_else(|error| panic!("base64url mutation is UTF-8: {error:?}"))
}

#[test]
fn authentic_direct_vector_verifies_expected_values() {
    let spec = vector(
        include_str!("vectors/direct/001-minimal-valid.json"),
        "mandates::direct_pair",
    );
    let amount = u128::from(
        spec["expected_values"]["amount_minor_units"]
            .as_u64()
            .unwrap_or_else(|| panic!("minor-unit amount")),
    );
    let (checkout, payment) = direct_pair(amount);
    let resolver = TestKeyResolver::authentic();
    let verified = MandateVerifier::new(&resolver)
        .verify(&checkout, &payment, &context())
        .unwrap_or_else(|error| panic!("authentic direct pair verifies: {error:?}"));
    assert_eq!(verified.mode(), MandateMode::Direct);
    assert_eq!(
        verified.checkout_id(),
        spec["expected_values"]["checkout_id"]
            .as_str()
            .unwrap_or_else(|| panic!("checkout id"))
    );
    assert_eq!(
        verified.payee().id(),
        spec["expected_values"]["payee_id"]
            .as_str()
            .unwrap_or_else(|| panic!("payee id"))
    );
    assert_eq!(verified.amount().minor_units(), amount);
    assert_eq!(
        verified.amount().currency(),
        spec["expected_values"]["currency"]
            .as_str()
            .unwrap_or_else(|| panic!("currency"))
    );
}

#[test]
fn authentic_autonomous_vector_verifies_key_binding_and_constraints() {
    let _spec = vector(
        include_str!("vectors/autonomous/001-line-items.json"),
        "mandates::autonomous_pair",
    );
    let (checkout, payment) = autonomous_pair(15_000, 20_000);
    let resolver = TestKeyResolver::authentic();
    let verified = MandateVerifier::new(&resolver)
        .verify(&checkout, &payment, &context())
        .unwrap_or_else(|error| panic!("authentic autonomous pair verifies: {error:?}"));
    assert_eq!(verified.mode(), MandateMode::Autonomous);
    assert_eq!(verified.checkout_id(), "checkout-67890");
    assert_eq!(verified.amount().minor_units(), 15_000);
}

#[test]
fn authentic_signature_corruption_is_refused_cryptographically() {
    let _spec = vector(
        include_str!("vectors/refusals/002-invalid-signature.json"),
        "mandates::direct_pair",
    );
    let (checkout, payment) = direct_pair(10_000);
    let resolver = TestKeyResolver::authentic();
    let result =
        MandateVerifier::new(&resolver).verify(&corrupt_signature(&checkout), &payment, &context());
    assert!(matches!(result, Err(Ap2Error::InvalidSignature)));
}

#[test]
fn authentic_payment_binding_corruption_is_refused_after_signature_verification() {
    let _spec = vector(
        include_str!("vectors/refusals/003-binding-mismatch.json"),
        "mandates::direct_pair",
    );
    let checkout_jwt = merchant_checkout(10_000, "checkout-12345");
    let checkout = root(closed_checkout(&checkout_jwt));
    let mut mandate = closed_payment(&checkout_jwt, 10_000);
    mandate["transaction_id"] = Value::String(URL_SAFE_NO_PAD.encode([0_u8; 32]));
    let resolver = TestKeyResolver::authentic();
    let result = MandateVerifier::new(&resolver).verify(&checkout, &root(mandate), &context());
    assert!(matches!(result, Err(Ap2Error::PaymentBindingMismatch)));
}

#[test]
fn authentic_expired_vector_is_refused_after_signature_verification() {
    let _spec = vector(
        include_str!("vectors/refusals/001-expired.json"),
        "mandates::root",
    );
    let checkout_jwt = merchant_checkout(10_000, "checkout-12345");
    let mut mandate = closed_checkout(&checkout_jwt);
    mandate["iat"] = json!(NOW - 7_200);
    mandate["exp"] = json!(NOW - 3_600);
    let resolver = TestKeyResolver::authentic();
    let result = MandateVerifier::new(&resolver).verify(
        &root(mandate),
        &root(closed_payment(&checkout_jwt, 10_000)),
        &context(),
    );
    assert!(matches!(result, Err(Ap2Error::Expired)));
}

#[test]
fn authentic_amount_violation_reaches_constraint_evaluation() {
    let spec = vector(
        include_str!("vectors/constraints/001-amount-range-exceeded.json"),
        "mandates::autonomous_pair",
    );
    let amount = u128::from(
        spec["authentic_amount_minor_units"]
            .as_u64()
            .unwrap_or_else(|| panic!("authentic amount")),
    );
    let maximum = u128::from(
        spec["constraint_max_minor_units"]
            .as_u64()
            .unwrap_or_else(|| panic!("constraint maximum")),
    );
    let (checkout, payment) = autonomous_pair(amount, maximum);
    let resolver = TestKeyResolver::authentic();
    let result = MandateVerifier::new(&resolver).verify(&checkout, &payment, &context());
    assert!(matches!(
        result,
        Err(Ap2Error::ConstraintViolated("payment.amount_range"))
    ));
}

#[test]
fn presentation_bounds_are_enforced_before_parsing() {
    let resolver = TestKeyResolver::authentic();
    let oversized = "a".repeat(300_000);
    let result = MandateVerifier::new(&resolver).verify(&oversized, &oversized, &context());
    assert!(matches!(result, Err(Ap2Error::Bounds)));
}

#[test]
fn empty_presentations_are_refused_at_the_bound() {
    let resolver = TestKeyResolver::authentic();
    let result = MandateVerifier::new(&resolver).verify("", "", &context());
    assert!(matches!(result, Err(Ap2Error::Bounds)));
}

#[test]
fn authentic_mismatched_issuers_are_refused() {
    let checkout_jwt = merchant_checkout(10_000, "checkout-12345");
    let checkout = root(closed_checkout(&checkout_jwt));
    let payment = alternate_root(closed_payment(&checkout_jwt, 10_000));
    let resolver = TestKeyResolver::authentic();
    let result = MandateVerifier::new(&resolver).verify(&checkout, &payment, &context());
    assert!(matches!(result, Err(Ap2Error::InvalidSignature)));
}

#[test]
fn authentic_not_yet_valid_mandate_is_refused() {
    let checkout_jwt = merchant_checkout(10_000, "checkout-12345");
    let mut mandate = closed_checkout(&checkout_jwt);
    mandate["iat"] = json!(NOW + 3_600);
    mandate["exp"] = json!(NOW + 7_200);
    let resolver = TestKeyResolver::authentic();
    let result = MandateVerifier::new(&resolver).verify(
        &root(mandate),
        &root(closed_payment(&checkout_jwt, 10_000)),
        &context(),
    );
    assert!(matches!(result, Err(Ap2Error::NotYetValid)));
}

#[test]
fn authentic_autonomous_audience_mismatch_is_refused() {
    let (checkout, payment) = autonomous_pair(15_000, 20_000);
    let resolver = TestKeyResolver::authentic();
    let mut wrong_context = context();
    wrong_context.expected_audience = "other-merchant-api";
    let result = MandateVerifier::new(&resolver).verify(&checkout, &payment, &wrong_context);
    assert!(matches!(result, Err(Ap2Error::AudienceMismatch)));
}

#[test]
fn signed_payment_amount_mismatch_is_refused() {
    let checkout_jwt = merchant_checkout(10_000, "checkout-12345");
    let resolver = TestKeyResolver::authentic();
    let result = MandateVerifier::new(&resolver).verify(
        &root(closed_checkout(&checkout_jwt)),
        &root(closed_payment(&checkout_jwt, 20_000)),
        &context(),
    );
    assert!(matches!(result, Err(Ap2Error::PaymentBindingMismatch)));
}

#[test]
fn unsupported_algorithm_is_refused_before_key_resolution() {
    let token = format!(
        "{}~",
        jws(
            &json!({"alg":"RS256","kid":ISSUER_KID}),
            &json!({"delegate_payload":[]}),
            &issuer_key()
        )
    );
    let resolver = TestKeyResolver::authentic();
    let result = MandateVerifier::new(&resolver).verify(&token, &token, &context());
    assert!(matches!(result, Err(Ap2Error::UnsupportedAlgorithm)));
}
