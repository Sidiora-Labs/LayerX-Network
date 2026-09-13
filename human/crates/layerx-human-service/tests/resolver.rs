use layerx_human_service::journeys::{
    BudgetCreation, BudgetRoute, ChangeSurface, CustodyRoute, Endpoint, LimitRefusal, LimitSource,
    Mechanism, MovementTerm, PayerGrantRoute, Relationship, RouteError, RouteRequest,
    RouteResolver, SendRoute,
};
use layerx_intents::IntentKind;
use layerx_types::account::AccountId;
use layerx_types::amount::Amount;
use layerx_types::ids::{AssetId, CheckpointId, IdempotencyKey};
use layerx_types::intent::{
    AuthorizationSignature, BudgetId, ContextHash, DepositProofId, EvmAddress, NetworkId,
    PeriodLength, ProtocolVersion, PublicKey, PurposeHash, RolloverPolicy, SendAuthorization,
    SendAuthorizationKind, Sequence, TimestampSeconds,
};
use proptest::prelude::*;

fn account(value: &str) -> AccountId {
    AccountId::parse(value).unwrap_or_else(|error| panic!("account {value}: {error:?}"))
}

fn human() -> AccountId {
    account("agent:did:layerx:human:main")
}

fn agent() -> AccountId {
    account("agent:did:layerx:worker:main")
}

fn budget() -> AccountId {
    account("agent:did:layerx:worker:budget:operations")
}

fn asset() -> AssetId {
    AssetId::new([0x41; 32])
}

fn key(byte: u8) -> IdempotencyKey {
    IdempotencyKey::new([byte; 32])
}

fn send_route(sequence: u64, key_byte: u8) -> SendRoute {
    SendRoute {
        account_sequence: Sequence::from_u64(sequence),
        idempotency_key: key(key_byte),
        expires_at: TimestampSeconds::from_u64(10_000),
        context_hash: ContextHash::new([0x33; 32]),
        authorization: SendAuthorization::new(
            SendAuthorizationKind::Owner,
            PublicKey::new([0x44; 32]),
            AuthorizationSignature::new([0x55; 64]),
        ),
        network_id: NetworkId::new(1)
            .unwrap_or_else(|error| panic!("network identifier: {error:?}")),
        protocol_version: ProtocolVersion::new(layerx_wire::limits::PROTOCOL_VERSION)
            .unwrap_or_else(|error| panic!("protocol version: {error:?}")),
    }
}

fn request(
    source: Endpoint,
    destination: Endpoint,
    relationship: Relationship,
    amount: u128,
) -> RouteRequest {
    RouteRequest {
        source,
        destination,
        relationship,
        asset: asset(),
        amount: Amount::from_u128(amount),
    }
}

#[test]
fn resolver_selects_every_normative_mechanism_and_vocabulary_term() {
    let deposit = request(
        Endpoint::PaxeerWallet,
        Endpoint::Human(human()),
        Relationship::Custody(CustodyRoute::Deposit {
            deposit_proof: DepositProofId::new([1; 32]),
            checkpoint: CheckpointId::new([2; 32]),
            reserve: account("system:paxeer-reserve"),
            idempotency_key: key(3),
        }),
        50,
    );
    let withdrawal = request(
        Endpoint::Human(human()),
        Endpoint::PaxeerWallet,
        Relationship::Custody(CustodyRoute::Withdrawal {
            request_anchor: CheckpointId::new([18; 32]),
            fee_limit: 100,
            withdrawals_account: account("system:paxeer-withdrawals"),
            payout_address: EvmAddress::new([5; 20]),
            idempotency_key: key(6),
        }),
        40,
    );
    let transfer = request(
        Endpoint::Human(human()),
        Endpoint::Agent(agent()),
        Relationship::Direct(send_route(7, 7)),
        30,
    );
    let allocate = request(
        Endpoint::AgentBudget(budget()),
        Endpoint::Agent(agent()),
        Relationship::Direct(send_route(8, 8)),
        20,
    );
    let returned = request(
        Endpoint::Agent(agent()),
        Endpoint::Human(human()),
        Relationship::AgentAuthorized(send_route(9, 9)),
        10,
    );
    let received = request(
        Endpoint::Agent(agent()),
        Endpoint::Human(human()),
        Relationship::PayerGrant(receive_route(10, 10, 10)),
        10,
    );

    let cases = [
        (
            deposit,
            MovementTerm::Deposit,
            Mechanism::BridgeDepositCredit,
        ),
        (
            withdrawal,
            MovementTerm::Withdrawal,
            Mechanism::BridgeWithdrawRequest,
        ),
        (transfer, MovementTerm::Transfer, Mechanism::Send),
        (allocate, MovementTerm::Allocate, Mechanism::Send),
        (returned, MovementTerm::Return, Mechanism::Send),
        (
            received,
            MovementTerm::Return,
            Mechanism::ReceiveUnderPayerGrant,
        ),
    ];
    for (input, term, mechanism) in cases {
        let route = RouteResolver::resolve(&input)
            .unwrap_or_else(|error| panic!("normative route: {error}"));
        assert_eq!(
            layerx_human_service::journeys::Route::user_action(),
            "Move money"
        );
        assert_eq!(route.legs().len(), 1);
        assert_eq!(route.legs()[0].term(), term);
        assert_eq!(route.legs()[0].mechanism(), mechanism);
    }
}

#[test]
fn missing_budget_is_composed_as_create_then_fund_without_partial_output() {
    let input = request(
        Endpoint::Human(human()),
        Endpoint::AgentBudget(budget()),
        Relationship::ManagedBudget(BudgetRoute {
            budget_id: BudgetId::new([0x61; 32]),
            idempotency_key: key(0x62),
            revocation_sequence: Sequence::from_u64(11),
            create: Some(BudgetCreation {
                per_period_limit: Amount::from_u128(1_000),
                period_length: PeriodLength::new(3_600)
                    .unwrap_or_else(|error| panic!("period: {error:?}")),
                rollover: RolloverPolicy::Capped,
                carry_cap: Amount::from_u128(500),
                purpose: PurposeHash::new([0x63; 32]),
                expiry: TimestampSeconds::from_u64(20_000),
            }),
        }),
        250,
    );
    let route = RouteResolver::resolve(&input)
        .unwrap_or_else(|error| panic!("composed budget route: {error}"));
    assert_eq!(route.legs().len(), 2);
    assert_eq!(route.legs()[0].mechanism(), Mechanism::BudgetCreate);
    assert!(matches!(
        route.legs()[0].intent().kind(),
        IntentKind::BudgetCreate(_)
    ));
    assert_eq!(route.legs()[1].mechanism(), Mechanism::BudgetFund);
    assert!(matches!(
        route.legs()[1].intent().kind(),
        IntentKind::BudgetFund(_)
    ));
    assert!(route
        .legs()
        .iter()
        .all(|leg| leg.term() == MovementTerm::Fund));

    let invalid = RouteRequest {
        amount: Amount::ZERO,
        ..input
    };
    assert!(matches!(
        RouteResolver::resolve(&invalid),
        Err(RouteError::InvalidIntent(_))
    ));
}

#[test]
fn custody_words_cannot_escape_the_wallet_boundary() {
    let internal_with_custody_material = request(
        Endpoint::Human(human()),
        Endpoint::Agent(agent()),
        Relationship::Custody(CustodyRoute::Deposit {
            deposit_proof: DepositProofId::new([1; 32]),
            checkpoint: CheckpointId::new([2; 32]),
            reserve: account("system:paxeer-reserve"),
            idempotency_key: key(3),
        }),
        1,
    );
    assert!(matches!(
        RouteResolver::resolve(&internal_with_custody_material),
        Err(RouteError::Unavailable { .. })
    ));
    assert_eq!(MovementTerm::Deposit.as_str(), "deposit");
    assert_eq!(MovementTerm::Withdrawal.as_str(), "withdrawal");
    assert_eq!(MovementTerm::Fund.as_str(), "fund");
    assert_eq!(MovementTerm::Allocate.as_str(), "allocate");
    assert_eq!(MovementTerm::Return.as_str(), "return");
    assert_eq!(MovementTerm::Transfer.as_str(), "transfer");
}

#[test]
fn refusals_name_the_limit_and_link_only_human_owned_settings() {
    let budget = LimitRefusal::new(
        LimitSource::Budget,
        "daily agent allowance",
        Some(ChangeSurface::Budget),
    )
    .unwrap_or_else(|error| panic!("budget refusal: {error}"));
    assert!(budget.plain_language().contains("daily agent allowance"));
    assert_eq!(budget.change_path(), Some("/agents/budgets"));

    let protocol = LimitRefusal::new(LimitSource::Protocol, "account sequence", None)
        .unwrap_or_else(|error| panic!("protocol refusal: {error}"));
    assert!(protocol.plain_language().contains("protocol"));
    assert_eq!(protocol.change_path(), None);
    assert!(LimitRefusal::new(
        LimitSource::Protocol,
        "account sequence",
        Some(ChangeSurface::Policy)
    )
    .is_err());
}

fn endpoint(selector: u8) -> Endpoint {
    match selector % 4 {
        0 => Endpoint::PaxeerWallet,
        1 => Endpoint::Human(human()),
        2 => Endpoint::Agent(agent()),
        _ => Endpoint::AgentBudget(budget()),
    }
}

fn relationship(selector: u8, sequence: u64, byte: u8) -> Relationship {
    let nonzero = byte.max(1);
    match selector % 6 {
        0 => Relationship::Direct(send_route(sequence, nonzero)),
        1 => Relationship::AgentAuthorized(send_route(sequence, nonzero)),
        2 => Relationship::ManagedBudget(BudgetRoute {
            budget_id: BudgetId::new([nonzero; 32]),
            idempotency_key: key(nonzero),
            revocation_sequence: Sequence::from_u64(sequence),
            create: None,
        }),
        3 => Relationship::PayerGrant(receive_route(sequence, nonzero, 1)),
        4 => Relationship::Custody(CustodyRoute::Deposit {
            deposit_proof: DepositProofId::new([nonzero; 32]),
            checkpoint: CheckpointId::new([nonzero; 32]),
            reserve: account("system:paxeer-reserve"),
            idempotency_key: key(nonzero),
        }),
        _ => Relationship::Custody(CustodyRoute::Withdrawal {
            request_anchor: CheckpointId::new([18; 32]),
            fee_limit: 100,
            withdrawals_account: account("system:paxeer-withdrawals"),
            payout_address: EvmAddress::new([nonzero; 20]),
            idempotency_key: key(nonzero),
        }),
    }
}

proptest! {
    #[test]
    fn equivalent_requests_always_choose_byte_for_byte_equivalent_routes(
        amount in 1_u128..u128::MAX,
        sequence in any::<u64>(),
        key_byte in 1_u8..=u8::MAX,
    ) {
        let input = request(
            Endpoint::Human(human()),
            Endpoint::Agent(agent()),
            Relationship::Direct(send_route(sequence, key_byte)),
            amount,
        );
        let equivalent = input.clone();
        prop_assert_eq!(
            RouteResolver::resolve(&input),
            RouteResolver::resolve(&equivalent)
        );
    }

    #[test]
    fn every_declared_input_is_total_and_never_returns_partial_work(
        source in any::<u8>(),
        destination in any::<u8>(),
        relation in any::<u8>(),
        amount in any::<u128>(),
        sequence in any::<u64>(),
        key_byte in any::<u8>(),
    ) {
        let input = request(
            endpoint(source),
            endpoint(destination),
            relationship(relation, sequence, key_byte),
            amount,
        );
        match RouteResolver::resolve(&input) {
            Ok(route) => {
                prop_assert!(!route.legs().is_empty());
                for leg in route.legs() {
                    if leg.term().is_custody_boundary() {
                        prop_assert!(matches!(
                            (input.source.kind(), input.destination.kind()),
                            (
                                layerx_human_service::journeys::EndpointKind::PaxeerWallet,
                                layerx_human_service::journeys::EndpointKind::Human
                            ) | (
                                layerx_human_service::journeys::EndpointKind::Human,
                                layerx_human_service::journeys::EndpointKind::PaxeerWallet
                            )
                        ));
                    }
                }
            }
            Err(RouteError::Unavailable { .. } | RouteError::InvalidIntent(_)) => {}
        }
    }
}

fn receive_route(sequence: u64, byte: u8, amount: u128) -> PayerGrantRoute {
    let canonical = layerx_human_test_support::receive::signed_receive(
        &layerx_human_test_support::receive::SignedReceiveRequest {
            from: &agent(),
            to: &human(),
            asset: asset().bytes(),
            amount,
            sequence,
            idempotency_key: key(byte).bytes(),
            network_id: 77,
            protocol_version: layerx_wire::limits::PROTOCOL_VERSION,
        },
    );
    PayerGrantRoute {
        receive: layerx_intents::NativeReceive::new(&canonical)
            .unwrap_or_else(|error| panic!("signed receive: {error:?}")),
    }
}

#[test]
fn receive_route_requires_complete_signed_authority_and_exact_request() {
    let input = request(
        Endpoint::Agent(agent()),
        Endpoint::Human(human()),
        Relationship::PayerGrant(receive_route(7, 19, 13)),
        13,
    );
    let canonical = input.canonical_encode();
    assert_eq!(
        RouteRequest::canonical_decode(&canonical),
        Ok(input.clone())
    );
    let Relationship::PayerGrant(route) = &input.relationship else {
        panic!("payer grant route")
    };
    let signed = route.receive.payload();
    for index in 0..signed.len() {
        let mut corrupted = *signed;
        corrupted[index] ^= 1;
        assert!(
            layerx_intents::NativeReceive::new(&corrupted).is_err(),
            "authorization byte {index}"
        );
    }
    for length in [0, 116, 220, 387, 732] {
        assert!(layerx_intents::NativeReceive::new(&signed[..length]).is_err());
    }
    let mut wrong = input.clone();
    wrong.amount = Amount::from_u128(14);
    assert!(RouteResolver::resolve(&wrong).is_err());
    wrong = input.clone();
    wrong.asset = AssetId::new([0x42; 32]);
    assert!(RouteResolver::resolve(&wrong).is_err());
    wrong = input;
    wrong.destination = Endpoint::Human(account("agent:did:layerx:other:main"));
    assert!(RouteResolver::resolve(&wrong).is_err());
}
