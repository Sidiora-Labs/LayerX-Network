use std::fmt::Debug;

use layerx_intents::precompile::{
    keccak256, route, route_event, EventDecodeError, EvmLog, ExchangeOrder, MarginDeposited,
    PrecompileEvent, PrecompileEventKind, RouteBinding, RouteError, BRIDGE_PRECOMPILE,
    EXCHANGE_PRECOMPILE, LAUNCHPAD_PRECOMPILE,
};
use layerx_intents::{
    compile, BridgeWithdrawRequest, CompileErrorReason, DisclosureCheck, DisclosureCheckError,
    Intent, IntentErrorReason, IntentField, IntentKind, IntentVersion, NativeCustodyCredit,
};
use layerx_types::account::AccountId;
use layerx_types::amount::Amount;
use layerx_types::ids::{AssetId, CheckpointId, IdempotencyKey};
use layerx_types::intent::EvmAddress;
use layerx_types::payload::{
    ActivityType, ModuleId, ModuleRegistration, ModuleRegistry, PerpsPayload, TradeSide,
};

const CREDIT: &[u8] =
    include_bytes!("../../../../tests/fixtures/custody/paxeer-light-v1/custody.credit");
const VECTORS: &str = include_str!("../../../../tests/fixtures/trading-payloads/vectors.json");
const EXCHANGE_ABI: &str = include_str!("../../../../precompiles/layerxexchange/abi.json");
const BRIDGE_ABI: &str = include_str!("../../../../precompiles/layerxbridge/abi.json");
const LAUNCHPAD_ABI: &str = include_str!("../../../../precompiles/launchpad/abi.json");

fn checked<T, E: Debug>(value: Result<T, E>) -> T {
    value.unwrap_or_else(|error| panic!("{error:?}"))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|index| checked(u8::from_str_radix(&text[index..index + 2], 16)))
        .collect()
}

fn vector_bytes(name: &str) -> Vec<u8> {
    let marker = format!("\"name\": \"{name}\"");
    let line = VECTORS
        .lines()
        .find(|line| line.contains(&marker))
        .unwrap_or_else(|| panic!("vector {name}"));
    let tag = "\"bytes\": \"";
    let start = line.find(tag).unwrap_or_else(|| panic!("bytes of {name}")) + tag.len();
    let end = start
        + line[start..]
            .find('"')
            .unwrap_or_else(|| panic!("end of {name}"));
    unhex(&line[start..end])
}

fn abi_objects(abi: &str) -> Vec<&str> {
    let mut objects = Vec::new();
    let mut depth = 0_usize;
    let mut start = 0;
    for (index, byte) in abi.bytes().enumerate() {
        match byte {
            b'{' => {
                if depth == 0 {
                    start = index;
                }
                depth += 1;
            }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    objects.push(&abi[start..=index]);
                }
            }
            _ => {}
        }
    }
    objects
}

fn string_after<'a>(text: &'a str, key: &str) -> &'a str {
    let rest = &text[text.find(key).unwrap_or_else(|| panic!("{key}")) + key.len()..];
    &rest[..rest.find('"').unwrap_or_else(|| panic!("{key} end"))]
}

fn abi_events(abi: &str) -> Vec<(String, usize)> {
    abi_objects(abi)
        .into_iter()
        .filter_map(|object| {
            let inputs_start = object.find("\"inputs\":[")? + 10;
            let inputs_end = inputs_start + object[inputs_start..].find(']')?;
            let inputs = &object[inputs_start..inputs_end];
            let outer = format!("{}{}", &object[..inputs_start], &object[inputs_end..]);
            if !outer.contains("\"type\":\"event\"") {
                return None;
            }
            let types: Vec<&str> = abi_objects(inputs)
                .into_iter()
                .map(|input| string_after(input, "\"type\":\""))
                .collect();
            Some((
                format!(
                    "{}({})",
                    string_after(&outer, "\"name\":\""),
                    types.join(",")
                ),
                inputs.matches("\"indexed\":true").count(),
            ))
        })
        .collect()
}

fn word_u64(value: u64) -> [u8; 32] {
    word_u128(u128::from(value))
}

fn word_u128(value: u128) -> [u8; 32] {
    let mut word = [0; 32];
    word[16..].copy_from_slice(&value.to_be_bytes());
    word
}

fn word_address(value: [u8; 20]) -> [u8; 32] {
    let mut word = [0; 32];
    word[12..].copy_from_slice(&value);
    word
}

fn data(words: &[[u8; 32]]) -> Vec<u8> {
    words.concat()
}

fn string_tail(text: &str) -> Vec<u8> {
    let mut out = word_u64(checked(u64::try_from(text.len()))).to_vec();
    out.extend_from_slice(text.as_bytes());
    out.resize(32 + text.len().div_ceil(32) * 32, 0);
    out
}

fn perps_registry() -> ModuleRegistry {
    let declared: Vec<_> = (1..=11)
        .map(|ordinal| checked(ActivityType::new(ModuleId::Perps, ordinal)))
        .collect();
    checked(ModuleRegistry::new(&[
        checked(ModuleRegistration::new(ModuleId::Perps, &declared)),
        checked(ModuleRegistration::new(
            ModuleId::Bridge,
            &[checked(ActivityType::new(ModuleId::Bridge, 1))],
        )),
        checked(ModuleRegistration::new(
            ModuleId::Asset,
            &[checked(ActivityType::new(ModuleId::Asset, 9))],
        )),
    ]))
}

fn order_log_topics(time_in_force: u8) -> (Vec<[u8; 32]>, Vec<u8>) {
    let topics = vec![
        PrecompileEventKind::OrderPlaced.topic0(),
        [0x31; 32],
        [0x11; 32],
        word_address([0xab; 20]),
    ];
    let body = data(&[
        word_u64(2),
        word_u128((1_u128 << 64) | 2),
        word_u128(5000),
        word_u64(u64::from(time_in_force)),
        word_u64(9),
    ]);
    (topics, body)
}

fn market_binding() -> RouteBinding {
    RouteBinding::Market {
        market_id: [0x11; 32],
        owner_account_id: [0x32; 32],
    }
}

#[test]
fn keccak_matches_published_digests() {
    assert_eq!(
        hex(&keccak256(b"")),
        "c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470"
    );
    assert_eq!(
        hex(&keccak256(b"Transfer(address,address,uint256)")),
        "ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
    );
    assert_eq!(
        hex(&keccak256(&[0x61; 200])),
        "96ea54061def936c4be90b518992fdc6f12f535068a256229aca54267b4d084d"
    );
    assert_eq!(
        hex(&keccak256(&[0; 136])),
        "3a5912a7c5faa06ee4fe906253e339467a9ce87d533c65be3c15cb231cdb25f9"
    );
}

#[test]
fn every_abi_event_is_declared_with_its_computed_topic() {
    let declared: Vec<(String, usize)> = [EXCHANGE_ABI, BRIDGE_ABI, LAUNCHPAD_ABI]
        .into_iter()
        .flat_map(abi_events)
        .collect();
    assert_eq!(declared.len(), PrecompileEventKind::ALL.len());
    for kind in PrecompileEventKind::ALL {
        assert!(
            declared
                .iter()
                .any(|(signature, _)| signature == kind.signature()),
            "{}",
            kind.signature()
        );
        assert_eq!(
            PrecompileEventKind::identify(kind.precompile(), kind.topic0()),
            Some(kind)
        );
    }
    let expected = [
        (
            PrecompileEventKind::MarginDeposited,
            "456ba29aa60d5cac1a6dc1c0f3df30b1f18963fd90dafe8c5f4a6de80440118a",
        ),
        (
            PrecompileEventKind::MarginWithdrawalRequested,
            "9cf8600ce0e07d0d2b0b82ed99fc940eb6121143201d8d682c25fc74972c88f0",
        ),
        (
            PrecompileEventKind::OrderCancelRequested,
            "39148489da3c16ee8c589a95e2f0c869816ee4f816b4ba1b85b32bc6d94c0241",
        ),
        (
            PrecompileEventKind::OrderPlaced,
            "88b93538701d726739ade066c2a9f09e5088f608d9b2ebba0cf305f5ee0752ce",
        ),
        (
            PrecompileEventKind::SettlementRequested,
            "70d5b9c37994017669de2a991c65a88ebeb34999f37c6dc6d0c44462d57eb655",
        ),
        (
            PrecompileEventKind::BridgeIn,
            "4352fb2e09bdaa35c4d407ce85dfa93eaec318876eddd6f5a490cce830c3f274",
        ),
        (
            PrecompileEventKind::BridgeOut,
            "3e990eb54009dcdca53d8fa87307210f07097f37dcf6185dee71a42f8e7d524e",
        ),
        (
            PrecompileEventKind::AirdropClaimed,
            "d399c6e7fad358fc300beda3f056717c94a04c7233ce92683de6500ba509022e",
        ),
        (
            PrecompileEventKind::AirdropExecuted,
            "171b2f9dc7a4c7eaa8ca718bcac62fbec15d147f033f38e42971b7ccabe9a469",
        ),
        (
            PrecompileEventKind::FeeRecorded,
            "b4d4d3bd2f97a7d6f1657ee69f7191d7aa7dbd5b6864a2d7a9d14efc1322552f",
        ),
        (
            PrecompileEventKind::FeeStrategyChanged,
            "66c2a2c42cf36fad89e5da817a0b5de0fd78d7481cbfdc59f604148252da2261",
        ),
        (
            PrecompileEventKind::FeesBurned,
            "0d9575a73e2a7da16cfde907df749d23d901528ff2e7c832b731babdecca000b",
        ),
        (
            PrecompileEventKind::FeesClaimed,
            "fe3464cd748424446c37877c28ce5b700222c5bc9f90d908afcc4e5cb22707ff",
        ),
        (
            PrecompileEventKind::LpRewardsExecuted,
            "a9e7850d400945e0434ddd18a194aff01f31efaae577a63486c3dc865c5ab759",
        ),
        (
            PrecompileEventKind::MarketCreated,
            "d8ad483b7300b5831650c4747b4d85390539f25f7d7d8c635eb3f8147daf198e",
        ),
        (
            PrecompileEventKind::PauseToggled,
            "79a5bc58b021076f821571d0fe8b0ae3d9e0a666563bb064fdbf0bf69281331c",
        ),
        (
            PrecompileEventKind::Swap,
            "f3369c7e0aa652773c7246b5481ca4b1ee0b408d90467d2ce93b165b9938fde5",
        ),
    ];
    for (kind, topic) in expected {
        assert_eq!(hex(&kind.topic0()), topic, "{}", kind.signature());
    }
}

#[test]
fn exchange_order_routes_to_the_kernel_perps_order_bytes() {
    let (topics, body) = order_log_topics(0);
    let log = EvmLog {
        address: EXCHANGE_PRECOMPILE,
        topics: &topics,
        data: &body,
    };
    let intent = checked(route(&log, market_binding()));
    let IntentKind::ExchangeOrder(order) = intent.kind() else {
        panic!("order routed to {:?}", intent.kind());
    };
    assert_eq!(order.event().owner, [0xab; 20]);
    assert_eq!(order.event().nonce, 9);
    assert_eq!(intent.module(), ModuleId::Perps);
    let compiled = checked(compile(&intent, &perps_registry()));
    assert_eq!(compiled.activity_type().value(), 0x0006_0004);
    assert_eq!(
        compiled.payload().as_bytes(),
        vector_bytes("perps_order_place")
    );
    let check = checked(DisclosureCheck::verify(&intent, &compiled));
    assert_eq!(check.canonical_payload(), compiled.payload().as_bytes());
    assert_eq!(
        checked(PerpsPayload::from_payload(compiled.payload())),
        PerpsPayload::OrderPlace {
            market_id: [0x11; 32],
            order_id: [0x31; 32],
            owner_account_id: [0x32; 32],
            side: TradeSide::Sell,
            price: (1_u128 << 64) | 2,
            quantity: 5000,
        }
    );
}

#[test]
fn exchange_cancel_and_settle_route_to_perps_cancel_and_close() {
    let registry = perps_registry();
    let cancel_topics = [
        PrecompileEventKind::OrderCancelRequested.topic0(),
        [0x77; 32],
        [0x31; 32],
        word_address([0xab; 20]),
    ];
    let nonce = data(&[word_u64(10)]);
    let cancel = checked(route(
        &EvmLog {
            address: EXCHANGE_PRECOMPILE,
            topics: &cancel_topics,
            data: &nonce,
        },
        market_binding(),
    ));
    let compiled = checked(compile(&cancel, &registry));
    assert_eq!(compiled.activity_type().value(), 0x0006_0005);
    assert_eq!(
        compiled.payload().as_bytes(),
        vector_bytes("perps_order_cancel")
    );
    checked(DisclosureCheck::verify(&cancel, &compiled));

    let settle_topics = [
        PrecompileEventKind::SettlementRequested.topic0(),
        [0x78; 32],
        [0x41; 32],
        word_address([0xab; 20]),
    ];
    let settle = checked(route(
        &EvmLog {
            address: EXCHANGE_PRECOMPILE,
            topics: &settle_topics,
            data: &nonce,
        },
        market_binding(),
    ));
    let compiled = checked(compile(&settle, &registry));
    assert_eq!(compiled.activity_type().value(), 0x0006_0008);
    assert_eq!(
        compiled.payload().as_bytes(),
        vector_bytes("perps_position_close")
    );
    checked(DisclosureCheck::verify(&settle, &compiled));
}

#[test]
fn direct_perps_intents_compile_every_kernel_vector() {
    let registry = perps_registry();
    for (name, ordinal) in [
        ("perps_market_create", 1),
        ("perps_market_halt", 2),
        ("perps_oracle_push", 3),
        ("perps_order_place", 4),
        ("perps_order_cancel", 5),
        ("perps_position_open", 6),
        ("perps_position_increase", 7),
        ("perps_position_close", 8),
        ("perps_funding_tick", 9),
        ("perps_liquidate", 10),
        ("perps_adl", 11),
    ] {
        let bytes = vector_bytes(name);
        let payload = checked(PerpsPayload::decode(
            checked(ActivityType::new(ModuleId::Perps, ordinal)),
            &bytes,
        ));
        let intent = Intent::v1(IntentKind::Perps(payload));
        let compiled = checked(compile(&intent, &registry));
        assert_eq!(compiled.payload().as_bytes(), bytes, "{name}");
        checked(DisclosureCheck::verify(&intent, &compiled));
        assert!(compile(&Intent::v2(intent.kind().clone()), &registry).is_err());
    }
}

#[test]
fn exchange_order_refuses_what_perps_cannot_carry() {
    let (topics, body) = order_log_topics(1);
    let log = EvmLog {
        address: EXCHANGE_PRECOMPILE,
        topics: &topics,
        data: &body,
    };
    let Err(RouteError::Intent(error)) = route(&log, market_binding()) else {
        panic!("IOC order routed");
    };
    assert_eq!(error.field, IntentField::TimeInForce);
    let (topics, body) = order_log_topics(0);
    let log = EvmLog {
        address: EXCHANGE_PRECOMPILE,
        topics: &topics,
        data: &body,
    };
    assert_eq!(
        route(&log, RouteBinding::Unbound),
        Err(RouteError::Binding(PrecompileEventKind::OrderPlaced))
    );
    let other_market = RouteBinding::Market {
        market_id: [0x12; 32],
        owner_account_id: [0x32; 32],
    };
    let Err(RouteError::Intent(error)) = route(&log, other_market) else {
        panic!("order routed to another market");
    };
    assert_eq!(error.reason, IntentErrorReason::EventMismatch);
    let PrecompileEvent::OrderPlaced(mut event) = checked(PrecompileEvent::decode(&log)) else {
        panic!("order event");
    };
    event.price = [0xff; 32];
    let Err(error) = ExchangeOrder::new(event, [0x32; 32]) else {
        panic!("over-wide price routed");
    };
    assert_eq!(
        (error.field, error.reason),
        (IntentField::Amount, IntentErrorReason::InvalidRange)
    );
}

#[test]
fn precompile_decoder_refuses_foreign_or_malformed_logs() {
    let (topics, body) = order_log_topics(0);
    let foreign = EvmLog {
        address: BRIDGE_PRECOMPILE,
        topics: &topics,
        data: &body,
    };
    assert_eq!(
        PrecompileEvent::decode(&foreign),
        Err(EventDecodeError::UnknownEvent)
    );
    let short = EvmLog {
        address: EXCHANGE_PRECOMPILE,
        topics: &topics,
        data: &body[..128],
    };
    assert_eq!(
        PrecompileEvent::decode(&short),
        Err(EventDecodeError::DataLength(128))
    );
    let mut dirty = topics.clone();
    dirty[3][0] = 1;
    let log = EvmLog {
        address: EXCHANGE_PRECOMPILE,
        topics: &dirty,
        data: &body,
    };
    assert_eq!(
        PrecompileEvent::decode(&log),
        Err(EventDecodeError::NonCanonicalWord)
    );
    let log = EvmLog {
        address: EXCHANGE_PRECOMPILE,
        topics: &topics[..3],
        data: &body,
    };
    assert_eq!(
        PrecompileEvent::decode(&log),
        Err(EventDecodeError::TopicCount(3))
    );
}

fn reserve() -> AccountId {
    checked(AccountId::parse("system:paxeer-reserve"))
}

fn credit_recipient() -> AccountId {
    checked(AccountId::parse(&format!(
        "agent:did:layerx:{}:main",
        hex(&CREDIT[139..171])
    )))
}

#[test]
fn margin_deposit_routes_to_the_matching_custody_credit() {
    let credit = checked(NativeCustodyCredit::new(
        CREDIT,
        reserve(),
        credit_recipient(),
    ));
    let mut amount = [0; 32];
    amount[16..].copy_from_slice(&CREDIT[191..207]);
    let topics = [
        PrecompileEventKind::MarginDeposited.topic0(),
        [0x55; 32],
        checked(<[u8; 32]>::try_from(&CREDIT[107..139])),
        word_address([0xab; 20]),
    ];
    let body = data(&[
        checked(<[u8; 32]>::try_from(&CREDIT[75..107])),
        amount,
        checked(<[u8; 32]>::try_from(&CREDIT[43..75])),
        word_u64(1),
    ]);
    let log = EvmLog {
        address: EXCHANGE_PRECOMPILE,
        topics: &topics,
        data: &body,
    };
    let intent = checked(route(&log, RouteBinding::Custody(credit.clone())));
    assert_eq!(intent.module(), ModuleId::Bridge);
    let registry = perps_registry();
    let compiled = checked(compile(&intent, &registry));
    let direct = checked(compile(
        &Intent::v1(IntentKind::NativeCustodyCredit(credit.clone())),
        &registry,
    ));
    assert_eq!(compiled, direct);
    checked(DisclosureCheck::verify(&intent, &compiled));

    let PrecompileEvent::MarginDeposited(event) = checked(PrecompileEvent::decode(&log)) else {
        panic!("deposit event");
    };
    let other = MarginDeposited {
        deposit_id: [0x66; 32],
        ..event
    };
    let Err(RouteError::Intent(error)) = route_event(
        PrecompileEvent::MarginDeposited(other),
        RouteBinding::Custody(credit),
    ) else {
        panic!("foreign deposit routed");
    };
    assert_eq!(
        (error.field, error.reason),
        (IntentField::DepositProof, IntentErrorReason::EventMismatch)
    );
}

#[test]
fn margin_withdrawal_routes_to_a_v2_asset_withdrawal() {
    let owner = [0xab; 20];
    let request = checked(BridgeWithdrawRequest::new(
        CheckpointId::new([18; 32]),
        100,
        checked(AccountId::parse("agent:did:layerx:human-compiler:main")),
        checked(AccountId::parse("system:paxeer-withdrawals")),
        EvmAddress::new(owner),
        AssetId::new([2; 32]),
        Amount::from_u128(25),
        IdempotencyKey::new([3; 32]),
    ));
    let topics = [
        PrecompileEventKind::MarginWithdrawalRequested.topic0(),
        [0x56; 32],
        [0x57; 32],
        word_address(owner),
    ];
    let body = data(&[[2; 32], word_u128(25), word_u64(2)]);
    let log = EvmLog {
        address: EXCHANGE_PRECOMPILE,
        topics: &topics,
        data: &body,
    };
    let intent = checked(route(&log, RouteBinding::Withdrawal(request.clone())));
    assert_eq!(intent.version(), IntentVersion::V2);
    let registry = perps_registry();
    let compiled = checked(compile(&intent, &registry));
    let direct = checked(compile(
        &Intent::v2(IntentKind::BridgeWithdrawRequest(request.clone())),
        &registry,
    ));
    assert_eq!(compiled, direct);
    checked(DisclosureCheck::verify(&intent, &compiled));
    let other = data(&[[2; 32], word_u128(26), word_u64(2)]);
    let Err(RouteError::Intent(error)) = route(
        &EvmLog {
            address: EXCHANGE_PRECOMPILE,
            topics: &topics,
            data: &other,
        },
        RouteBinding::Withdrawal(request),
    ) else {
        panic!("mismatched withdrawal routed");
    };
    assert_eq!(error.field, IntentField::Amount);
}

fn settled_logs() -> Vec<([u8; 20], Vec<[u8; 32]>, Vec<u8>)> {
    let token = word_address([0x0c; 20]);
    let who = word_address([0x0d; 20]);
    let mut bridge_in = data(&[
        word_u64(4),
        word_address([0x0e; 20]),
        word_u128(700),
        word_u64(128),
    ]);
    bridge_in.extend(string_tail("ibc/ATOM"));
    let mut created = data(&[word_u64(128), word_u64(192), word_u64(256), word_u64(2)]);
    created.extend(string_tail("factory/pad"));
    created.extend(string_tail("Pad Token"));
    created.extend(string_tail("PAD"));
    vec![
        (
            BRIDGE_PRECOMPILE,
            vec![
                PrecompileEventKind::BridgeIn.topic0(),
                word_u64(1),
                [0x0f; 32],
                who,
            ],
            bridge_in,
        ),
        (
            BRIDGE_PRECOMPILE,
            vec![
                PrecompileEventKind::BridgeOut.topic0(),
                word_u64(1),
                token,
                word_u64(3),
            ],
            data(&[word_u128(5), who]),
        ),
        (
            LAUNCHPAD_PRECOMPILE,
            vec![PrecompileEventKind::AirdropClaimed.topic0(), token, who],
            data(&[word_u128(1), word_u128(2)]),
        ),
        (
            LAUNCHPAD_PRECOMPILE,
            vec![PrecompileEventKind::AirdropExecuted.topic0(), token],
            data(&[word_u128(1), word_u128(2)]),
        ),
        (
            LAUNCHPAD_PRECOMPILE,
            vec![PrecompileEventKind::FeeRecorded.topic0(), token],
            data(&[word_u128(3), word_u128(1), word_u128(2)]),
        ),
        (
            LAUNCHPAD_PRECOMPILE,
            vec![PrecompileEventKind::FeeStrategyChanged.topic0(), token],
            data(&[word_u64(1), word_u64(2)]),
        ),
        (
            LAUNCHPAD_PRECOMPILE,
            vec![PrecompileEventKind::FeesBurned.topic0(), token],
            data(&[word_u128(9)]),
        ),
        (
            LAUNCHPAD_PRECOMPILE,
            vec![PrecompileEventKind::FeesClaimed.topic0(), token, who],
            data(&[word_u128(9)]),
        ),
        (
            LAUNCHPAD_PRECOMPILE,
            vec![PrecompileEventKind::LpRewardsExecuted.topic0(), token],
            data(&[word_u128(9)]),
        ),
        (
            LAUNCHPAD_PRECOMPILE,
            vec![PrecompileEventKind::MarketCreated.topic0(), token, who],
            created,
        ),
        (
            LAUNCHPAD_PRECOMPILE,
            vec![PrecompileEventKind::PauseToggled.topic0(), token],
            data(&[word_u64(1)]),
        ),
        (
            LAUNCHPAD_PRECOMPILE,
            vec![PrecompileEventKind::Swap.topic0(), token, who, who],
            data(&[
                word_u64(1),
                word_u128(10),
                word_u128(20),
                word_u128(1),
                word_u128(2),
            ]),
        ),
    ]
}

#[test]
fn paxeer_settled_events_route_but_carry_no_layerx_activity() {
    let registry = perps_registry();
    let logs = settled_logs();
    assert_eq!(logs.len(), 12);
    for (address, topics, body) in &logs {
        let log = EvmLog {
            address: *address,
            topics,
            data: body,
        };
        let intent = checked(route(&log, RouteBinding::Unbound));
        assert!(intent.kind().settled_on_paxeer());
        let Err(error) = compile(&intent, &registry) else {
            panic!("settled event compiled");
        };
        assert_eq!(error.reason, CompileErrorReason::SettledOnPaxeer);
        let perps = checked(compile(
            &Intent::v1(IntentKind::Perps(PerpsPayload::FundingTick {
                market_id: [0x11; 32],
            })),
            &registry,
        ));
        assert_eq!(
            DisclosureCheck::verify(&intent, &perps),
            Err(DisclosureCheckError::SettledOnPaxeer)
        );
    }
    let (address, topics, body) = &logs[0];
    let PrecompileEvent::BridgeIn(event) = checked(PrecompileEvent::decode(&EvmLog {
        address: *address,
        topics,
        data: body,
    })) else {
        panic!("bridge in");
    };
    assert_eq!(event.denom, "ibc/ATOM");
    assert_eq!(event.log_index, 4);
    let (address, topics, body) = &logs[9];
    let PrecompileEvent::MarketCreated(event) = checked(PrecompileEvent::decode(&EvmLog {
        address: *address,
        topics,
        data: body,
    })) else {
        panic!("market created");
    };
    assert_eq!(
        (
            event.denom.as_str(),
            event.name.as_str(),
            event.symbol.as_str()
        ),
        ("factory/pad", "Pad Token", "PAD")
    );
    assert_eq!(event.fee_strategy, 2);
}
