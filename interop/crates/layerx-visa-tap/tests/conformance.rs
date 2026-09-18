//! Visa Trusted Agent Protocol conformance harness. The vectors are the
//! first-party suite under `interop/specs/conformance/visa-tap`, which the
//! gateway deployment pins by identifier, vector count and digest. Every record
//! carries the exact RFC 9421 signature-input and target wire strings and runs
//! through the production `TapRequest` and `TapVerifier` types; the credential
//! signature itself is produced from the record's own key seed, because a
//! signature over a request is a live signer's output and cannot be a literal
//! that stays valid for another target. The credential-binding case at the end
//! drives the binding store and a canonical receipt, so it stays in Rust and is
//! not a vector.

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use ed25519_dalek::{Signer as _, SigningKey};
use layerx_interop_gateway::principal::PrincipalId;
use layerx_interop_gateway::trace::TraceId;
use layerx_proof::receipt::AuthorizedBatch;
use layerx_visa_tap::{
    bind_verified_agent, prepare_trusted_intent, AgentIntent, AgentPublicKey, CredentialBinding,
    CredentialBindingStore, KeyStatus, MerchantOperationResult, NonceWindow, RegisteredAgentKey,
    TapError, TapRequest, TapVerifier, TrustedAgentRegistry, VerifiedTrustedAgent,
};

const NOW: u64 = 1_735_689_700;
const KEY_ID: &str = "poqkLGiymh_W0uP6PZFw-dvez3QJT5SolqXBCW38r0U";
const AGENT: [u8; 32] = [0x44; 32];

const CREDENTIAL_VERIFICATION: &str =
    include_str!("../../../specs/conformance/visa-tap/credential-verification.json");
const TARGET_CANONICALIZATION: &str =
    include_str!("../../../specs/conformance/visa-tap/target-canonicalization.json");

struct Registry {
    key: RegisteredAgentKey,
}

impl TrustedAgentRegistry for Registry {
    fn resolve(&self, key_id: &str, _now: u64) -> Result<RegisteredAgentKey, TapError> {
        (key_id == self.key.key_id)
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

fn records(suite: &str, source: &str) -> Vec<serde_json::Value> {
    let records: Vec<serde_json::Value> =
        serde_json::from_str(suite).unwrap_or_else(|error| panic!("{source}: {error}"));
    assert!(
        !records.is_empty(),
        "{source}: a suite that carries no vector is not a conformance suite"
    );
    records
}

fn field<'a>(record: &'a serde_json::Value, name: &str, source: &str) -> &'a str {
    record[name]
        .as_str()
        .unwrap_or_else(|| panic!("{source}: every vector declares {name}"))
}

fn number(record: &serde_json::Value, name: &str, source: &str) -> u64 {
    record[name]
        .as_u64()
        .unwrap_or_else(|| panic!("{source}: every vector declares {name}"))
}

fn hex32(value: &str, source: &str) -> [u8; 32] {
    assert_eq!(value.len(), 64, "{source}: {value} is not 32 bytes");
    let mut bytes = [0_u8; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .unwrap_or_else(|error| panic!("{source}: {value}: {error}"));
    }
    bytes
}

fn tap_error(name: &str, source: &str) -> TapError {
    match name {
        "MalformedSignatureInput" => TapError::MalformedSignatureInput,
        "MissingCoveredComponent" => TapError::MissingCoveredComponent,
        "MissingSignatureParameter" => TapError::MissingSignatureParameter,
        "DuplicateSignatureParameter" => TapError::DuplicateSignatureParameter,
        "UnknownSignatureParameter" => TapError::UnknownSignatureParameter,
        "InvalidTag" => TapError::InvalidTag,
        "UnsupportedAlgorithm" => TapError::UnsupportedAlgorithm,
        "InvalidTarget" => TapError::InvalidTarget,
        "MalformedSignature" => TapError::MalformedSignature,
        "NotYetValid" => TapError::NotYetValid,
        "Expired" => TapError::Expired,
        "WindowTooLong" => TapError::WindowTooLong,
        "ClockSkewTooLarge" => TapError::ClockSkewTooLarge,
        "UnknownKey" => TapError::UnknownKey,
        "RegistryMismatch" => TapError::RegistryMismatch,
        "Revoked" => TapError::Revoked,
        "ExpiredKey" => TapError::ExpiredKey,
        "AlgorithmMismatch" => TapError::AlgorithmMismatch,
        "InvalidSignature" => TapError::InvalidSignature,
        "Replay" => TapError::Replay,
        other => panic!("{source}: {other} is not a TAP refusal"),
    }
}

fn intent(name: &str, source: &str) -> AgentIntent {
    match name {
        "pay" => AgentIntent::Pay,
        "browse" => AgentIntent::Browse,
        other => panic!("{source}: {other} is not a TAP interaction"),
    }
}

fn signature_base(authority: &str, path: &str, parameters: &str) -> String {
    format!("\"@authority\": {authority}\n\"@path\": {path}\n\"@signature-params\": {parameters}")
}

fn signed_request(signing: &SigningKey, nonce: &str, tag: &str, extension: &str) -> TapRequest {
    let parameters = format!(
        "(\"@authority\" \"@path\");created={};keyid=\"{KEY_ID}\";alg=\"Ed25519\";expires={};nonce=\"{nonce}\";tag=\"{tag}\"{extension}",
        NOW - 1,
        NOW + 479
    );
    let base = signature_base("shop.example", "/checkout", &parameters);
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
fn every_credential_vector_verifies_or_is_refused_exactly_as_declared() {
    let source = "interop/specs/conformance/visa-tap/credential-verification.json";
    let mut verified_intents = Vec::new();
    let mut refusals = Vec::new();
    for record in records(CREDENTIAL_VERIFICATION, source) {
        let name = field(&record, "name", source);
        let parameters = field(&record, "signature_params", source);
        let signing =
            SigningKey::from_bytes(&hex32(field(&record, "signing_key_seed", source), source));
        let base = signature_base(
            field(&record, "signed_authority", source),
            field(&record, "signed_path", source),
            parameters,
        );
        let signature = STANDARD.encode(signing.sign(base.as_bytes()).to_bytes());
        let declared = record["expect"]
            .as_str()
            .unwrap_or_else(|| panic!("{source}: {name} declares no outcome"));
        let expected = (declared != "verified").then(|| tap_error(declared, source));
        let request = TapRequest::parse(
            field(&record, "authority", source),
            field(&record, "path", source),
            &format!("sig2={parameters}"),
            &format!("sig2=:{signature}:"),
        );
        let request = match request {
            Ok(request) => request,
            Err(error) => {
                assert_eq!(Some(error), expected, "{name}: signature-input admission");
                refusals.push(error);
                continue;
            }
        };
        let layerx_agent = hex32(field(&record, "registry_layerx_agent", source), source);
        let registry = Registry {
            key: RegisteredAgentKey {
                key_id: field(&record, "registry_key_id", source).to_owned(),
                agent_id: field(&record, "registry_agent_id", source).to_owned(),
                agent_domain: field(&record, "registry_agent_domain", source).to_owned(),
                layerx_agent: Some(layerx_agent),
                key: AgentPublicKey::Ed25519(signing.verifying_key().to_bytes()),
                status: match field(&record, "key_status", source) {
                    "active" => KeyStatus::Active,
                    "revoked" => KeyStatus::Revoked,
                    other => panic!("{source}: {other} is not a registry key status"),
                },
                expires_at: number(&record, "key_expires_at", source),
            },
        };
        let now = number(&record, "now", source);
        let outcome: Result<VerifiedTrustedAgent, TapError> =
            match record["clock_skew_seconds"].as_u64() {
                Some(skew) => TapVerifier::verify_credential(&request, &registry, now, skew),
                None => {
                    let mut nonces = NonceWindow::new();
                    for delivery in 0..number(&record, "replays", source) {
                        TapVerifier::verify(&request, &registry, &mut nonces, now).unwrap_or_else(
                            |error| panic!("{name}: delivery {delivery} must verify: {error}"),
                        );
                    }
                    TapVerifier::verify(&request, &registry, &mut nonces, now)
                }
            };
        match expected {
            Some(error) => {
                assert_eq!(outcome, Err(error), "{name}");
                refusals.push(error);
            }
            None => {
                let verified = outcome.unwrap_or_else(|error| panic!("{name}: {error}"));
                assert_eq!(
                    verified.intent,
                    intent(
                        record["intent"].as_str().unwrap_or_else(|| panic!(
                            "{name}: a verified vector names its intent"
                        )),
                        source
                    ),
                    "{name}: interaction"
                );
                assert_eq!(verified.layerx_agent, Some(layerx_agent), "{name}: binding");
                assert_eq!(
                    verified.key_id,
                    field(&record, "registry_key_id", source),
                    "{name}: registry key"
                );
                assert_eq!(
                    verified.agent_id,
                    field(&record, "registry_agent_id", source),
                    "{name}: registry agent"
                );
                verified_intents.push(verified.intent);
            }
        }
    }
    for interaction in [AgentIntent::Pay, AgentIntent::Browse] {
        assert!(
            verified_intents.contains(&interaction),
            "{source}: {interaction:?} is not covered by a verified vector"
        );
    }
    for error in [
        TapError::Replay,
        TapError::Expired,
        TapError::Revoked,
        TapError::InvalidSignature,
        TapError::NotYetValid,
        TapError::ClockSkewTooLarge,
    ] {
        assert!(
            refusals.contains(&error),
            "{source}: {error} is not covered by a refusal vector"
        );
    }
}

#[test]
fn target_components_require_one_canonical_authority_and_path_representation() {
    let source = "interop/specs/conformance/visa-tap/target-canonicalization.json";
    for record in records(TARGET_CANONICALIZATION, source) {
        let name = field(&record, "name", source);
        let parsed = TapRequest::parse(
            field(&record, "authority", source),
            field(&record, "path", source),
            field(&record, "signature_input", source),
            field(&record, "signature", source),
        );
        let accepted = record["accepted"]
            .as_bool()
            .unwrap_or_else(|| panic!("{source}: {name} declares whether it is admitted"));
        if accepted {
            let request = parsed.unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(
                request.authority(),
                field(&record, "authority", source),
                "{name}: canonical authority"
            );
            assert_eq!(
                request.path(),
                field(&record, "path", source),
                "{name}: canonical path"
            );
        } else {
            assert_eq!(
                parsed.err(),
                Some(tap_error(field(&record, "refusal", source), source)),
                "{name}"
            );
        }
    }
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
