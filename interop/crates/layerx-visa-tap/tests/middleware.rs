use layerx_interop_gateway::principal::PrincipalId;
use layerx_interop_gateway::trace::TraceId;
use layerx_proof::receipt::AuthorizedBatch;
use layerx_visa_tap::{
    bind_verified_agent, prepare_trusted_intent, AgentIntent, AgentPublicKey, CredentialBinding,
    CredentialBindingStore, KeyStatus, LayerXIntentAuthority, MerchantCredentialStatus,
    MerchantOperationResult, NonceWindow, RegisteredAgentKey, TapError, TapRequest, TapVerifier,
    TrustedAgentRegistry, TrustedCommerceIntent, VerifiedTrustedAgent,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

const NOW: u64 = 1_735_689_700;
const KEY_ID: &str = "tap-credential-xyz";
const AGENT: [u8; 32] = [0xAA; 32];
const MERCHANT_PRINCIPAL: &str = "merchant-shop-alpha";

struct MockTrustedAgentRegistry {
    keys: HashMap<String, RegisteredAgentKey>,
}

impl MockTrustedAgentRegistry {
    fn new() -> Self {
        Self {
            keys: HashMap::new(),
        }
    }

    fn register(&mut self, key: RegisteredAgentKey) {
        self.keys.insert(key.key_id.clone(), key);
    }
}

impl TrustedAgentRegistry for MockTrustedAgentRegistry {
    fn resolve(&self, key_id: &str, _now: u64) -> Result<RegisteredAgentKey, TapError> {
        self.keys
            .get(key_id)
            .cloned()
            .ok_or(TapError::UnknownKey)
    }
}

struct MerchantBindingStore {
    bindings: Arc<Mutex<HashMap<String, Vec<CredentialBinding>>>>,
}

impl MerchantBindingStore {
    fn new() -> Self {
        Self {
            bindings: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn get_bindings(&self, principal: &PrincipalId) -> Vec<CredentialBinding> {
        self.bindings
            .lock()
            .unwrap()
            .get(principal.as_str())
            .cloned()
            .unwrap_or_default()
    }
}

impl CredentialBindingStore for MerchantBindingStore {
    fn put(
        &mut self,
        principal: &PrincipalId,
        binding: &CredentialBinding,
        _trace: &TraceId,
    ) -> Result<(), TapError> {
        self.bindings
            .lock()
            .unwrap()
            .entry(principal.as_str().to_owned())
            .or_default()
            .push(binding.clone());
        Ok(())
    }
}

struct MockIntentAuthority {
    refused: bool,
}

impl MockIntentAuthority {
    fn new(refused: bool) -> Self {
        Self { refused }
    }
}

impl LayerXIntentAuthority for MockIntentAuthority {
    type Intent = String;

    fn compile(
        &mut self,
        intent: &TrustedCommerceIntent,
        _trace: &TraceId,
    ) -> Result<Self::Intent, TapError> {
        if self.refused {
            Err(TapError::IntentRefused)
        } else {
            Ok(format!(
                "Intent(agent={:?}, trusted_agent={}, intent={:?})",
                intent.layerx_agent, intent.trusted_agent_id, intent.intent
            ))
        }
    }
}

fn create_tap_signature() -> (ed25519_dalek::SigningKey, String, String) {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;
    use ed25519_dalek::Signer as _;

    let signing = ed25519_dalek::SigningKey::from_bytes(&[0x42; 32]);
    let created = NOW - 10;
    let expires = NOW + 300;
    let nonce = "merchant-session-abc123";
    let tag = "agent-payer-auth";

    let parameters = format!(
        "(\"@authority\" \"@path\");created={created};keyid=\"{KEY_ID}\";alg=\"Ed25519\";expires={expires};nonce=\"{nonce}\";tag=\"{tag}\""
    );
    let base = format!(
        "\"@authority\": shop.example.com\n\"@path\": /api/checkout\n\"@signature-params\": {parameters}"
    );
    let signature = STANDARD.encode(signing.sign(base.as_bytes()).to_bytes());

    (signing, parameters, signature)
}

#[test]
fn merchant_middleware_verifies_and_surfaces_credential_status() {
    let (signing_key, parameters, signature) = create_tap_signature();
    let request = TapRequest::parse(
        "shop.example.com",
        "/api/checkout",
        &format!("sig2={parameters}"),
        &format!("sig2=:{signature}:"),
    )
    .unwrap_or_else(|error| panic!("request must parse: {error}"));

    let mut registry = MockTrustedAgentRegistry::new();
    registry.register(RegisteredAgentKey {
        key_id: KEY_ID.to_owned(),
        agent_id: "visa-commerce-agent-5".to_owned(),
        agent_domain: "https://commerce.example".to_owned(),
        layerx_agent: Some(AGENT),
        key: AgentPublicKey::Ed25519(signing_key.verifying_key().to_bytes()),
        status: KeyStatus::Active,
        expires_at: NOW + 86400,
    });

    let verified = TapVerifier::verify(&request, &registry, &mut NonceWindow::new(), NOW)
        .unwrap_or_else(|error| panic!("verification must succeed: {error}"));

    assert_eq!(verified.agent_id, "visa-commerce-agent-5");
    assert_eq!(verified.layerx_agent, Some(AGENT));
    assert_eq!(verified.intent, AgentIntent::Pay);

    let merchant_status = TapError::Expired.merchant_status();
    assert_eq!(merchant_status, MerchantCredentialStatus::Expired);

    let revoked_status = TapError::Revoked.merchant_status();
    assert_eq!(revoked_status, MerchantCredentialStatus::Revoked);

    let replay_status = TapError::Replay.merchant_status();
    assert_eq!(replay_status, MerchantCredentialStatus::Replayed);
}

#[test]
fn seller_middleware_binds_credentials_without_granting_protocol_authority() {
    let (signing_key, parameters, signature) = create_tap_signature();
    let request = TapRequest::parse(
        "shop.example.com",
        "/api/checkout",
        &format!("sig2={parameters}"),
        &format!("sig2=:{signature}:"),
    )
    .unwrap_or_else(|error| panic!("request must parse: {error}"));

    let mut registry = MockTrustedAgentRegistry::new();
    registry.register(RegisteredAgentKey {
        key_id: KEY_ID.to_owned(),
        agent_id: "visa-commerce-agent-7".to_owned(),
        agent_domain: "https://commerce.example".to_owned(),
        layerx_agent: Some(AGENT),
        key: AgentPublicKey::Ed25519(signing_key.verifying_key().to_bytes()),
        status: KeyStatus::Active,
        expires_at: NOW + 86400,
    });

    let verified = TapVerifier::verify(&request, &registry, &mut NonceWindow::new(), NOW)
        .unwrap_or_else(|error| panic!("verification must succeed: {error}"));

    let principal = PrincipalId::new(MERCHANT_PRINCIPAL)
        .unwrap_or_else(|error| panic!("principal must parse: {error}"));
    let trace = TraceId::mint([0x88; 16]);
    let mut store = MerchantBindingStore::new();

    let binding =
        bind_verified_agent(&principal, AGENT, &verified, &mut store, &trace)
            .unwrap_or_else(|error| panic!("binding must succeed: {error}"));

    assert_eq!(binding.layerx_agent, AGENT);
    assert_eq!(binding.trusted_agent_id, "visa-commerce-agent-7");
    assert_eq!(binding.trusted_agent_domain, "https://commerce.example");

    let stored = store.get_bindings(&principal);
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0], binding);

    let wrong_agent = [0xBB; 32];
    let mismatch = bind_verified_agent(&principal, wrong_agent, &verified, &mut store, &trace);
    assert_eq!(mismatch, Err(TapError::LayerxAgentMismatch));
}

#[test]
fn merchant_middleware_surfaces_operation_result_through_verified_receipts() {
    let canonical_receipt = b"not-a-real-receipt";
    let batch = AuthorizedBatch::new([1; 32], [2; 32], [3; 32], [4; 32], [5; 32]);

    let result = MerchantOperationResult::from_receipt(canonical_receipt, &batch);
    assert_eq!(result, Err(TapError::ReceiptMismatch));

    let pending = MerchantOperationResult::Pending;
    assert_eq!(pending, MerchantOperationResult::Pending);

    let refused = MerchantOperationResult::Refused;
    assert_eq!(refused, MerchantOperationResult::Refused);
}

#[test]
fn trusted_intent_handoff_to_canonical_authority_preserves_non_authoritative_constraint() {
    let (signing_key, parameters, signature) = create_tap_signature();
    let request = TapRequest::parse(
        "shop.example.com",
        "/api/checkout",
        &format!("sig2={parameters}"),
        &format!("sig2=:{signature}:"),
    )
    .unwrap_or_else(|error| panic!("request must parse: {error}"));

    let mut registry = MockTrustedAgentRegistry::new();
    registry.register(RegisteredAgentKey {
        key_id: KEY_ID.to_owned(),
        agent_id: "visa-commerce-agent-11".to_owned(),
        agent_domain: "https://commerce.example".to_owned(),
        layerx_agent: Some(AGENT),
        key: AgentPublicKey::Ed25519(signing_key.verifying_key().to_bytes()),
        status: KeyStatus::Active,
        expires_at: NOW + 86400,
    });

    let verified = TapVerifier::verify(&request, &registry, &mut NonceWindow::new(), NOW)
        .unwrap_or_else(|error| panic!("verification must succeed: {error}"));

    let principal = PrincipalId::new(MERCHANT_PRINCIPAL)
        .unwrap_or_else(|error| panic!("principal must parse: {error}"));
    let trace = TraceId::mint([0x99; 16]);
    let mut store = MerchantBindingStore::new();

    let intent = prepare_trusted_intent(&principal, AGENT, &verified, &mut store, &trace)
        .unwrap_or_else(|error| panic!("intent preparation must succeed: {error}"));

    assert_eq!(intent.principal, principal);
    assert_eq!(intent.layerx_agent, AGENT);
    assert_eq!(intent.trusted_agent_id, "visa-commerce-agent-11");
    assert_eq!(intent.intent, AgentIntent::Pay);

    let stored = store.get_bindings(&principal);
    assert_eq!(stored.len(), 1);

    let mut authority = MockIntentAuthority::new(false);
    let compiled = authority
        .compile(&intent, &trace)
        .unwrap_or_else(|error| panic!("compilation must succeed: {error}"));

    assert!(compiled.contains("visa-commerce-agent-11"));
    assert!(compiled.contains("Pay"));

    let mut refusing_authority = MockIntentAuthority::new(true);
    let refusal = refusing_authority.compile(&intent, &trace);
    assert_eq!(refusal, Err(TapError::IntentRefused));
}

#[test]
fn revoked_and_expired_credentials_are_refused_with_typed_merchant_visible_status() {
    let (signing_key, parameters, signature) = create_tap_signature();
    let request = TapRequest::parse(
        "shop.example.com",
        "/api/checkout",
        &format!("sig2={parameters}"),
        &format!("sig2=:{signature}:"),
    )
    .unwrap_or_else(|error| panic!("request must parse: {error}"));

    let mut registry = MockTrustedAgentRegistry::new();
    registry.register(RegisteredAgentKey {
        key_id: KEY_ID.to_owned(),
        agent_id: "visa-commerce-agent-13".to_owned(),
        agent_domain: "https://commerce.example".to_owned(),
        layerx_agent: Some(AGENT),
        key: AgentPublicKey::Ed25519(signing_key.verifying_key().to_bytes()),
        status: KeyStatus::Revoked,
        expires_at: NOW + 86400,
    });

    let revoked_result = TapVerifier::verify(&request, &registry, &mut NonceWindow::new(), NOW);
    assert_eq!(revoked_result, Err(TapError::Revoked));
    assert_eq!(
        TapError::Revoked.merchant_status(),
        MerchantCredentialStatus::Revoked
    );

    let expired_past = NOW - 100;
    let mut expired_registry = MockTrustedAgentRegistry::new();
    expired_registry.register(RegisteredAgentKey {
        key_id: KEY_ID.to_owned(),
        agent_id: "visa-commerce-agent-14".to_owned(),
        agent_domain: "https://commerce.example".to_owned(),
        layerx_agent: Some(AGENT),
        key: AgentPublicKey::Ed25519(signing_key.verifying_key().to_bytes()),
        status: KeyStatus::Active,
        expires_at: expired_past,
    });

    let expired_result =
        TapVerifier::verify(&request, &expired_registry, &mut NonceWindow::new(), NOW);
    assert_eq!(expired_result, Err(TapError::ExpiredKey));
    assert_eq!(
        TapError::ExpiredKey.merchant_status(),
        MerchantCredentialStatus::Expired
    );
}

#[test]
fn malformed_credentials_are_refused_with_typed_status() {
    let result = TapRequest::parse(
        "shop.example.com",
        "/api/checkout",
        "malformed-signature-input",
        "sig2=:not-base64!@#$%:",
    );

    assert!(result.is_err());
    let error = result.unwrap_err();
    assert_eq!(error.merchant_status(), MerchantCredentialStatus::Invalid);
}
