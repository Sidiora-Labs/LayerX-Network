use layerx_agentd::prepare::{
    prepare_activity, verify_disclosure_binding, CorePreparationBoundary, CorePreparationState,
    CoreStateError, PreparationDefaults, PrepareRequest,
};
use layerx_intents::{
    compile, BridgeDepositCredit, BridgeWithdrawRequest, BudgetCreate, BudgetDefund, BudgetFund,
    CompileErrorReason, CompileField, DidRegistration, DisclosureCheck, DisclosureCheckError,
    DisclosureField, EvmPayoutBinding, Intent, IntentKind, KeyRotation, LxpReceive, LxpSend,
    PayerGrantRegistration, RecoveryRegistration,
};
use layerx_types::account::AccountId;
use layerx_types::activity::{Authority, Signature, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{AssetId, CheckpointId, Did, IdempotencyKey};
use layerx_types::intent::{
    ApprovalThreshold, AuthorizationSignature, BudgetId, ContextHash, DepositProofId, EvmAddress,
    GrantSchedule, NetworkId, PayerGrantId, PeriodLength, ProtocolVersion, PublicKey, PurposeHash,
    RecoveryRoot, RolloverPolicy, SendAuthorization, SendAuthorizationKind, Sequence,
    TimestampSeconds, WithdrawalId,
};
use layerx_types::payload::{
    ActivityType, ModuleId, ModuleRegistration, ModuleRegistry, PayloadError,
};
use layerx_wire::hash;
use proptest::prelude::*;

struct RecordedCoreBoundary {
    state: CorePreparationState,
}

impl CorePreparationBoundary for RecordedCoreBoundary {
    fn preparation_state(&mut self, _actor: &Did) -> Result<CorePreparationState, CoreStateError> {
        Ok(self.state.clone())
    }
}

fn activity(module: ModuleId, ordinal: u16) -> ActivityType {
    ActivityType::new(module, ordinal)
        .unwrap_or_else(|error| panic!("activity {module:?}/{ordinal}: {error:?}"))
}

fn registry() -> ModuleRegistry {
    let governance = [
        activity(ModuleId::Governance, 1),
        activity(ModuleId::Governance, 2),
        activity(ModuleId::Governance, 3),
        activity(ModuleId::Governance, 4),
    ];
    let asset = [activity(ModuleId::Asset, 5), activity(ModuleId::Asset, 6)];
    let budget = [
        activity(ModuleId::Budget, 1),
        activity(ModuleId::Budget, 2),
        activity(ModuleId::Budget, 4),
        activity(ModuleId::Budget, 7),
    ];
    let bridge = [activity(ModuleId::Bridge, 1), activity(ModuleId::Bridge, 2)];
    let registrations = [
        ModuleRegistration::new(ModuleId::Governance, &governance)
            .unwrap_or_else(|error| panic!("governance registry: {error:?}")),
        ModuleRegistration::new(ModuleId::Asset, &asset)
            .unwrap_or_else(|error| panic!("asset registry: {error:?}")),
        ModuleRegistration::new(ModuleId::Budget, &budget)
            .unwrap_or_else(|error| panic!("budget registry: {error:?}")),
        ModuleRegistration::new(ModuleId::Bridge, &bridge)
            .unwrap_or_else(|error| panic!("bridge registry: {error:?}")),
    ];
    ModuleRegistry::new(&registrations).unwrap_or_else(|error| panic!("module registry: {error:?}"))
}

fn did() -> Did {
    Did::new(b"did:layerx:human-compiler")
        .unwrap_or_else(|error| panic!("did construction: {error:?}"))
}

fn account(value: &str) -> AccountId {
    AccountId::parse(value).unwrap_or_else(|error| panic!("account {value}: {error:?}"))
}

fn owner() -> AccountId {
    account("agent:did:layerx:human-compiler:main")
}

fn recipient() -> AccountId {
    account("agent:did:layerx:recipient:main")
}

fn send_intent(entropy: [u8; 32]) -> Intent {
    let mut public_key = entropy;
    public_key[0] |= 1;
    let mut signature = [0_u8; 64];
    signature[..32].copy_from_slice(&entropy);
    signature[32..].copy_from_slice(&entropy);
    let send = LxpSend::new(
        owner(),
        recipient(),
        AssetId::new([2; 32]),
        Amount::from_u128(u128::from(entropy[0]) + 1),
        Sequence::from_u64(7),
        IdempotencyKey::new(entropy),
        TimestampSeconds::from_u64(1_010),
        ContextHash::new([5; 32]),
        SendAuthorization::new(
            SendAuthorizationKind::Owner,
            PublicKey::new(public_key),
            AuthorizationSignature::new(signature),
        ),
        NetworkId::new(77).unwrap_or_else(|error| panic!("network: {error:?}")),
        ProtocolVersion::new(layerx_wire::limits::PROTOCOL_VERSION)
            .unwrap_or_else(|error| panic!("protocol: {error:?}")),
    )
    .unwrap_or_else(|error| panic!("send intent: {error:?}"));
    Intent::v1(IntentKind::LxpSend(send))
}

proptest! {
    #[test]
    fn equal_intents_always_compile_to_identical_bytes(seed in any::<[u8; 32]>()) {
        let intent = send_intent(seed);
        let equal_intent = intent.clone();
        let first = compile(&intent, &registry())
            .map_err(|error| TestCaseError::fail(format!("first compile: {error:?}")))?;
        let second = compile(&equal_intent, &registry())
            .map_err(|error| TestCaseError::fail(format!("second compile: {error:?}")))?;
        prop_assert_eq!(first.activity_type(), second.activity_type());
        prop_assert_eq!(first.payload().as_bytes(), second.payload().as_bytes());
        prop_assert_eq!(first.payload_hash(), second.payload_hash());
    }
}

#[test]
fn compiled_send_is_accepted_by_prepare_and_disclosed_field_for_field() {
    let intent = send_intent([4; 32]);
    let registry = registry();
    let compiled = compile(&intent, &registry).unwrap_or_else(|error| panic!("compile: {error:?}"));
    let mut boundary = RecordedCoreBoundary {
        state: CorePreparationState {
            network_id: 77,
            account_sequence: 7,
            protocol_timestamp: 1_000,
            observed_head_sequence: 91,
            module_registry: registry.clone(),
        },
    };
    let prepared = prepare_activity(
        &mut boundary,
        PreparationDefaults {
            timestamp_span: 20,
            fee_limit: Amount::from_u128(9),
            maximum_payload_bytes: 1_024,
        },
        PrepareRequest {
            actor: did(),
            authority: Authority::owner(b"human-owner")
                .unwrap_or_else(|error| panic!("authority: {error:?}")),
            activity_type: compiled.activity_type(),
            expected_account_sequence: Some(7),
            timestamp_bound: Some(
                TimestampBound::new(995, 1_010)
                    .unwrap_or_else(|error| panic!("timestamp: {error:?}")),
            ),
            fee_limit: None,
            idempotency_key: IdempotencyKey::new([4; 32]),
            payload: compiled.payload().as_bytes().to_vec(),
            declared_payload_limit: 1_024,
        },
    )
    .unwrap_or_else(|error| panic!("prepare: {error:?}"));

    let from = hash::account_id(&owner()).unwrap_or_else(|error| panic!("from hash: {error:?}"));
    let to = hash::account_id(&recipient()).unwrap_or_else(|error| panic!("to hash: {error:?}"));
    assert_eq!(prepared.envelope.payload_hash(), compiled.payload_hash());
    assert_eq!(prepared.disclosure.counterparties[0].account, from);
    assert_eq!(prepared.disclosure.counterparties[1].account, to);
    assert_eq!(prepared.disclosure.amounts[0].value, 5);
    assert_eq!(prepared.disclosure.asset, [2; 32]);
    assert_eq!(prepared.disclosure.idempotency_key, [4; 32]);
    assert_eq!(prepared.disclosure.expiry.payload_expires_at, 1_010);
    DisclosureCheck::verify(&intent, &compiled)
        .unwrap_or_else(|error| panic!("intent disclosure: {error:?}"));
    DisclosureCheck::verify_agent(&intent, &prepared.disclosure)
        .unwrap_or_else(|error| panic!("agent disclosure: {error:?}"));
    verify_disclosure_binding(&prepared)
        .unwrap_or_else(|error| panic!("disclosure binding: {error:?}"));

    let mut altered_disclosure = prepared.disclosure.clone();
    altered_disclosure.amounts[0].value += 1;
    assert_eq!(
        DisclosureCheck::verify_agent(&intent, &altered_disclosure),
        Err(DisclosureCheckError::FieldMismatch(DisclosureField::Amount))
    );
}

#[test]
fn undeclared_activity_is_a_typed_payload_field_error() {
    let empty =
        ModuleRegistry::new(&[]).unwrap_or_else(|error| panic!("empty module registry: {error:?}"));
    let error = compile(&send_intent([8; 32]), &empty)
        .err()
        .unwrap_or_else(|| panic!("undeclared activity compiled"));
    assert_eq!(error.field, CompileField::Payload);
    assert_eq!(
        error.reason,
        CompileErrorReason::Payload(PayloadError::UndeclaredActivity(
            activity(ModuleId::Asset, 5).value()
        ))
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn every_v1_intent_compiles_through_the_registered_module() {
    let asset = || AssetId::new([2; 32]);
    let idempotency = || IdempotencyKey::new([3; 32]);
    let amount = || Amount::from_u128(25);
    let budget_account = || account("agent:did:layerx:human-compiler:budget:primary");
    let intents = vec![
        Intent::v1(IntentKind::DidRegistration(
            DidRegistration::new(did(), PublicKey::new([1; 32]))
                .unwrap_or_else(|error| panic!("registration: {error:?}")),
        )),
        Intent::v1(IntentKind::KeyRotation(
            KeyRotation::new(
                did(),
                PublicKey::new([2; 32]),
                TimestampBound::new(10, 20).unwrap_or_else(|error| panic!("window: {error:?}")),
                Sequence::from_u64(3),
            )
            .unwrap_or_else(|error| panic!("rotation: {error:?}")),
        )),
        Intent::v1(IntentKind::RecoveryRegistration(
            RecoveryRegistration::new(
                did(),
                RecoveryRoot::new([3; 32]),
                ApprovalThreshold::new(2).unwrap_or_else(|error| panic!("threshold: {error:?}")),
            )
            .unwrap_or_else(|error| panic!("recovery: {error:?}")),
        )),
        Intent::v1(IntentKind::EvmPayoutBinding(
            EvmPayoutBinding::new(
                did(),
                NetworkId::new(77).unwrap_or_else(|error| panic!("network: {error:?}")),
                EvmAddress::new([4; 20]),
                Signature::new(&[5; 65]).unwrap_or_else(|error| panic!("signature: {error:?}")),
            )
            .unwrap_or_else(|error| panic!("payout binding: {error:?}")),
        )),
        send_intent([4; 32]),
        Intent::v1(IntentKind::LxpReceive(
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
        Intent::v1(IntentKind::PayerGrantRegistration(
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
        Intent::v1(IntentKind::BudgetCreate(
            BudgetCreate::new(
                BudgetId::new([11; 32]),
                owner(),
                budget_account(),
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
        Intent::v1(IntentKind::BudgetFund(
            BudgetFund::new(
                BudgetId::new([11; 32]),
                owner(),
                budget_account(),
                asset(),
                amount(),
                idempotency(),
            )
            .unwrap_or_else(|error| panic!("budget fund: {error:?}")),
        )),
        Intent::v1(IntentKind::BudgetDefund(
            BudgetDefund::new(
                BudgetId::new([11; 32]),
                budget_account(),
                owner(),
                asset(),
                amount(),
                Sequence::from_u64(8),
                idempotency(),
            )
            .unwrap_or_else(|error| panic!("budget defund: {error:?}")),
        )),
        Intent::v1(IntentKind::BridgeDepositCredit(
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
        Intent::v1(IntentKind::BridgeWithdrawRequest(
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
    ];
    let registry = registry();
    let compiled = intents
        .iter()
        .map(|intent| {
            compile(intent, &registry).unwrap_or_else(|error| panic!("compile: {error:?}"))
        })
        .collect::<Vec<_>>();
    assert_eq!(compiled.len(), 12);
    assert!(compiled
        .iter()
        .all(|value| !value.payload().as_bytes().is_empty()));
    assert!(compiled
        .iter()
        .zip(intents.iter())
        .all(|(compiled, intent)| compiled.activity_type().module() == intent.module()));
}
