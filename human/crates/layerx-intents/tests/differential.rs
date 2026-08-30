use layerx_crypto::session::{issue_session_key, SessionKeyRequest};
use layerx_intents::{
    compile, golden, BridgeDepositCredit, BridgeWithdrawRequest, BudgetCreate, BudgetDefund,
    BudgetFund, DidRegistration, DisclosureCheck, DisclosureCheckError, DisclosureField,
    EvmPayoutBinding, Intent, IntentKind, IntentVersion, KeyRotation, LxpReceive, LxpSend,
    PayerGrantRegistration, RecoveryRegistration, SessionGrant, SessionRevoke,
};
use layerx_types::account::AccountId;
use layerx_types::activity::{Signature, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{AssetId, CheckpointId, Did, IdempotencyKey};
use layerx_types::intent::{
    ApprovalThreshold, AuthorityGrantId, AuthorizationSignature, BudgetId, ContextHash,
    DepositProofId, EvmAddress, GrantSchedule, NetworkId, PayerGrantId, PeriodLength,
    ProtocolVersion, PublicKey, PurposeHash, RecoveryRoot, RolloverPolicy, SendAuthorization,
    SendAuthorizationKind, Sequence, SessionRevocationReason, TimestampSeconds, WithdrawalId,
};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use proptest::prelude::*;

struct Fixture {
    name: &'static str,
    source: &'static str,
    intent: Intent,
}

fn activity(module: ModuleId, ordinal: u16) -> ActivityType {
    ActivityType::new(module, ordinal)
        .unwrap_or_else(|error| panic!("activity {module:?}/{ordinal}: {error:?}"))
}

fn registry() -> ModuleRegistry {
    let registrations = [
        ModuleRegistration::new(
            ModuleId::Governance,
            &[
                activity(ModuleId::Governance, 1),
                activity(ModuleId::Governance, 2),
                activity(ModuleId::Governance, 3),
                activity(ModuleId::Governance, 4),
                activity(ModuleId::Governance, 5),
                activity(ModuleId::Governance, 6),
            ],
        )
        .unwrap_or_else(|error| panic!("governance: {error:?}")),
        ModuleRegistration::new(
            ModuleId::Asset,
            &[activity(ModuleId::Asset, 5), activity(ModuleId::Asset, 6)],
        )
        .unwrap_or_else(|error| panic!("asset: {error:?}")),
        ModuleRegistration::new(
            ModuleId::Budget,
            &[
                activity(ModuleId::Budget, 1),
                activity(ModuleId::Budget, 2),
                activity(ModuleId::Budget, 4),
                activity(ModuleId::Budget, 7),
            ],
        )
        .unwrap_or_else(|error| panic!("budget: {error:?}")),
        ModuleRegistration::new(
            ModuleId::Bridge,
            &[activity(ModuleId::Bridge, 1), activity(ModuleId::Bridge, 2)],
        )
        .unwrap_or_else(|error| panic!("bridge: {error:?}")),
    ];
    ModuleRegistry::new(&registrations).unwrap_or_else(|error| panic!("registry: {error:?}"))
}

fn did() -> Did {
    Did::new(b"did:layerx:golden").unwrap_or_else(|error| panic!("did construction: {error:?}"))
}

fn account(value: &str) -> AccountId {
    AccountId::parse(value).unwrap_or_else(|error| panic!("account {value}: {error:?}"))
}

fn owner() -> AccountId {
    account("agent:did:layerx:golden:main")
}

fn recipient() -> AccountId {
    account("agent:did:layerx:recipient:main")
}

fn budget() -> AccountId {
    account("agent:did:layerx:golden:budget:primary")
}

fn send(amount: u128, idempotency: [u8; 32]) -> Intent {
    Intent::v1(IntentKind::LxpSend(
        LxpSend::new(
            owner(),
            recipient(),
            AssetId::new([2; 32]),
            Amount::from_u128(amount),
            Sequence::from_u64(7),
            IdempotencyKey::new(idempotency),
            TimestampSeconds::from_u64(1_010),
            ContextHash::new([5; 32]),
            SendAuthorization::new(
                SendAuthorizationKind::Owner,
                PublicKey::new([6; 32]),
                AuthorizationSignature::new([7; 64]),
            ),
            NetworkId::new(77).unwrap_or_else(|error| panic!("network: {error:?}")),
            ProtocolVersion::new(1).unwrap_or_else(|error| panic!("protocol: {error:?}")),
        )
        .unwrap_or_else(|error| panic!("send: {error:?}")),
    ))
}

#[allow(clippy::too_many_lines)]
fn fixtures() -> Vec<Fixture> {
    let asset = || AssetId::new([2; 32]);
    let idempotency = || IdempotencyKey::new([3; 32]);
    let amount = || Amount::from_u128(25);
    let issued_session = issue_session_key(&SessionKeyRequest {
        grantor: [1; 32],
        session_public_key: [2; 32],
        not_before: 10,
        expires_at: Some(1_000),
        permitted_activity_types: (1..=6)
            .map(|ordinal| activity(ModuleId::Governance, ordinal))
            .collect(),
        revocation_sequence: Some(3),
    })
    .unwrap_or_else(|error| panic!("session grant: {error:?}"));
    vec![
        Fixture {
            name: "did_registration",
            source: "v1 did_registration did=did:layerx:golden primary_key=01x32",
            intent: Intent::v1(IntentKind::DidRegistration(
                DidRegistration::new(did(), PublicKey::new([1; 32]))
                    .unwrap_or_else(|error| panic!("registration: {error:?}")),
            )),
        },
        Fixture {
            name: "key_rotation",
            source: "v1 key_rotation did=did:layerx:golden pending_key=02x32 not_before=10 not_after=20 effective_sequence=3",
            intent: Intent::v1(IntentKind::KeyRotation(
                KeyRotation::new(
                    did(),
                    PublicKey::new([2; 32]),
                    TimestampBound::new(10, 20).unwrap_or_else(|error| panic!("window: {error:?}")),
                    Sequence::from_u64(3),
                )
                .unwrap_or_else(|error| panic!("rotation: {error:?}")),
            )),
        },
        Fixture {
            name: "recovery_registration",
            source: "v1 recovery_registration did=did:layerx:golden recovery_root=03x32 threshold=2",
            intent: Intent::v1(IntentKind::RecoveryRegistration(
                RecoveryRegistration::new(
                    did(),
                    RecoveryRoot::new([3; 32]),
                    ApprovalThreshold::new(2)
                        .unwrap_or_else(|error| panic!("threshold: {error:?}")),
                )
                .unwrap_or_else(|error| panic!("recovery: {error:?}")),
            )),
        },
        Fixture {
            name: "evm_payout_binding",
            source: "v1 evm_payout_binding did=did:layerx:golden network=77 payout=04x20 ownership_signature=05x65",
            intent: Intent::v1(IntentKind::EvmPayoutBinding(
                EvmPayoutBinding::new(
                    did(),
                    NetworkId::new(77).unwrap_or_else(|error| panic!("network: {error:?}")),
                    EvmAddress::new([4; 20]),
                    Signature::new(&[5; 65]).unwrap_or_else(|error| panic!("signature: {error:?}")),
                )
                .unwrap_or_else(|error| panic!("payout binding: {error:?}")),
            )),
        },
        Fixture {
            name: "session_grant",
            source: "v1 session_grant canonical_authority_grant=0x2001 grantor=01x32 session_public_key=02x32 module_mask=128 ordinal_min=1 ordinal_max=6 not_before=10 expires=1000 revocation_sequence=3",
            intent: Intent::v1(IntentKind::SessionGrant(
                SessionGrant::new(issued_session.registration_payload)
                    .unwrap_or_else(|error| panic!("session intent: {error:?}")),
            )),
        },
        Fixture {
            name: "session_revoke",
            source: "v1 session_revoke grant_id=03x32 reason=paused effective_sequence=9",
            intent: Intent::v1(IntentKind::SessionRevoke(
                SessionRevoke::new(
                    AuthorityGrantId::new([3; 32]),
                    SessionRevocationReason::Paused,
                    Sequence::from_u64(9),
                )
                .unwrap_or_else(|error| panic!("session revoke: {error:?}")),
            )),
        },
        Fixture {
            name: "lxp_send",
            source: "v1 lxp_send from=agent:did:layerx:golden:main to=agent:did:layerx:recipient:main asset=02x32 amount=25 sequence=7 idempotency=03x32 expires=1010 context=05x32 authorization=owner public_key=06x32 signature=07x64 network=77 protocol=1",
            intent: send(25, [3; 32]),
        },
        Fixture {
            name: "lxp_receive",
            source: "v1 lxp_receive from=agent:did:layerx:golden:main to=agent:did:layerx:recipient:main asset=02x32 amount=25 grant=06x32 sequence=4 idempotency=03x32 context=07x32",
            intent: Intent::v1(IntentKind::LxpReceive(
                LxpReceive::new(
                    owner(),
                    recipient(),
                    asset(),
                    amount(),
                    PayerGrantId::new([6; 32]),
                    Sequence::from_u64(4),
                    idempotency(),
                    ContextHash::new([7; 32]),
                )
                .unwrap_or_else(|error| panic!("receive: {error:?}")),
            )),
        },
        Fixture {
            name: "payer_grant_registration",
            source: "v1 payer_grant_registration grant=08x32 from=agent:did:layerx:golden:main recipient=agent:did:layerx:recipient:main asset=02x32 per_draw=25 allowance=100 schedule=recurring:60 expiration=2000 purpose=09x32 public_key=0ax32",
            intent: Intent::v1(IntentKind::PayerGrantRegistration(
                PayerGrantRegistration::new(
                    PayerGrantId::new([8; 32]),
                    owner(),
                    recipient(),
                    asset(),
                    amount(),
                    Amount::from_u128(100),
                    GrantSchedule::Recurring(
                        PeriodLength::new(60).unwrap_or_else(|error| panic!("period: {error:?}")),
                    ),
                    TimestampSeconds::from_u64(2_000),
                    PurposeHash::new([9; 32]),
                    PublicKey::new([10; 32]),
                )
                .unwrap_or_else(|error| panic!("payer grant: {error:?}")),
            )),
        },
        Fixture {
            name: "budget_create",
            source: "v1 budget_create budget=0bx32 owner=agent:did:layerx:golden:main account=agent:did:layerx:golden:budget:primary asset=02x32 limit=100 period=86400 rollover=capped carry=50 purpose=0cx32 expiry=3000",
            intent: Intent::v1(IntentKind::BudgetCreate(
                BudgetCreate::new(
                    BudgetId::new([11; 32]),
                    owner(),
                    budget(),
                    asset(),
                    Amount::from_u128(100),
                    PeriodLength::new(86_400).unwrap_or_else(|error| panic!("period: {error:?}")),
                    RolloverPolicy::Capped,
                    Amount::from_u128(50),
                    PurposeHash::new([12; 32]),
                    TimestampSeconds::from_u64(3_000),
                )
                .unwrap_or_else(|error| panic!("budget create: {error:?}")),
            )),
        },
        Fixture {
            name: "budget_fund",
            source: "v1 budget_fund budget=0bx32 owner=agent:did:layerx:golden:main account=agent:did:layerx:golden:budget:primary asset=02x32 amount=25 idempotency=03x32",
            intent: Intent::v1(IntentKind::BudgetFund(
                BudgetFund::new(
                    BudgetId::new([11; 32]),
                    owner(),
                    budget(),
                    asset(),
                    amount(),
                    idempotency(),
                )
                .unwrap_or_else(|error| panic!("budget fund: {error:?}")),
            )),
        },
        Fixture {
            name: "budget_defund",
            source: "v1 budget_defund budget=0bx32 account=agent:did:layerx:golden:budget:primary owner=agent:did:layerx:golden:main asset=02x32 amount=25 revocation_sequence=8 idempotency=03x32",
            intent: Intent::v1(IntentKind::BudgetDefund(
                BudgetDefund::new(
                    BudgetId::new([11; 32]),
                    budget(),
                    owner(),
                    asset(),
                    amount(),
                    Sequence::from_u64(8),
                    idempotency(),
                )
                .unwrap_or_else(|error| panic!("budget defund: {error:?}")),
            )),
        },
        Fixture {
            name: "bridge_deposit_credit",
            source: "v1 bridge_deposit_credit proof=0dx32 checkpoint=0ex32 reserve=system:paxeer-reserve recipient=agent:did:layerx:golden:main asset=02x32 amount=25 idempotency=03x32",
            intent: Intent::v1(IntentKind::BridgeDepositCredit(
                BridgeDepositCredit::new(
                    DepositProofId::new([13; 32]),
                    CheckpointId::new([14; 32]),
                    account("system:paxeer-reserve"),
                    owner(),
                    asset(),
                    amount(),
                    idempotency(),
                )
                .unwrap_or_else(|error| panic!("bridge deposit: {error:?}")),
            )),
        },
        Fixture {
            name: "bridge_withdraw_request",
            source: "v1 bridge_withdraw_request withdrawal=0fx32 owner=agent:did:layerx:golden:main account=system:paxeer-withdrawals payout=10x20 asset=02x32 amount=25 idempotency=03x32",
            intent: Intent::v1(IntentKind::BridgeWithdrawRequest(
                BridgeWithdrawRequest::new(
                    WithdrawalId::new([15; 32]),
                    owner(),
                    account("system:paxeer-withdrawals"),
                    EvmAddress::new([16; 20]),
                    asset(),
                    amount(),
                    idempotency(),
                )
                .unwrap_or_else(|error| panic!("bridge withdrawal: {error:?}")),
            )),
        },
    ]
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(DIGITS[usize::from(byte >> 4)]));
        encoded.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn render_vectors() -> String {
    let registry = registry();
    let fixtures = fixtures();
    let mut rendered = format!(
        "# Immutable golden vectors for IntentVersion::V1.\nintent_version = {}\nvector_count = {}\n",
        IntentVersion::CURRENT.value(),
        fixtures.len()
    );
    for fixture in fixtures {
        let compiled = compile(&fixture.intent, &registry)
            .unwrap_or_else(|error| panic!("compile {}: {error:?}", fixture.name));
        write!(
            rendered,
            "\n[vector.{}]\nintent = \"{}\"\nactivity_type = {}\npayload_hex = \"{}\"\npayload_hash_hex = \"{}\"\n",
            fixture.name,
            fixture.source,
            compiled.activity_type().value(),
            hex(compiled.payload().as_bytes()),
            hex(&compiled.payload_hash()),
        )
        .unwrap_or_else(|error| panic!("render {}: {error}", fixture.name));
    }
    rendered
}

#[test]
fn v1_golden_vectors_are_byte_locked_to_the_intent_version() {
    assert_eq!(golden::V1_VERSION, IntentVersion::CURRENT.value());
    let rendered = render_vectors();
    if std::env::var_os("LAYERX_PRINT_INTENT_VECTORS").is_some() {
        println!("{rendered}");
    } else {
        assert_eq!(rendered, golden::V1_SOURCE);
    }
}

#[test]
fn every_vector_decodes_matches_and_reencodes_byte_identically() {
    let registry = registry();
    for fixture in fixtures() {
        let compiled = compile(&fixture.intent, &registry)
            .unwrap_or_else(|error| panic!("compile {}: {error:?}", fixture.name));
        let check = DisclosureCheck::verify(&fixture.intent, &compiled)
            .unwrap_or_else(|error| panic!("disclosure {}: {error:?}", fixture.name));
        assert_eq!(check.activity_type(), compiled.activity_type());
        assert_eq!(check.canonical_payload(), compiled.payload().as_bytes());
        assert_eq!(check.payload_hash(), compiled.payload_hash());
    }
}

#[test]
fn mismatch_aborts_on_the_exact_originating_intent_field() {
    let registry = registry();
    let originating = send(25, [3; 32]);
    let compiled = compile(&originating, &registry)
        .unwrap_or_else(|error| panic!("originating compile: {error:?}"));
    let altered = send(26, [3; 32]);
    assert_eq!(
        DisclosureCheck::verify(&altered, &compiled),
        Err(DisclosureCheckError::FieldMismatch(DisclosureField::Amount))
    );
}

proptest! {
    #[test]
    fn generated_sends_round_trip_without_semantic_drift(
        amount in 1_u128..=u128::MAX,
        idempotency in any::<[u8; 32]>(),
    ) {
        let intent = send(amount, idempotency);
        let compiled = compile(&intent, &registry())
            .map_err(|error| TestCaseError::fail(format!("compile: {error:?}")))?;
        let checked = DisclosureCheck::verify(&intent, &compiled)
            .map_err(|error| TestCaseError::fail(format!("disclosure: {error:?}")))?;
        prop_assert_eq!(checked.canonical_payload(), compiled.payload().as_bytes());
        prop_assert_eq!(checked.payload_hash(), compiled.payload_hash());
    }
}
use std::fmt::Write as _;
