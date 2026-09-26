use layerx_types::payload::{
    ActivityType, ModuleId, ModuleRegistration, ModuleRegistry, PerpsMarket, PerpsPayload,
    SpotOrderKind, SpotPayload, SpotTimeInForce, TradeSide, TradingPayloadError,
};

const VECTORS: &str = include_str!("../../../../tests/fixtures/trading-payloads/vectors.json");

fn vector(name: &str) -> (u32, Vec<u8>) {
    let marker = format!("\"name\": \"{name}\"");
    let Some(line) = VECTORS.lines().find(|line| line.contains(&marker)) else {
        panic!("missing vector {name}");
    };
    let field = |key: &str| -> &str {
        let tag = format!("\"{key}\": ");
        let Some(start) = line.find(&tag) else {
            panic!("vector {name} lacks {key}");
        };
        let rest = &line[start + tag.len()..];
        let end = rest.find([',', '}']).unwrap_or(rest.len());
        rest[..end].trim_matches('"')
    };
    let Ok(activity) = field("activity_type").parse::<u32>() else {
        panic!("vector {name} activity type");
    };
    let hex = field("bytes");
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|index| {
            let Ok(byte) = u8::from_str_radix(&hex[index..index + 2], 16) else {
                panic!("vector {name} hex");
            };
            byte
        })
        .collect();
    (activity, bytes)
}

fn perps_type(ordinal: u16) -> ActivityType {
    let Ok(value) = ActivityType::new(ModuleId::Perps, ordinal) else {
        panic!("perps ordinal rejected");
    };
    value
}

fn market() -> PerpsMarket {
    PerpsMarket {
        market_id: [0x11; 32],
        quote_asset: [0x12; 32],
        administrator: [0x13; 32],
        liquidity_account_id: [0x14; 32],
        long_funding_account_id: [0x15; 32],
        short_funding_account_id: [0x16; 32],
        insurance_account_id: [0x17; 32],
        contract_size: 1,
        tick_size: 10,
        lot_size: 1000,
        price_scale: 100_000_000,
        initial_margin_ratio_bps: 1000,
        maintenance_margin_ratio_bps: 500,
        liquidation_fee_bps: 100,
        liquidator_share_bps: 5000,
        maximum_funding_rate_bps: 75,
        maximum_deviation_basis_points: 250,
        funding_interval_ms: 3_600_000,
        maximum_oracle_staleness_ms: 60_000,
        minimum_price: 1,
        maximum_price: (0x0102_0304_0506_0708_u128 << 64) | 0x090a_0b0c_0d0e_0f10,
        permitted_oracle_keys: vec![[0x21; 32], [0x22; 32]],
        parameter_version: 1,
        halted: false,
    }
}

fn perps_cases() -> Vec<(&'static str, PerpsPayload)> {
    vec![
        (
            "perps_market_create",
            PerpsPayload::MarketCreate(Box::new(market())),
        ),
        (
            "perps_market_halt",
            PerpsPayload::MarketHalt {
                market_id: [0x11; 32],
                halted: true,
            },
        ),
        (
            "perps_oracle_push",
            PerpsPayload::OraclePush {
                market_id: [0x11; 32],
                observation_sequence: 7,
                price: 123_456_789,
                observed_at: 1_700_000_000_000,
                source_identifier: 42,
            },
        ),
        (
            "perps_order_place",
            PerpsPayload::OrderPlace {
                market_id: [0x11; 32],
                order_id: [0x31; 32],
                owner_account_id: [0x32; 32],
                side: TradeSide::Sell,
                price: (1_u128 << 64) | 2,
                quantity: 5000,
            },
        ),
        (
            "perps_order_cancel",
            PerpsPayload::OrderCancel {
                market_id: [0x11; 32],
                order_id: [0x31; 32],
            },
        ),
        (
            "perps_position_open",
            PerpsPayload::PositionOpen {
                market_id: [0x11; 32],
                position_id: [0x41; 32],
                margin_account_id: [0x42; 32],
                side: TradeSide::Buy,
                size: 1000,
                entry_notional: 0,
                margin_amount: 250_000,
            },
        ),
        (
            "perps_position_increase",
            PerpsPayload::PositionIncrease {
                market_id: [0x11; 32],
                position_id: [0x41; 32],
                size_delta: 500,
                notional_delta: 0,
                margin_amount: 1000,
            },
        ),
        (
            "perps_position_close",
            PerpsPayload::PositionClose {
                market_id: [0x11; 32],
                position_id: [0x41; 32],
            },
        ),
        (
            "perps_funding_tick",
            PerpsPayload::FundingTick {
                market_id: [0x11; 32],
            },
        ),
        (
            "perps_liquidate",
            PerpsPayload::Liquidate {
                market_id: [0x11; 32],
                position_id: [0x41; 32],
                liquidator_account_id: [0x51; 32],
            },
        ),
        (
            "perps_adl",
            PerpsPayload::Adl {
                market_id: [0x11; 32],
                position_ids: vec![[0x61; 32], [0x62; 32], [0x63; 32]],
            },
        ),
    ]
}

fn spot_cases() -> Vec<(&'static str, SpotPayload)> {
    let limit = SpotPayload::OrderPlace {
        market_id: [0x71; 32],
        order_id: [0x81; 32],
        base_account_id: [0x82; 32],
        quote_account_id: [0x83; 32],
        side: TradeSide::Buy,
        kind: SpotOrderKind::Limit,
        time_in_force: SpotTimeInForce::GoodTilCancelled,
        price: 250,
        quantity: (0x0102_0304_0506_0708_u128 << 64) | 0x090a_0b0c_0d0e_0f10,
    };
    let market = SpotPayload::OrderPlace {
        market_id: [0x71; 32],
        order_id: [0x84; 32],
        base_account_id: [0x82; 32],
        quote_account_id: [0x83; 32],
        side: TradeSide::Sell,
        kind: SpotOrderKind::Market,
        time_in_force: SpotTimeInForce::ImmediateOrCancel,
        price: 0,
        quantity: 400,
    };
    vec![
        (
            "spot_market_create",
            SpotPayload::MarketCreate {
                market_id: [0x71; 32],
                base_asset: [0x72; 32],
                quote_asset: [0x73; 32],
                tick_size: 5,
                lot_size: 100,
                administrator: [0x74; 32],
            },
        ),
        ("spot_order_place_limit", limit),
        ("spot_order_place_market", market),
        (
            "spot_order_cancel",
            SpotPayload::OrderCancel {
                market_id: [0x71; 32],
                order_id: [0x81; 32],
            },
        ),
        (
            "spot_market_halt",
            SpotPayload::MarketHalt {
                market_id: [0x71; 32],
            },
        ),
        (
            "spot_market_resume",
            SpotPayload::MarketResume {
                market_id: [0x71; 32],
            },
        ),
    ]
}

#[test]
fn perps_payload_encodings_match_the_kernel_codec_vectors() {
    for (name, payload) in perps_cases() {
        let (activity, bytes) = vector(name);
        assert_eq!(payload.activity_type().value(), activity, "{name}");
        assert_eq!(payload.encode(), Ok(bytes.clone()), "{name}");
        assert_eq!(
            PerpsPayload::decode(payload.activity_type(), &bytes),
            Ok(payload),
            "{name}"
        );
    }
}

#[test]
fn spot_payload_encodings_match_the_kernel_codec_vectors() {
    for (name, payload) in spot_cases() {
        let (activity, bytes) = vector(name);
        assert_eq!(payload.activity_value(), activity, "{name}");
        assert_eq!(payload.encode(), Ok(bytes.clone()), "{name}");
        assert_eq!(SpotPayload::decode(activity, &bytes), Ok(payload), "{name}");
    }
}

#[test]
fn perps_payload_round_trips_through_a_declared_module_payload() {
    let declared: Vec<_> = (1..=11).map(perps_type).collect();
    let Ok(registration) = ModuleRegistration::new(ModuleId::Perps, &declared) else {
        panic!("perps registration rejected");
    };
    let Ok(registry) = ModuleRegistry::new(&[registration]) else {
        panic!("perps registry rejected");
    };
    for (name, payload) in perps_cases() {
        let Ok(wrapped) = payload.payload(&registry) else {
            panic!("{name} payload rejected");
        };
        assert_eq!(PerpsPayload::from_payload(&wrapped), Ok(payload), "{name}");
    }
}

#[test]
fn perps_payload_decoder_refuses_what_the_kernel_refuses() {
    let (_, mut halt) = vector("perps_market_halt");
    halt[32] = 2;
    assert_eq!(
        PerpsPayload::decode(perps_type(2), &halt),
        Err(TradingPayloadError::NonCanonical)
    );
    let (_, mut order) = vector("perps_order_place");
    order[96] = 3;
    assert_eq!(
        PerpsPayload::decode(perps_type(4), &order),
        Err(TradingPayloadError::NonCanonical)
    );
    let (_, open) = vector("perps_position_open");
    assert_eq!(
        PerpsPayload::decode(perps_type(6), &open[..144]),
        Err(TradingPayloadError::Length(144))
    );
    let (_, mut market) = vector("perps_market_create");
    let padding = 32 * 7 + 16 * 4 + 4 * 6 + 8 * 2 + 16 * 2 + 1 + 64;
    market[padding] = 1;
    assert_eq!(
        PerpsPayload::decode(perps_type(1), &market),
        Err(TradingPayloadError::NonCanonical)
    );
    let (_, mut adl) = vector("perps_adl");
    adl[33..65].copy_from_slice(&[0x64; 32]);
    assert_eq!(
        PerpsPayload::decode(perps_type(11), &adl),
        Err(TradingPayloadError::UnsortedSequence)
    );
    assert_eq!(
        PerpsPayload::decode(perps_type(12), &[1; 32]),
        Err(TradingPayloadError::UnknownActivity(0x0006_000c))
    );
    let mut bounded = market_with_initial_margin(500);
    assert_eq!(
        PerpsPayload::MarketCreate(Box::new(bounded.clone())).encode(),
        Err(TradingPayloadError::ParameterBounds)
    );
    bounded.initial_margin_ratio_bps = 1000;
    bounded.permitted_oracle_keys.reverse();
    assert_eq!(
        PerpsPayload::MarketCreate(Box::new(bounded)).encode(),
        Err(TradingPayloadError::ParameterBounds)
    );
    assert_eq!(
        PerpsPayload::PositionOpen {
            market_id: [0x11; 32],
            position_id: [0x41; 32],
            margin_account_id: [0x42; 32],
            side: TradeSide::Buy,
            size: 1000,
            entry_notional: 1,
            margin_amount: 250_000,
        }
        .encode(),
        Err(TradingPayloadError::NonCanonical)
    );
}

fn market_with_initial_margin(initial: u32) -> PerpsMarket {
    let mut value = market();
    value.initial_margin_ratio_bps = initial;
    value
}

#[test]
fn spot_payload_decoder_refuses_what_the_kernel_refuses() {
    let (activity, mut order) = vector("spot_order_place_market");
    order[130] = 1;
    assert_eq!(
        SpotPayload::decode(activity, &order),
        Err(TradingPayloadError::NonCanonical)
    );
    let (activity, mut limit) = vector("spot_order_place_limit");
    limit[96..128].copy_from_slice(&[0x82; 32]);
    assert_eq!(
        SpotPayload::decode(activity, &limit),
        Err(TradingPayloadError::NonCanonical)
    );
    let (activity, mut market) = vector("spot_market_create");
    market[64..96].copy_from_slice(&[0x72; 32]);
    assert_eq!(
        SpotPayload::decode(activity, &market),
        Err(TradingPayloadError::NonCanonical)
    );
    assert_eq!(
        SpotPayload::decode(0x000a_0006, &[1; 32]),
        Err(TradingPayloadError::UnknownActivity(0x000a_0006))
    );
    assert_eq!(
        SpotPayload::decode(0x000a_0004, &[0; 32]),
        Err(TradingPayloadError::NonCanonical)
    );
}
