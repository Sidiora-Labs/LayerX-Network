use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use ed25519_dalek::{Signer as _, SigningKey};
use layerx_interop_gateway::principal::PrincipalId;
use layerx_interop_gateway::trace::TraceId;
use layerx_proof::receipt::AuthorizedBatch;
use layerx_visa_tap::{
    bind_verified_agent, prepare_trusted_intent, AgentIntent, AgentPublicKey, CredentialBinding,
    CredentialBindingStore, KeyStatus, MerchantOperationResult, NonceWindow, RegisteredAgentKey,
    TapError, TapRequest, TapVerifier, TrustedAgentRegistry,
};

const NOW: u64 = 1_735_689_700;
const KEY_ID: &str = "poqkLGiymh_W0uP6PZFw-dvez3QJT5SolqXBCW38r0U";
const AGENT: [u8; 32] = [0x44; 32];

struct Registry {
    key: RegisteredAgentKey,
}

impl TrustedAgentRegistry for Registry {
    fn resolve(&self, key_id: &str, _now: u64) -> Result<RegisteredAgentKey, TapError> {
        (key_id == KEY_ID)
            .then(|| self.key.clone())
            .ok_or(TapError::UnknownKey)
    }
}

#[derive(Default)]
struct Bindings(Vec<CredentialBinding>);

impl CredentialBindingStore for Bindings {
    fn put(
        &mut self,
        _principal: &PrincipalId,
        binding: &CredentialBinding,
        _trace: &TraceId,
    ) -> Result<(), TapError> {
        self.0.push(binding.clone());
        Ok(())
    }
}

fn signed_request(signing: &SigningKey, nonce: &str, tag: &str, extension: &str) -> TapRequest {
    let parameters = format!(
        "(\"@authority\" \"@path\");created={};keyid=\"{KEY_ID}\";alg=\"Ed25519\";expires={};nonce=\"{nonce}\";tag=\"{tag}\"{extension}",
        NOW - 1,
        NOW + 479
    );
    let base = format!(
        "\"@authority\": shop.example\n\"@path\": /checkout\n\"@signature-params\": {parameters}"
    );
    let signature = STANDARD.encode(signing.sign(base.as_bytes()).to_bytes());
    TapRequest::parse(
        "shop.example",
        "/checkout",
        &format!("sig2={parameters}"),
        &format!("sig2=:{signature}:"),
    )
    .unwrap_or_else(|error| panic!("official-shape request must parse: {error}"))
}

fn registry(signing: &SigningKey, status: KeyStatus, expires_at: u64) -> Registry {
    Registry {
        key: RegisteredAgentKey {
            key_id: KEY_ID.to_owned(),
            agent_id: "visa-agent-7".to_owned(),
            agent_domain: "https://agent.example".to_owned(),
            layerx_agent: Some(AGENT),
            key: AgentPublicKey::Ed25519(signing.verifying_key().to_bytes()),
            status,
            expires_at,
        },
    }
}

#[test]
fn verifies_official_header_shape_and_preserves_signed_extensions() {
    let signing = SigningKey::from_bytes(&[0x31; 32]);
    let request = signed_request(
        &signing,
        "unique-session-1",
        "agent-payer-auth",
        ";scheme=\"visa\"",
    );
    let verified = TapVerifier::verify(
        &request,
        &registry(&signing, KeyStatus::Active, NOW + 1_000),
        &mut NonceWindow::new(),
        NOW,
    )
    .unwrap_or_else(|error| panic!("valid signature must verify: {error}"));
    assert_eq!(verified.intent, AgentIntent::Pay);
    assert_eq!(verified.layerx_agent, Some(AGENT));
}

#[test]
fn target_tampering_replay_expiry_and_revocation_are_typed_refusals() {
    let signing = SigningKey::from_bytes(&[0x32; 32]);
    let request = signed_request(&signing, "unique-session-2", "agent-browser-auth", "");
    let active = registry(&signing, KeyStatus::Active, NOW + 1_000);
    let mut nonces = NonceWindow::new();
    TapVerifier::verify(&request, &active, &mut nonces, NOW)
        .unwrap_or_else(|error| panic!("first request must verify: {error}"));
    assert_eq!(
        TapVerifier::verify(&request, &active, &mut nonces, NOW),
        Err(TapError::Replay)
    );
    assert_eq!(
        TapVerifier::verify(&request, &active, &mut NonceWindow::new(), NOW + 480),
        Err(TapError::Expired)
    );
    let revoked = registry(&signing, KeyStatus::Revoked, NOW + 1_000);
    assert_eq!(
        TapVerifier::verify(&request, &revoked, &mut NonceWindow::new(), NOW),
        Err(TapError::Revoked)
    );
    let altered = TapRequest::parse(
        "evil.example",
        "/checkout",
        &format!("sig2={}", request_parameters()),
        &signature_for(&signing, &request_parameters()),
    )
    .unwrap_or_else(|error| panic!("altered request must remain syntactically valid: {error}"));
    assert_eq!(
        TapVerifier::verify(&altered, &active, &mut NonceWindow::new(), NOW),
        Err(TapError::InvalidSignature)
    );
}

#[test]
fn server_time_and_bounded_skew_refuse_validity_manipulation() {
    let signing = SigningKey::from_bytes(&[0x35; 32]);
    let active = registry(&signing, KeyStatus::Active, NOW + 1_000);
    let future = format!(
        "(\"@authority\" \"@path\");created={};keyid=\"{KEY_ID}\";alg=\"Ed25519\";expires={};nonce=\"future-session\";tag=\"agent-payer-auth\"",
        NOW + 61,
        NOW + 300
    );
    let future_request = TapRequest::parse(
        "shop.example",
        "/checkout",
        &format!("sig2={future}"),
        &signature_for(&signing, &future),
    )
    .unwrap_or_else(|error| panic!("future request must parse: {error}"));
    assert_eq!(
        TapVerifier::verify_credential(&future_request, &active, NOW, 60),
        Err(TapError::NotYetValid)
    );

    let expired = format!(
        "(\"@authority\" \"@path\");created={};keyid=\"{KEY_ID}\";alg=\"Ed25519\";expires={};nonce=\"expired-session\";tag=\"agent-payer-auth\"",
        NOW - 479,
        NOW - 60
    );
    let expired_request = TapRequest::parse(
        "shop.example",
        "/checkout",
        &format!("sig2={expired}"),
        &signature_for(&signing, &expired),
    )
    .unwrap_or_else(|error| panic!("expired request must parse: {error}"));
    assert_eq!(
        TapVerifier::verify_credential(&expired_request, &active, NOW, 60),
        Err(TapError::Expired)
    );
    assert!(TapVerifier::verify_credential(&expired_request, &active, NOW, 61).is_ok());
    assert_eq!(
        TapVerifier::verify_credential(&future_request, &active, NOW, 301),
        Err(TapError::ClockSkewTooLarge)
    );
}

fn request_parameters() -> String {
    format!(
        "(\"@authority\" \"@path\");created={};keyid=\"{KEY_ID}\";alg=\"Ed25519\";expires={};nonce=\"unique-session-3\";tag=\"agent-payer-auth\"",
        NOW - 1,
        NOW + 479
    )
}

fn signature_for(signing: &SigningKey, parameters: &str) -> String {
    let base = format!(
        "\"@authority\": shop.example\n\"@path\": /checkout\n\"@signature-params\": {parameters}"
    );
    format!(
        "sig2=:{}:",
        STANDARD.encode(signing.sign(base.as_bytes()).to_bytes())
    )
}

#[test]
fn binding_is_scoped_non_authoritative_and_success_requires_a_real_receipt() {
    let signing = SigningKey::from_bytes(&[0x33; 32]);
    let verified = TapVerifier::verify(
        &signed_request(&signing, "unique-session-4", "agent-payer-auth", ""),
        &registry(&signing, KeyStatus::Active, NOW + 1_000),
        &mut NonceWindow::new(),
        NOW,
    )
    .unwrap_or_else(|error| panic!("credential must verify: {error}"));
    let principal = PrincipalId::new("merchant-1")
        .unwrap_or_else(|error| panic!("principal must parse: {error}"));
    let trace = TraceId::mint([7; 16]);
    let mut bindings = Bindings::default();
    let intent = prepare_trusted_intent(&principal, AGENT, &verified, &mut bindings, &trace)
        .unwrap_or_else(|error| panic!("typed intent must be prepared: {error}"));
    assert_eq!(intent.principal, principal);
    assert_eq!(intent.intent, AgentIntent::Pay);
    assert_eq!(bindings.0.len(), 1);
    assert_eq!(
        bind_verified_agent(&principal, [0x55; 32], &verified, &mut bindings, &trace),
        Err(TapError::LayerxAgentMismatch)
    );
    let batch = AuthorizedBatch::new([1; 32], [2; 32], [3; 32], [4; 32], [5; 32]);
    assert_eq!(
        MerchantOperationResult::from_receipt(b"not a LayerX receipt", &batch),
        Err(TapError::ReceiptMismatch)
    );
}
