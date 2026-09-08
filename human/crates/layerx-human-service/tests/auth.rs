use layerx_human_test_support as support;

use std::fmt::Display;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use ciborium::value::Value as CborValue;
use ed25519_dalek::{Signer as _, SigningKey};
use layerx_human_service::auth::{
    AccessDecision, AccountIdentity, AuthConfig, AuthError, AuthorizationRequest, Device,
    OperationClass, OperationDigest, Passkeys, RateLimit,
};
use layerx_human_service::store::Table;
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};

use support::{directory, install_and_open, principal, retention_uniform, tenancy};

const RP_ID: &str = "id.layerx.example";
const ORIGIN: &str = "https://id.layerx.example";
const FLAG_UP: u8 = 1 << 0;
const FLAG_UV: u8 = 1 << 2;
const FLAG_AT: u8 = 1 << 6;

fn required<T, E: Display>(result: Result<T, E>, label: &str) -> T {
    result.unwrap_or_else(|error| panic!("{label}: {error}"))
}

fn config(attempts: u32) -> AuthConfig {
    AuthConfig {
        rp_id: RP_ID.to_owned(),
        rp_name: "LayerX".to_owned(),
        origin: ORIGIN.to_owned(),
        ceremony_ttl_secs: 300,
        assertion_ttl_secs: 30,
        session_ttl_secs: 30,
        refresh_ttl_secs: 300,
        step_up_ttl_secs: 10,
        rate_limit: RateLimit {
            attempts,
            window_secs: 60,
        },
    }
}

fn decode_ceremony(value: &str) -> Value {
    let bytes = required(URL_SAFE_NO_PAD.decode(value), "decode ceremony");
    required(serde_json::from_slice(&bytes), "parse ceremony")
}

fn required_text<'a>(value: &'a Value, pointer: &str) -> &'a str {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("missing ceremony field {pointer}"))
}

fn encode_response(value: &Value) -> String {
    URL_SAFE_NO_PAD.encode(required(serde_json::to_vec(value), "serialize response"))
}

/// A software `WebAuthn` authenticator exercising the same Ed25519 signature,
/// authenticator-data, CBOR attestation and monotonic-counter path as a
/// hardware/platform authenticator.
struct SoftwareAuthenticator {
    signing_key: SigningKey,
    credential_id: Vec<u8>,
    counter: u32,
    user_handle: Option<String>,
}

impl SoftwareAuthenticator {
    fn new() -> Self {
        let mut seed = [0_u8; 32];
        required(getrandom::fill(&mut seed), "authenticator entropy");
        let mut credential_id = vec![0_u8; 32];
        required(
            getrandom::fill(&mut credential_id),
            "credential identifier entropy",
        );
        Self {
            signing_key: SigningKey::from_bytes(&seed),
            credential_id,
            counter: 0,
            user_handle: None,
        }
    }

    fn register(&mut self, ceremony: &str) -> String {
        let options = decode_ceremony(ceremony);
        let challenge = required_text(&options, "/challenge");
        self.user_handle = Some(required_text(&options, "/user/id").to_owned());
        let client_data = client_data("webauthn.create", challenge, ORIGIN);
        encode_response(&json!({
            "id": URL_SAFE_NO_PAD.encode(&self.credential_id),
            "transports": ["internal"],
            "attestationObject": URL_SAFE_NO_PAD.encode(self.attestation_object()),
            "clientDataJSON": URL_SAFE_NO_PAD.encode(client_data),
        }))
    }

    fn assert(&mut self, ceremony: &str) -> String {
        self.assert_for_origin(ceremony, ORIGIN)
    }

    fn assert_for_origin(&mut self, ceremony: &str, origin: &str) -> String {
        let options = decode_ceremony(ceremony);
        let challenge = required_text(&options, "/challenge");
        self.counter = self.counter.saturating_add(1);
        let authenticator_data = self.authenticator_data(self.counter, false);
        let client_data = client_data("webauthn.get", challenge, origin);
        let client_hash = Sha256::digest(&client_data);
        let mut signed = Vec::with_capacity(authenticator_data.len() + client_hash.len());
        signed.extend_from_slice(&authenticator_data);
        signed.extend_from_slice(&client_hash);
        let signature = self.signing_key.sign(&signed).to_bytes();
        encode_response(&json!({
            "id": URL_SAFE_NO_PAD.encode(&self.credential_id),
            "authenticatorData": URL_SAFE_NO_PAD.encode(authenticator_data),
            "signature": URL_SAFE_NO_PAD.encode(signature),
            "clientDataJSON": URL_SAFE_NO_PAD.encode(client_data),
            "userHandle": self.user_handle,
        }))
    }

    fn attestation_object(&self) -> Vec<u8> {
        let map = CborValue::Map(vec![
            (
                CborValue::Text("fmt".to_owned()),
                CborValue::Text("none".to_owned()),
            ),
            (
                CborValue::Text("attStmt".to_owned()),
                CborValue::Map(Vec::new()),
            ),
            (
                CborValue::Text("authData".to_owned()),
                CborValue::Bytes(self.authenticator_data(0, true)),
            ),
        ]);
        let mut bytes = Vec::new();
        required(
            ciborium::ser::into_writer(&map, &mut bytes),
            "encode attestation",
        );
        bytes
    }

    fn authenticator_data(&self, counter: u32, attested: bool) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&Sha256::digest(RP_ID.as_bytes()));
        bytes.push(FLAG_UP | FLAG_UV | if attested { FLAG_AT } else { 0 });
        bytes.extend_from_slice(&counter.to_be_bytes());
        if attested {
            bytes.extend_from_slice(&[0_u8; 16]);
            let credential_length = u16::try_from(self.credential_id.len())
                .unwrap_or_else(|_| panic!("credential identifier too long"));
            bytes.extend_from_slice(&credential_length.to_be_bytes());
            bytes.extend_from_slice(&self.credential_id);
            bytes.extend_from_slice(&self.cose_public_key());
        }
        bytes
    }

    fn cose_public_key(&self) -> Vec<u8> {
        let public_key = self.signing_key.verifying_key().to_bytes().to_vec();
        let map = CborValue::Map(vec![
            (CborValue::Integer(1.into()), CborValue::Integer(1.into())),
            (
                CborValue::Integer(3.into()),
                CborValue::Integer((-8).into()),
            ),
            (
                CborValue::Integer((-1).into()),
                CborValue::Integer(6.into()),
            ),
            (
                CborValue::Integer((-2).into()),
                CborValue::Bytes(public_key),
            ),
        ]);
        let mut bytes = Vec::new();
        required(
            ciborium::ser::into_writer(&map, &mut bytes),
            "encode public key",
        );
        bytes
    }
}

fn client_data(kind: &str, challenge: &str, origin: &str) -> Vec<u8> {
    required(
        serde_json::to_vec(&json!({
            "type": kind,
            "challenge": challenge,
            "origin": origin,
            "crossOrigin": false,
        })),
        "encode client data",
    )
}

fn register_and_open_session(
    passkeys: &Passkeys,
    scope: &mut layerx_human_service::store::PrincipalScope<'_>,
    authenticator: &mut SoftwareAuthenticator,
    device: Device,
    now: u64,
) -> layerx_human_service::auth::SessionGrant {
    let identity = required(
        AccountIdentity::new("mara@example.com", "Mara"),
        "account identity",
    );
    let registration = required(
        passkeys.begin_registration(scope, &identity, "Phone passkey", now),
        "begin registration",
    );
    let registration_response = authenticator.register(&registration.ceremony);
    required(
        passkeys.finish_registration(
            scope,
            &registration.registration_id,
            &registration_response,
            now.saturating_add(1),
        ),
        "finish registration",
    );
    let assertion = required(
        passkeys.begin_assertion(scope, now.saturating_add(2)),
        "begin assertion",
    );
    let assertion_response = authenticator.assert(&assertion.ceremony);
    required(
        passkeys.finish_assertion(
            scope,
            &assertion.assertion_id,
            &assertion_response,
            now.saturating_add(3),
        ),
        "finish assertion",
    );
    required(
        passkeys.open_session(
            scope,
            &assertion.assertion_id,
            device,
            now.saturating_add(4),
        ),
        "open session",
    )
}

#[test]
#[allow(clippy::too_many_lines)]
fn real_passkey_session_step_up_and_expiry_are_fail_closed() {
    let root = directory("auth-real-path");
    let map = tenancy(&[("alice", "tenant-a"), ("bob", "tenant-b")]);
    let (mut store, _) = install_and_open(&root, &map, retention_uniform(10_000));
    let mut scope = required(store.principal(&principal("alice")), "principal scope");
    let passkeys = required(Passkeys::new(config(100)), "passkey service");
    let mut authenticator = SoftwareAuthenticator::new();
    let device = required(
        Device::new("dev_aabbccddeeff00112233445566778899", "Phone", "ios"),
        "device",
    );
    let session = register_and_open_session(&passkeys, &mut scope, &mut authenticator, device, 100);
    let debug_grant = format!("{session:?}");
    assert!(!debug_grant.contains(session.access_token().expose()));
    assert!(!debug_grant.contains(session.refresh_token().expose()));
    assert!(!debug_grant.contains(session.csrf_token().expose()));

    let notification_keys = scope.keys(Table::Notifications);
    assert_eq!(notification_keys.len(), 1);
    let notification_row = scope
        .get(Table::Notifications, &notification_keys[0])
        .unwrap_or_else(|| panic!("new-device notification row missing"));
    let notification: Value = required(
        serde_json::from_slice(notification_row.bytes()),
        "decode new-device notification",
    );
    assert_eq!(
        notification.pointer("/class").and_then(Value::as_str),
        Some("security")
    );
    assert_eq!(
        notification.pointer("/deep_link").and_then(Value::as_str),
        Some("/app/settings/devices")
    );
    assert_eq!(
        notification
            .pointer("/action_copy_key")
            .and_then(Value::as_str),
        Some("notification.action.review-devices")
    );
    let missing_csrf = passkeys.authorize(
        &mut scope,
        session.access_token().expose(),
        None,
        &AuthorizationRequest {
            operation: OperationClass::MoneyMovement,
            digest: None,
            step_up: None,
            intended_destination: "/move/review",
        },
        105,
    );
    assert!(matches!(missing_csrf, Err(AuthError::ForgeryRefused)));

    let digest = OperationDigest::new([7_u8; 32]);
    let challenge = required(
        passkeys.begin_step_up(
            &mut scope,
            session.access_token().expose(),
            session.csrf_token().expose(),
            digest,
            106,
        ),
        "begin step-up",
    );
    let response = authenticator.assert(&challenge.ceremony);
    let evidence = required(
        passkeys.finish_step_up(&mut scope, &challenge.challenge_id, &response, 107),
        "finish step-up",
    );
    let authorized = required(
        passkeys.authorize(
            &mut scope,
            session.access_token().expose(),
            Some(session.csrf_token().expose()),
            &AuthorizationRequest {
                operation: OperationClass::Withdrawal,
                digest: Some(digest),
                step_up: Some(&evidence),
                intended_destination: "/withdraw/review",
            },
            108,
        ),
        "authorize withdrawal",
    );
    assert!(matches!(authorized, AccessDecision::Authorized(_)));

    let mismatch = passkeys.authorize(
        &mut scope,
        session.access_token().expose(),
        Some(session.csrf_token().expose()),
        &AuthorizationRequest {
            operation: OperationClass::Withdrawal,
            digest: Some(OperationDigest::new([8_u8; 32])),
            step_up: Some(&evidence),
            intended_destination: "/withdraw/review",
        },
        109,
    );
    assert!(matches!(mismatch, Err(AuthError::StepUpMismatch)));

    let expired_step_up = passkeys.authorize(
        &mut scope,
        session.access_token().expose(),
        Some(session.csrf_token().expose()),
        &AuthorizationRequest {
            operation: OperationClass::Withdrawal,
            digest: Some(digest),
            step_up: Some(&evidence),
            intended_destination: "/withdraw/review",
        },
        evidence.expires_at().saturating_add(1),
    );
    assert!(matches!(expired_step_up, Err(AuthError::StepUpExpired)));

    for designated in [
        OperationClass::Approval,
        OperationClass::Withdrawal,
        OperationClass::Exit,
        OperationClass::SecuritySettings,
        OperationClass::SecretReveal,
        OperationClass::WalletRebind,
        OperationClass::AgentArchive,
    ] {
        assert!(designated.requires_step_up());
    }

    let expired = required(
        passkeys.authorize(
            &mut scope,
            session.access_token().expose(),
            Some(session.csrf_token().expose()),
            &AuthorizationRequest {
                operation: OperationClass::MoneyMovement,
                digest: None,
                step_up: None,
                intended_destination: "/move/review",
            },
            session.access_expires_at().saturating_add(1),
        ),
        "expired decision",
    );
    assert_eq!(
        expired,
        AccessDecision::Reauthenticate {
            intended_destination: "/move/review".to_owned(),
        }
    );

    drop(scope);
    let mut bob = required(store.principal(&principal("bob")), "bob scope");
    assert!(matches!(
        passkeys.authorize(
            &mut bob,
            session.access_token().expose(),
            None,
            &AuthorizationRequest {
                operation: OperationClass::Read,
                digest: None,
                step_up: None,
                intended_destination: "/activity",
            },
            110,
        ),
        Err(AuthError::Unauthenticated)
    ));
    drop(bob);
    drop(store);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn refresh_revocation_inventory_and_sign_out_everywhere_cover_refresh_paths() {
    let root = directory("auth-sessions");
    let map = tenancy(&[("alice", "tenant-a")]);
    let (mut store, _) = install_and_open(&root, &map, retention_uniform(10_000));
    let mut scope = required(store.principal(&principal("alice")), "principal scope");
    let passkeys = required(Passkeys::new(config(100)), "passkey service");
    let mut authenticator = SoftwareAuthenticator::new();
    let phone = required(
        Device::new("dev_11111111111111111111111111111111", "Phone", "ios"),
        "phone",
    );
    let first = register_and_open_session(&passkeys, &mut scope, &mut authenticator, phone, 200);

    let assertion = required(
        passkeys.begin_assertion(&mut scope, 210),
        "second assertion",
    );
    let assertion_response = authenticator.assert(&assertion.ceremony);
    required(
        passkeys.finish_assertion(
            &mut scope,
            &assertion.assertion_id,
            &assertion_response,
            211,
        ),
        "finish second assertion",
    );
    let desktop = required(
        Device::new("dev_22222222222222222222222222222222", "Desktop", "web"),
        "desktop",
    );
    let second = required(
        passkeys.open_session(&mut scope, &assertion.assertion_id, desktop, 212),
        "open second session",
    );
    let inventory = required(
        passkeys.list_sessions(&mut scope, first.access_token().expose(), 213),
        "session inventory",
    );
    assert_eq!(inventory.len(), 2);
    assert_eq!(scope.keys(Table::Notifications).len(), 2);

    let rotated = required(
        passkeys.refresh_session(
            &mut scope,
            first.refresh_token().expose(),
            first.csrf_token().expose(),
            214,
        ),
        "refresh",
    );
    assert!(matches!(
        passkeys.list_sessions(&mut scope, first.access_token().expose(), 215),
        Err(AuthError::Unauthenticated)
    ));
    let revoked = required(
        passkeys.revoke_session(
            &mut scope,
            rotated.access_token().expose(),
            rotated.csrf_token().expose(),
            second.session_id(),
            216,
        ),
        "revoke second",
    );
    assert_eq!(
        revoked.revoked_session_ids,
        vec![second.session_id().to_owned()]
    );
    assert!(matches!(
        passkeys.refresh_session(
            &mut scope,
            second.refresh_token().expose(),
            second.csrf_token().expose(),
            217,
        ),
        Err(AuthError::Unauthenticated)
    ));

    let signed_out = required(
        passkeys.sign_out_everywhere(
            &mut scope,
            rotated.access_token().expose(),
            rotated.csrf_token().expose(),
            218,
        ),
        "sign out everywhere",
    );
    assert_eq!(
        signed_out.revoked_session_ids,
        vec![rotated.session_id().to_owned()]
    );
    assert!(matches!(
        passkeys.refresh_session(
            &mut scope,
            rotated.refresh_token().expose(),
            rotated.csrf_token().expose(),
            219,
        ),
        Err(AuthError::Unauthenticated)
    ));

    drop(scope);
    drop(store);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
#[allow(clippy::too_many_lines)]
fn fallback_is_one_time_read_only_and_never_satisfies_step_up() {
    let root = directory("auth-fallback");
    let map = tenancy(&[("alice", "tenant-a")]);
    let (mut store, _) = install_and_open(&root, &map, retention_uniform(10_000));
    let mut scope = required(store.principal(&principal("alice")), "principal scope");
    let passkeys = required(Passkeys::new(config(100)), "passkey service");
    let mut authenticator = SoftwareAuthenticator::new();
    let primary_device = required(
        Device::new("dev_33333333333333333333333333333333", "Primary", "ios"),
        "primary device",
    );
    let primary = register_and_open_session(
        &passkeys,
        &mut scope,
        &mut authenticator,
        primary_device,
        300,
    );
    let fallback_digest = OperationDigest::new([9_u8; 32]);
    let challenge = required(
        passkeys.begin_step_up(
            &mut scope,
            primary.access_token().expose(),
            primary.csrf_token().expose(),
            fallback_digest,
            306,
        ),
        "begin fallback step-up",
    );
    let response = authenticator.assert(&challenge.ceremony);
    let evidence = required(
        passkeys.finish_step_up(&mut scope, &challenge.challenge_id, &response, 307),
        "finish fallback step-up",
    );
    let fallback = required(
        passkeys.replace_fallback_credential(
            &mut scope,
            primary.access_token().expose(),
            primary.csrf_token().expose(),
            fallback_digest,
            &evidence,
            308,
            600,
        ),
        "replace fallback",
    );
    let recovery_device = required(
        Device::new("dev_44444444444444444444444444444444", "Recovery", "web"),
        "recovery device",
    );
    let recovery = required(
        passkeys.authenticate_fallback(
            &mut scope,
            fallback.secret().expose(),
            recovery_device.clone(),
            309,
        ),
        "fallback authenticate",
    );
    let read = required(
        passkeys.authorize(
            &mut scope,
            recovery.access_token().expose(),
            None,
            &AuthorizationRequest {
                operation: OperationClass::Read,
                digest: None,
                step_up: None,
                intended_destination: "/activity",
            },
            310,
        ),
        "fallback read",
    );
    assert!(matches!(read, AccessDecision::Authorized(_)));
    assert!(matches!(
        passkeys.authorize(
            &mut scope,
            recovery.access_token().expose(),
            Some(recovery.csrf_token().expose()),
            &AuthorizationRequest {
                operation: OperationClass::MoneyMovement,
                digest: None,
                step_up: None,
                intended_destination: "/move/review",
            },
            311,
        ),
        Err(AuthError::FallbackRestricted)
    ));
    assert!(matches!(
        passkeys.begin_step_up(
            &mut scope,
            recovery.access_token().expose(),
            recovery.csrf_token().expose(),
            OperationDigest::new([10_u8; 32]),
            312,
        ),
        Err(AuthError::FallbackRestricted)
    ));
    assert!(matches!(
        passkeys.authenticate_fallback(
            &mut scope,
            fallback.secret().expose(),
            recovery_device,
            313,
        ),
        Err(AuthError::FallbackRefused)
    ));

    drop(scope);
    drop(store);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn typed_rate_limit_is_scoped_per_principal_and_carries_retry_timing() {
    let root = directory("auth-rate");
    let map = tenancy(&[("alice", "tenant-a"), ("bob", "tenant-b")]);
    let (mut store, _) = install_and_open(&root, &map, retention_uniform(10_000));
    let passkeys = required(Passkeys::new(config(2)), "passkey service");
    {
        let mut alice = required(store.principal(&principal("alice")), "alice scope");
        assert!(matches!(
            passkeys.begin_assertion(&mut alice, 500),
            Err(AuthError::NoPasskeys)
        ));
        assert!(matches!(
            passkeys.begin_assertion(&mut alice, 501),
            Err(AuthError::NoPasskeys)
        ));
        assert!(matches!(
            passkeys.begin_assertion(&mut alice, 502),
            Err(AuthError::RateLimited {
                retry_at: 560,
                retry_after_secs: 58,
            })
        ));
    }
    {
        let mut bob = required(store.principal(&principal("bob")), "bob scope");
        assert!(matches!(
            passkeys.begin_assertion(&mut bob, 502),
            Err(AuthError::NoPasskeys)
        ));
    }

    drop(store);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn origin_tampering_and_challenge_replay_are_refused() {
    let root = directory("auth-tamper");
    let map = tenancy(&[("alice", "tenant-a")]);
    let (mut store, _) = install_and_open(&root, &map, retention_uniform(10_000));
    let mut scope = required(store.principal(&principal("alice")), "principal scope");
    let passkeys = required(Passkeys::new(config(100)), "passkey service");
    let mut authenticator = SoftwareAuthenticator::new();
    let identity = required(
        AccountIdentity::new("mara@example.com", "Mara"),
        "account identity",
    );
    let registration = required(
        passkeys.begin_registration(&mut scope, &identity, "Passkey", 600),
        "begin registration",
    );
    let registration_response = authenticator.register(&registration.ceremony);
    required(
        passkeys.finish_registration(
            &mut scope,
            &registration.registration_id,
            &registration_response,
            601,
        ),
        "finish registration",
    );
    let assertion = required(passkeys.begin_assertion(&mut scope, 602), "begin assertion");
    let hostile = authenticator.assert_for_origin(&assertion.ceremony, "https://evil.example");
    let Err(hostile_error) =
        passkeys.finish_assertion(&mut scope, &assertion.assertion_id, &hostile, 603)
    else {
        panic!("hostile origin unexpectedly authenticated");
    };
    assert!(matches!(&hostile_error, AuthError::Passkey(_)));
    let redacted = format!("{hostile_error:?}");
    assert!(!redacted.contains("evil.example"));
    assert!(!redacted.contains(&hostile));

    let valid_assertion = required(passkeys.begin_assertion(&mut scope, 604), "fresh assertion");
    let valid = authenticator.assert(&valid_assertion.ceremony);
    required(
        passkeys.finish_assertion(&mut scope, &valid_assertion.assertion_id, &valid, 605),
        "finish fresh assertion",
    );
    let device = required(
        Device::new("dev_55555555555555555555555555555555", "Phone", "ios"),
        "device",
    );
    required(
        passkeys.open_session(
            &mut scope,
            &valid_assertion.assertion_id,
            device.clone(),
            606,
        ),
        "open session",
    );
    assert!(matches!(
        passkeys.open_session(&mut scope, &valid_assertion.assertion_id, device, 607,),
        Err(AuthError::AssertionSpent)
    ));

    drop(scope);
    drop(store);
    let _ = std::fs::remove_dir_all(root);
}
