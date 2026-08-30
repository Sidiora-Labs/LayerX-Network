use layerx_agentd::identity::{
    register, revalidate, CoreIdentity, IdentityError, IdentityResolver, ProtocolAuthority,
};
use layerx_agentd::store::{Store, TenantId};
use layerx_types::ids::Did;
use layerx_types::verify::VerificationLevel;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

struct BoundaryIdentityLedger {
    result: Result<Option<CoreIdentity>, IdentityError>,
    calls: usize,
}

impl IdentityResolver for BoundaryIdentityLedger {
    fn resolve(&mut self, _did: &Did) -> Result<Option<CoreIdentity>, IdentityError> {
        self.calls += 1;
        match &self.result {
            Ok(value) => Ok(value.clone()),
            Err(IdentityError::BoundaryUnavailable) => Err(IdentityError::BoundaryUnavailable),
            Err(IdentityError::UnknownDid) => Err(IdentityError::UnknownDid),
            Err(IdentityError::Frozen) => Err(IdentityError::Frozen),
            Err(IdentityError::Unverified) => Err(IdentityError::Unverified),
            Err(IdentityError::Store(_)) => panic!("boundary ledger cannot return a store error"),
        }
    }
}

fn directory(label: &str) -> PathBuf {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "layerx-agentd-identity-{label}-{}-{sequence}",
        std::process::id()
    ))
}

fn did() -> Did {
    match Did::new(b"did:layerx:agent-7") {
        Ok(value) => value,
        Err(error) => panic!("DID fixture invalid: {error:?}"),
    }
}

fn tenant() -> TenantId {
    match TenantId::new("tenant-a") {
        Ok(value) => value,
        Err(error) => panic!("tenant fixture invalid: {error}"),
    }
}

fn active_identity(head_sequence: u64) -> CoreIdentity {
    CoreIdentity {
        canonical_bytes: format!("identity-at-{head_sequence}").into_bytes(),
        head_sequence,
        revocation_sequence: 1,
        verification_level: VerificationLevel::STATE_PROVEN,
        frozen: false,
        authorities: vec![ProtocolAuthority::SessionKey([7; 32])],
    }
}

#[test]
fn registration_resolves_core_state_and_persists_provenance() {
    let root = directory("register");
    let mut store = Store::open(&root).unwrap_or_else(|error| panic!("open failed: {error}"));
    let mut boundary = BoundaryIdentityLedger {
        result: Ok(Some(active_identity(41))),
        calls: 0,
    };
    let record = register(&mut store, tenant(), did(), &mut boundary)
        .unwrap_or_else(|error| panic!("registration failed: {error:?}"));
    assert_eq!(boundary.calls, 1);
    assert_eq!(record.head_sequence(), 41);
    assert_eq!(record.verification_level(), VerificationLevel::STATE_PROVEN);
    assert_eq!(
        record.authorities(),
        &[ProtocolAuthority::SessionKey([7; 32])]
    );
    assert_eq!(record.canonical_core_bytes(), b"identity-at-41");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn unknown_frozen_unverified_and_unavailable_identities_are_refused() {
    let cases = [
        (Ok(None), IdentityError::UnknownDid),
        (
            Ok(Some(CoreIdentity {
                frozen: true,
                ..active_identity(1)
            })),
            IdentityError::Frozen,
        ),
        (
            Ok(Some(CoreIdentity {
                verification_level: VerificationLevel::UNVERIFIED,
                ..active_identity(1)
            })),
            IdentityError::Unverified,
        ),
        (
            Err(IdentityError::BoundaryUnavailable),
            IdentityError::BoundaryUnavailable,
        ),
    ];
    for (index, (result, expected)) in cases.into_iter().enumerate() {
        let root = directory(&format!("refusal-{index}"));
        let mut store = Store::open(&root).unwrap_or_else(|error| panic!("open failed: {error}"));
        let mut boundary = BoundaryIdentityLedger { result, calls: 0 };
        assert_eq!(
            register(&mut store, tenant(), did(), &mut boundary),
            Err(expected)
        );
        let _ = fs::remove_dir_all(root);
    }
}

#[test]
fn restored_binding_is_revalidated_and_freeze_is_observed() {
    let root = directory("revalidate");
    let mut store = Store::open(&root).unwrap_or_else(|error| panic!("open failed: {error}"));
    let mut active = BoundaryIdentityLedger {
        result: Ok(Some(active_identity(10))),
        calls: 0,
    };
    let record = register(&mut store, tenant(), did(), &mut active)
        .unwrap_or_else(|error| panic!("registration failed: {error:?}"));
    drop(store);

    let mut reopened = Store::open(&root).unwrap_or_else(|error| panic!("reopen failed: {error}"));
    let mut frozen = BoundaryIdentityLedger {
        result: Ok(Some(CoreIdentity {
            frozen: true,
            ..active_identity(11)
        })),
        calls: 0,
    };
    assert_eq!(
        revalidate(&mut reopened, &record, &mut frozen),
        Err(IdentityError::Frozen)
    );
    assert_eq!(frozen.calls, 1);
    let _ = fs::remove_dir_all(root);
}
