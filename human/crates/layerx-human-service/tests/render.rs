#[allow(dead_code)]
mod support;

use std::fmt::Write as _;

use ed25519_dalek::SigningKey;
use layerx_agent_api::identity::{
    ActivityType as ApiActivityType, AgentDid, Asset, AuthorityRef, ExplicitSet,
};
use layerx_agent_api::prepare::{
    CanonicalBytes, DisclosedAmount as ApiDisclosedAmount, Disclosure as ApiDisclosure,
    IdempotencyRef, PreparationRef, Prepared as ApiPrepared, SigningPreimage,
};
use layerx_agent_api::verify::Level;
use layerx_agent_api::{Amount as ApiAmount, TimestampSeconds as ApiTimestamp};
use layerx_agentd::budget::{reconcile, LocalAccounting, ProtocolBudgetState, ReconciliationState};
use layerx_agentd::capability::CapabilityId;
use layerx_agentd::policy::approval::{hold, ApprovalContext, ApprovalRegistry};
use layerx_agentd::prepare::{
    prepare_activity, CorePreparationBoundary, CorePreparationState, CoreStateError,
    PreparationDefaults, PrepareRequest, Prepared,
};
use layerx_agentd::session::SessionId;
use layerx_agentd::store::TenantId;
use layerx_human_service::approvals::{
    ApprovalActivityClass, ApprovalPresentation, DisclosureRenderError, DisclosureRenderer,
    RenderedApproval, RenderedCounterparty, VerifiedBudgetAfter,
};
use layerx_intents::{compile, BridgeDepositCredit, EvmPayoutBinding, Intent, IntentKind, LxpSend};
use layerx_types::account::AccountId;
use layerx_types::activity::{Authority, Signature, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{AssetId, CheckpointId, Did, IdempotencyKey};
use layerx_types::intent::{
    AuthorizationSignature, ContextHash, DepositProofId, EvmAddress, NetworkId, ProtocolVersion,
    PublicKey, SendAuthorization, SendAuthorizationKind, Sequence, TimestampSeconds,
};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use proptest::prelude::*;
use sha2::{Digest as _, Sha256};

const NETWORK_ID: u32 = 77;
const ACCOUNT_SEQUENCE: u64 = 7;
const ENVELOPE_EXPIRY: u64 = 1_000;
const CATALOG: &str = include_str!("../../../apps/web/copy/catalog.ts");

struct RecordedCore(CorePreparationState);

impl CorePreparationBoundary for RecordedCore {
    fn preparation_state(&mut self, _actor: &Did) -> Result<CorePreparationState, CoreStateError> {
        Ok(self.0.clone())
    }
}

fn activity(module: ModuleId, ordinal: u16) -> ActivityType {
    ActivityType::new(module, ordinal)
        .unwrap_or_else(|error| panic!("activity {module:?}/{ordinal}: {error:?}"))
}

fn registry() -> ModuleRegistry {
    let governance =
        ModuleRegistration::new(ModuleId::Governance, &[activity(ModuleId::Governance, 4)])
            .unwrap_or_else(|error| panic!("governance registry: {error:?}"));
    let asset = ModuleRegistration::new(ModuleId::Asset, &[activity(ModuleId::Asset, 5)])
        .unwrap_or_else(|error| panic!("asset registry: {error:?}"));
    let bridge = ModuleRegistration::new(ModuleId::Bridge, &[activity(ModuleId::Bridge, 1)])
        .unwrap_or_else(|error| panic!("bridge registry: {error:?}"));
    ModuleRegistry::new(&[governance, asset, bridge])
        .unwrap_or_else(|error| panic!("module registry: {error:?}"))
}

fn actor() -> Did {
    Did::new(b"did:layerx:approval-render").unwrap_or_else(|error| panic!("actor DID: {error:?}"))
}

fn account(value: &str) -> AccountId {
    AccountId::parse(value).unwrap_or_else(|error| panic!("account {value}: {error:?}"))
}

fn owner() -> AccountId {
    account("agent:did:layerx:approval-render:main")
}

fn recipient(marker: u64) -> AccountId {
    account(&format!("agent:did:layerx:recipient-{marker}:main"))
}

fn nonzero_key(mut key: [u8; 32]) -> [u8; 32] {
    key[0] |= 1;
    key
}

fn send_intent(recipient_marker: u64, amount: u128, asset: [u8; 32], key: [u8; 32]) -> Intent {
    let key = nonzero_key(key);
    let send = LxpSend::new(
        owner(),
        recipient(recipient_marker),
        AssetId::new(asset),
        Amount::from_u128(amount),
        Sequence::from_u64(ACCOUNT_SEQUENCE),
        IdempotencyKey::new(key),
        TimestampSeconds::from_u64(900),
        ContextHash::new([0x55; 32]),
        SendAuthorization::new(
            SendAuthorizationKind::Owner,
            PublicKey::new([0x31; 32]),
            AuthorizationSignature::new([0x41; 64]),
        ),
        NetworkId::new(NETWORK_ID).unwrap_or_else(|error| panic!("network: {error:?}")),
        ProtocolVersion::new(1).unwrap_or_else(|error| panic!("protocol: {error:?}")),
    )
    .unwrap_or_else(|error| panic!("send intent: {error:?}"));
    Intent::v1(IntentKind::LxpSend(send))
}

fn deposit_intent(recipient_marker: u64, amount: u128, asset: [u8; 32], key: [u8; 32]) -> Intent {
    let key = nonzero_key(key);
    let deposit = BridgeDepositCredit::new(
        DepositProofId::new([0x61; 32]),
        CheckpointId::new([0x62; 32]),
        account("system:paxeer-reserve"),
        recipient(recipient_marker),
        AssetId::new(asset),
        Amount::from_u128(amount),
        IdempotencyKey::new(key),
    )
    .unwrap_or_else(|error| panic!("deposit intent: {error:?}"));
    Intent::v1(IntentKind::BridgeDepositCredit(deposit))
}

fn wallet_intent(address: [u8; 20]) -> Intent {
    let binding = EvmPayoutBinding::new(
        actor(),
        NetworkId::new(NETWORK_ID).unwrap_or_else(|error| panic!("network: {error:?}")),
        EvmAddress::new(address),
        Signature::new(&[0x71; 65]).unwrap_or_else(|error| panic!("signature: {error:?}")),
    )
    .unwrap_or_else(|error| panic!("binding intent: {error:?}"));
    Intent::v1(IntentKind::EvmPayoutBinding(binding))
}

fn prepare(intent: &Intent, key: [u8; 32], fee_limit: u128) -> Prepared {
    let key = nonzero_key(key);
    let registry = registry();
    let compiled =
        compile(intent, &registry).unwrap_or_else(|error| panic!("compile intent: {error:?}"));
    let mut core = RecordedCore(CorePreparationState {
        network_id: NETWORK_ID,
        account_sequence: ACCOUNT_SEQUENCE,
        protocol_timestamp: 100,
        observed_head_sequence: 91,
        module_registry: registry,
    });
    prepare_activity(
        &mut core,
        PreparationDefaults {
            timestamp_span: ENVELOPE_EXPIRY - 90,
            fee_limit: Amount::from_u128(fee_limit),
            maximum_payload_bytes: 1_024,
        },
        PrepareRequest {
            actor: actor(),
            authority: Authority::owner(b"approval-render-owner")
                .unwrap_or_else(|error| panic!("authority: {error:?}")),
            activity_type: compiled.activity_type(),
            expected_account_sequence: Some(ACCOUNT_SEQUENCE),
            timestamp_bound: Some(
                TimestampBound::new(90, ENVELOPE_EXPIRY)
                    .unwrap_or_else(|error| panic!("timestamp bound: {error:?}")),
            ),
            fee_limit: Some(Amount::from_u128(fee_limit)),
            idempotency_key: IdempotencyKey::new(key),
            payload: compiled.payload().as_bytes().to_vec(),
            declared_payload_limit: 1_024,
        },
    )
    .unwrap_or_else(|error| panic!("real agent preparation: {error:?}"))
}

fn held_digest(prepared: &Prepared, marker: u8) -> [u8; 32] {
    let digest: [u8; 32] = Sha256::digest(&prepared.canonical_bytes).into();
    let counterparties = prepared
        .disclosure
        .counterparties
        .iter()
        .map(|counterparty| {
            AgentDid::new(hex(&counterparty.account))
                .unwrap_or_else(|error| panic!("counterparty: {error:?}"))
        })
        .collect::<Vec<_>>();
    let amounts = prepared
        .disclosure
        .amounts
        .iter()
        .zip(counterparties.iter().cycle())
        .map(|(amount, counterparty)| ApiDisclosedAmount {
            counterparty: counterparty.clone(),
            amount: ApiAmount(amount.value),
        })
        .collect::<Vec<_>>();
    let api_prepared = ApiPrepared {
        preparation_ref: PreparationRef::new(format!("approval-render-{marker}"))
            .unwrap_or_else(|error| panic!("preparation ref: {error:?}")),
        unsigned_canonical_bytes: CanonicalBytes::new(prepared.canonical_bytes.clone())
            .unwrap_or_else(|error| panic!("canonical bytes: {error:?}")),
        signing_preimage: SigningPreimage::new(prepared.signing_preimage.to_vec())
            .unwrap_or_else(|error| panic!("signing preimage: {error:?}")),
        disclosure: ApiDisclosure {
            canonical_digest: digest,
            activity_type: ApiActivityType(prepared.disclosure.activity_type.ordinal()),
            actor: AgentDid::new("did:layerx:approval-render")
                .unwrap_or_else(|error| panic!("API actor: {error:?}")),
            authority: AuthorityRef::new("approval-render-owner")
                .unwrap_or_else(|error| panic!("authority ref: {error:?}")),
            counterparties: ExplicitSet::allow(counterparties),
            amounts: ExplicitSet::allow(amounts),
            asset: Asset::new(hex(&prepared.disclosure.asset))
                .unwrap_or_else(|error| panic!("asset: {error:?}")),
            fee_limit: ApiAmount(prepared.disclosure.fee_limit),
            expiry: ApiTimestamp(ENVELOPE_EXPIRY),
            idempotency_key: IdempotencyRef::new(format!("approval-render-key-{marker}"))
                .unwrap_or_else(|error| panic!("idempotency ref: {error:?}")),
        },
        expiry: ApiTimestamp(ENVELOPE_EXPIRY),
    };
    let registry = ApprovalRegistry::default();
    let context = ApprovalContext {
        tenant: TenantId::new("tenant-approval-render")
            .unwrap_or_else(|error| panic!("tenant: {error}")),
        agent: actor(),
        session: SessionId([0x81; 32]),
        capability: CapabilityId([0x82; 32]),
        policy_version: "approval-render-v1".to_owned(),
        request_id: [marker; 32],
    };
    hold(&registry, context, api_prepared, 100, 900)
        .unwrap_or_else(|error| panic!("real agent approval hold: {error:?}"))
        .disclosure_digest
}

fn verified_budget(remaining: u128, observed_at_sequence: u64) -> VerifiedBudgetAfter {
    let mut local = LocalAccounting {
        consumed: 0,
        window_start_sequence: 1,
        last_receipt: None,
    };
    let state = reconcile(
        &mut local,
        ProtocolBudgetState {
            evidence: support::raw_state_leaf(
                remaining.to_be_bytes().to_vec(),
                observed_at_sequence,
            ),
        },
        &[],
        &support::evidence_verifier(&SigningKey::from_bytes(&[0x84; 32])),
    )
    .unwrap_or_else(|error| panic!("verified budget reconciliation: {error:?}"));
    budget_from_state(state)
}

fn budget_from_state(state: ReconciliationState) -> VerifiedBudgetAfter {
    let mut evidence = Sha256::new();
    evidence.update(b"layerx-agent-budget-read/v1");
    evidence.update(state.remaining().to_be_bytes());
    evidence.update(state.observed_head_sequence().to_be_bytes());
    VerifiedBudgetAfter {
        remaining: state.remaining(),
        level: Level::StateProven,
        evidence_digest: evidence.finalize().into(),
        observed_at_sequence: state.observed_head_sequence(),
    }
}

fn rendered(
    prepared: &Prepared,
    marker: u8,
    budget_after: VerifiedBudgetAfter,
) -> RenderedApproval {
    let digest = held_digest(prepared, marker);
    match DisclosureRenderer::render(&prepared.disclosure, digest, budget_after)
        .unwrap_or_else(|error| panic!("render approval: {error}"))
    {
        ApprovalPresentation::Rendered(rendered) => *rendered,
        ApprovalPresentation::Unrenderable(value) => {
            panic!("known class was unrenderable: {value:?}")
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

#[test]
fn every_real_v1_hold_class_has_plain_catalog_copy() {
    let send = prepare(&send_intent(1, 25, [0x21; 32], [1; 32]), [1; 32], 3);
    let deposit = prepare(&deposit_intent(2, 30, [0x22; 32], [2; 32]), [2; 32], 4);
    let wallet = prepare(&wallet_intent([0x23; 20]), [3; 32], 5);
    let values = [
        rendered(&send, 1, verified_budget(975, 101)),
        rendered(&deposit, 2, verified_budget(970, 102)),
        rendered(&wallet, 3, verified_budget(965, 103)),
    ];

    assert_eq!(ApprovalActivityClass::ALL.len(), values.len());
    for (class, value) in ApprovalActivityClass::ALL.into_iter().zip(values) {
        assert_eq!(value.class, class);
        assert_eq!(value.copy_key, class.copy_key());
        assert!(CATALOG.contains(&format!(
            "key: \"{}\", message: \"{}\"",
            class.copy_key(),
            class.copy_template()
        )));
        assert!(!value.plain_copy.is_empty());
    }
    assert_eq!(
        values_for_wallet(&wallet),
        RenderedCounterparty::EvmAddress([0x23; 20])
    );
}

fn values_for_wallet(prepared: &Prepared) -> RenderedCounterparty {
    rendered(prepared, 4, verified_budget(1, 104))
        .facts
        .counterparty
}

#[test]
fn unknown_activity_is_explicitly_unrenderable_and_never_approvable() {
    let prepared = prepare(&send_intent(4, 10, [0x24; 32], [4; 32]), [4; 32], 2);
    let mut unknown = prepared.disclosure.clone();
    unknown.activity_type = activity(ModuleId::Asset, 99);
    let presentation = DisclosureRenderer::render(
        &unknown,
        held_digest(&prepared, 5),
        verified_budget(990, 105),
    )
    .unwrap_or_else(|error| panic!("unknown result: {error}"));
    assert!(!presentation.can_approve());
    let ApprovalPresentation::Unrenderable(value) = presentation else {
        panic!("unknown activity became approvable");
    };
    assert_eq!(value.activity_type, activity(ModuleId::Asset, 99).value());
    assert_eq!(value.copy_key, "approval.activity.unrenderable");
    assert_eq!(value.plain_copy, "This request cannot be reviewed here.");
    assert!(CATALOG.contains("key: \"approval.activity.unrenderable\""));
}

#[test]
fn mismatched_digest_and_unverified_budget_are_refused_before_content() {
    let prepared = prepare(&send_intent(5, 10, [0x25; 32], [5; 32]), [5; 32], 2);
    let mut digest = held_digest(&prepared, 6);
    digest[0] ^= 1;
    assert_eq!(
        DisclosureRenderer::render(&prepared.disclosure, digest, verified_budget(990, 106)),
        Err(DisclosureRenderError::DigestMismatch)
    );
    let digest = held_digest(&prepared, 7);
    let mut budget = verified_budget(990, 107);
    budget.level = Level::Unverified;
    assert_eq!(
        DisclosureRenderer::render(&prepared.disclosure, digest, budget),
        Err(DisclosureRenderError::UnverifiedBudget)
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    #[test]
    fn property_send_facts_and_budget_equal_their_verified_sources(
        amount in 1_u128..1_000_000_000_u128,
        recipient_marker in any::<u64>(),
        asset in any::<[u8; 32]>(),
        fee_limit in 1_u128..1_000_000_u128,
        remaining in 0_u128..10_000_000_000_u128,
        key in any::<[u8; 32]>(),
    ) {
        let prepared = prepare(
            &send_intent(recipient_marker, amount, asset, key),
            key,
            fee_limit,
        );
        let budget = verified_budget(remaining, 1_000);
        let value = rendered(&prepared, 8, budget);
        prop_assert_eq!(value.class, ApprovalActivityClass::MoveMoney);
        prop_assert_eq!(value.facts.amount, Some(prepared.disclosure.amounts[0].value));
        prop_assert_eq!(
            value.facts.counterparty,
            RenderedCounterparty::Account(prepared.disclosure.counterparties[1].account)
        );
        prop_assert_eq!(value.facts.asset, Some(prepared.disclosure.asset));
        prop_assert_eq!(value.facts.fee_limit, prepared.disclosure.fee_limit);
        prop_assert_eq!(value.facts.expires_at, prepared.disclosure.expiry.payload_expires_at);
        prop_assert_eq!(value.budget_after, budget);
    }

    #[test]
    fn property_deposit_facts_and_budget_equal_their_verified_sources(
        amount in 1_u128..1_000_000_000_u128,
        recipient_marker in any::<u64>(),
        asset in any::<[u8; 32]>(),
        fee_limit in 1_u128..1_000_000_u128,
        remaining in 0_u128..10_000_000_000_u128,
        key in any::<[u8; 32]>(),
    ) {
        let prepared = prepare(
            &deposit_intent(recipient_marker, amount, asset, key),
            key,
            fee_limit,
        );
        let budget = verified_budget(remaining, 1_001);
        let value = rendered(&prepared, 9, budget);
        prop_assert_eq!(value.class, ApprovalActivityClass::AddMoney);
        prop_assert_eq!(value.facts.amount, Some(prepared.disclosure.amounts[0].value));
        prop_assert_eq!(
            value.facts.counterparty,
            RenderedCounterparty::Account(prepared.disclosure.counterparties[1].account)
        );
        prop_assert_eq!(value.facts.asset, Some(prepared.disclosure.asset));
        prop_assert_eq!(value.facts.fee_limit, prepared.disclosure.fee_limit);
        prop_assert_eq!(value.facts.expires_at, prepared.disclosure.expiry.payload_expires_at);
        prop_assert_eq!(value.budget_after, budget);
    }

    #[test]
    fn property_any_changed_disclosure_fact_fails_reencoding(
        field in 0_u8..5,
        amount in 1_u128..1_000_000_u128,
        key in any::<[u8; 32]>(),
    ) {
        let prepared = prepare(&send_intent(10, amount, [0x31; 32], key), key, 7);
        let digest = held_digest(&prepared, 10);
        let mut changed = prepared.disclosure;
        match field {
            0 => changed.amounts[0].value = changed.amounts[0].value.saturating_add(1),
            1 => changed.counterparties[1].account[0] ^= 1,
            2 => changed.asset[0] ^= 1,
            3 => changed.fee_limit = changed.fee_limit.saturating_add(1),
            _ => changed.expiry.payload_expires_at = changed.expiry.payload_expires_at.saturating_sub(1),
        }
        prop_assert!(matches!(
            DisclosureRenderer::render(&changed, digest, verified_budget(100, 1_002)),
            Err(DisclosureRenderError::DefectiveDisclosure(_))
        ));
    }
}
