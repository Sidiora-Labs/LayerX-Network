use serde_json::Value;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use x_websearch::config::{
    AssetSymbol, Config, ConfigError, PaymentConfig, Refusal, DEFAULT_DRAW_FEE_LIMIT,
    MAX_CONFIG_BYTES,
};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/config")
        .join(name)
}

fn valid_json() -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str(&std::fs::read_to_string(fixture(
        "valid.json",
    ))?)?)
}

fn refusal(field: &str, refusal: Refusal) -> ConfigError {
    ConfigError {
        field: field.to_owned(),
        refusal,
    }
}

fn refusal_named(name: &str) -> Refusal {
    match name {
        "missing" => Refusal::Missing,
        "invalid" => Refusal::Invalid,
        "placeholder" => Refusal::Placeholder,
        "unknown" => Refusal::Unknown,
        "key_material" => Refusal::KeyMaterial,
        other => panic!("unexpected refusal {other}"),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

#[test]
fn committed_configuration_is_accepted_with_every_field() -> Result<(), Box<dyn std::error::Error>>
{
    let config = x_websearch::load(&fixture("valid.json"))?;
    assert_eq!(config.listen, "127.0.0.1:8480".parse()?);
    assert_eq!(config.data_dir, PathBuf::from("/var/lib/x-websearch"));
    assert_eq!(
        config.seeds,
        [
            "https://paxeer.app/",
            "https://github.com/Sidiora-Labs/Paxeer-X-Network"
        ]
    );
    assert_eq!(config.crawl.pages_per_cycle, 1000);
    assert_eq!(config.crawl.pages_per_host, 100);
    assert_eq!(config.crawl.max_depth, 3);
    assert_eq!(config.crawl.politeness_delay().as_millis(), 1000);
    assert_eq!(config.fetch.connect_timeout().as_millis(), 3000);
    assert_eq!(config.fetch.total_timeout().as_millis(), 10_000);
    assert_eq!(config.fetch.max_body_bytes, 2_097_152);
    assert_eq!(config.fetch.max_redirects, 3);
    assert!(!config.fetch.allow_loopback);
    let symbols: Vec<AssetSymbol> = config.assets.iter().map(|asset| asset.symbol).collect();
    assert_eq!(symbols, AssetSymbol::ALL);
    assert_eq!(
        hex(&config.asset(AssetSymbol::Sid).asset_id),
        "5c1d0a93e4b7f2682c9e1f0473ad5b6e8f1027c3d49a5b6e7f80912a3b4c5d6e"
    );
    assert_eq!(config.asset(AssetSymbol::Sid).price, 3114);
    assert_eq!(config.asset(AssetSymbol::Pax).price, 1_000_000_000_000_000);
    assert_eq!(config.asset(AssetSymbol::Usdc).price, 1000);
    assert_eq!(
        hex(&config.asset(AssetSymbol::Usdl).asset_id),
        "8f403dc617ea259bcf204f37a6d08e91b2435f607cd8e9fa0b1c2d3e4f506172"
    );
    assert_eq!(config.gateway.endpoint, "http://127.0.0.1:8547/rpc");
    assert_eq!(
        hex(&config.gateway.sequencer.public_key),
        "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a"
    );
    assert_eq!(
        hex(&config.gateway.sequencer.sequencer_id),
        "2a7d1c5e9b3f4a6d8c0e2f4a6b8d0c2e4f6a8b0d2c4e6f8a0b2d4c6e8f0a2b4d"
    );
    assert_eq!(config.evm.endpoint, "http://127.0.0.1:8545");
    assert_eq!(config.evm.chain_id, 1337);
    assert_eq!(config.evm.confirmations, 12);
    assert_eq!(config.kernel_network_id, 1);
    assert_eq!(
        config.peers,
        ["http://127.0.0.1:8481", "http://127.0.0.1:8482"]
    );
    assert_eq!(Config::load(&fixture("valid.json")), Ok(config));
    Ok(())
}

#[test]
fn loopback_seeds_are_accepted_only_with_allow_loopback() -> Result<(), Box<dyn std::error::Error>>
{
    let config = x_websearch::load(&fixture("loopback.json"))?;
    assert!(config.fetch.allow_loopback);
    assert_eq!(
        config.seeds,
        ["http://127.0.0.1:9000/", "http://localhost:9001/index.html"]
    );
    assert!(config.peers.is_empty());
    let mut value: Value =
        serde_json::from_str(&std::fs::read_to_string(fixture("loopback.json"))?)?;
    value["fetch"]["allow_loopback"] = Value::Bool(false);
    assert_eq!(
        Config::parse(&value.to_string()).err(),
        Some(refusal("seeds", Refusal::Invalid))
    );
    Ok(())
}

#[test]
fn https_peers_and_endpoints_are_accepted() -> Result<(), Box<dyn std::error::Error>> {
    let mut value = valid_json()?;
    value["peers"] = serde_json::json!(["https://paxeer.app", "https://[::1]:8481"]);
    value["evm"]["endpoint"] = serde_json::json!("https://paxeer.app/evm");
    let config = Config::parse(&value.to_string())?;
    assert_eq!(config.peers, ["https://paxeer.app", "https://[::1]:8481"]);
    assert_eq!(config.evm.endpoint, "https://paxeer.app/evm");
    Ok(())
}

#[test]
fn committed_refusal_files_name_the_field() {
    for (name, field, expected) in [
        ("placeholders.json", "listen", Refusal::Placeholder),
        ("unknown-field.json", "cache_size", Refusal::Unknown),
        (
            "key-material.json",
            "gateway.receiver_private_key",
            Refusal::KeyMaterial,
        ),
        ("extra-asset.json", "assets.USDT", Refusal::Unknown),
        ("missing-asset.json", "assets.USDL", Refusal::Missing),
        ("not-json.json", "config", Refusal::Syntax),
    ] {
        assert_eq!(
            x_websearch::load(&fixture(name)).err(),
            Some(refusal(field, expected)),
            "{name}"
        );
    }
}

#[test]
fn committed_placeholder_example_refuses_every_placeholder_in_turn(
) -> Result<(), Box<dyn std::error::Error>> {
    let mut value: Value =
        serde_json::from_str(&std::fs::read_to_string(fixture("placeholders.json"))?)?;
    let valid = valid_json()?;
    for (pointer, field) in [
        ("/listen", "listen"),
        ("/data_dir", "data_dir"),
        ("/seeds", "seeds"),
        ("/crawl/pages_per_cycle", "crawl.pages_per_cycle"),
        ("/crawl/pages_per_host", "crawl.pages_per_host"),
        ("/crawl/max_depth", ""),
        ("/crawl/politeness_delay_ms", "crawl.politeness_delay_ms"),
        ("/assets/SID/asset_id", "assets.SID.asset_id"),
        ("/assets/SID/price", "assets.SID.price"),
        ("/assets/PAX/asset_id", "assets.PAX.asset_id"),
        ("/assets/PAX/price", "assets.PAX.price"),
        ("/assets/USDC/asset_id", "assets.USDC.asset_id"),
        ("/assets/USDC/price", "assets.USDC.price"),
        ("/assets/USDL/asset_id", "assets.USDL.asset_id"),
        ("/assets/USDL/price", "assets.USDL.price"),
        ("/gateway/endpoint", "gateway.endpoint"),
        ("/gateway/sequencer_id", "gateway.sequencer_id"),
        (
            "/gateway/sequencer_public_key",
            "gateway.sequencer_public_key",
        ),
        ("/evm/endpoint", "evm.endpoint"),
        ("/evm/chain_id", "evm.chain_id"),
        ("/evm/confirmations", "evm.confirmations"),
        ("/kernel_network_id", "kernel_network_id"),
    ] {
        if !field.is_empty() {
            assert_eq!(
                Config::parse(&value.to_string()).err(),
                Some(refusal(field, Refusal::Placeholder)),
                "{pointer}"
            );
        }
        let replacement = valid.pointer(pointer).cloned().ok_or(pointer)?;
        *value.pointer_mut(pointer).ok_or(pointer)? = replacement;
    }
    let config = Config::parse(&value.to_string())?;
    assert!(config.peers.is_empty());
    Ok(())
}

fn leaf_paths(value: &Value, pointer: &str, field: &str, into: &mut Vec<(String, String)>) {
    if let Value::Object(map) = value {
        for (name, child) in map {
            let child_pointer = format!("{pointer}/{name}");
            let child_field = if field.is_empty() {
                name.clone()
            } else {
                format!("{field}.{name}")
            };
            into.push((child_pointer.clone(), child_field.clone()));
            leaf_paths(child, &child_pointer, &child_field, into);
        }
    }
}

#[test]
fn every_missing_or_null_field_is_named() -> Result<(), Box<dyn std::error::Error>> {
    let valid = valid_json()?;
    let mut paths = Vec::new();
    leaf_paths(&valid, "", "", &mut paths);
    assert_eq!(paths.len(), 37);
    for (pointer, field) in paths {
        let (parent, name) = pointer.rsplit_once('/').ok_or("pointer")?;
        let mut removed = valid.clone();
        let parent_value = if parent.is_empty() {
            &mut removed
        } else {
            removed.pointer_mut(parent).ok_or("parent")?
        };
        parent_value
            .as_object_mut()
            .ok_or("object")?
            .remove(name)
            .ok_or("field")?;
        assert_eq!(
            Config::parse(&removed.to_string()).err(),
            Some(refusal(&field, Refusal::Missing)),
            "removed {pointer}"
        );
        let mut nulled = valid.clone();
        *nulled.pointer_mut(&pointer).ok_or("field")? = Value::Null;
        assert_eq!(
            Config::parse(&nulled.to_string()).err(),
            Some(refusal(&field, Refusal::Missing)),
            "null {pointer}"
        );
    }
    Ok(())
}

#[test]
fn every_committed_refusal_case_names_its_field() -> Result<(), Box<dyn std::error::Error>> {
    let cases: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(fixture("refusals.json"))?)?;
    assert!(cases.len() >= 80);
    let valid = valid_json()?;
    for case in cases {
        let pointer = case["pointer"].as_str().ok_or("pointer")?;
        let field = case["field"].as_str().ok_or("field")?;
        let expected = refusal_named(case["refusal"].as_str().ok_or("refusal")?);
        let (parent, name) = pointer.rsplit_once('/').ok_or("pointer")?;
        let mut value = valid.clone();
        let parent_value = if parent.is_empty() {
            &mut value
        } else {
            value.pointer_mut(parent).ok_or("parent")?
        };
        parent_value
            .as_object_mut()
            .ok_or("object")?
            .insert(name.to_owned(), case["value"].clone());
        assert_eq!(
            Config::parse(&value.to_string()).err(),
            Some(refusal(field, expected)),
            "{pointer} = {}",
            case["value"]
        );
    }
    Ok(())
}

#[test]
fn key_material_is_refused_without_echoing_it() -> Result<(), Box<dyn std::error::Error>> {
    let pem = format!("-----BEGIN {} KEY-----", "PRIVATE");
    let mut value = valid_json()?;
    value["gateway"]["endpoint"] = Value::String(pem);
    let error = Config::parse(&value.to_string()).err().ok_or("accepted")?;
    assert_eq!(
        error,
        ConfigError {
            field: "gateway.endpoint".into(),
            refusal: Refusal::KeyMaterial,
        }
    );
    assert_eq!(
        error.to_string(),
        "configuration refused: gateway.endpoint is key material, which is read only from key files"
    );
    let mut value = valid_json()?;
    let secret = "cd".repeat(32);
    value["data_dir"] = Value::String(secret.clone());
    let error = Config::parse(&value.to_string()).err().ok_or("accepted")?;
    assert_eq!(error.field, "data_dir");
    assert_eq!(error.refusal, Refusal::KeyMaterial);
    assert!(!error.to_string().contains(&secret));
    assert!(!format!("{error:?}").contains(&secret));
    Ok(())
}

#[test]
fn unreadable_oversized_and_non_json_files_are_refused() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::env::temp_dir().join(format!("x-websearch-config-{}", std::process::id()));
    std::fs::create_dir_all(&directory)?;
    let missing = directory.join("missing.json");
    assert_eq!(
        x_websearch::load(&missing).err(),
        Some(refusal("config", Refusal::Unreadable))
    );
    let oversized = directory.join("oversized.json");
    std::fs::write(&oversized, vec![b' '; MAX_CONFIG_BYTES + 1])?;
    let binary = directory.join("binary.json");
    std::fs::write(&binary, [0xff, 0xfe, b'{', b'}'])?;
    let array = directory.join("array.json");
    std::fs::write(&array, b"[]")?;
    let oversized_result = x_websearch::load(&oversized);
    let binary_result = x_websearch::load(&binary);
    let array_result = x_websearch::load(&array);
    let directory_result = x_websearch::load(&directory);
    std::fs::remove_dir_all(&directory)?;
    assert_eq!(
        oversized_result.err(),
        Some(refusal("config", Refusal::Oversized))
    );
    assert_eq!(
        binary_result.err(),
        Some(refusal("config", Refusal::Syntax))
    );
    assert_eq!(array_result.err(), Some(refusal("config", Refusal::Syntax)));
    assert_eq!(
        directory_result.err(),
        Some(refusal("config", Refusal::Unreadable))
    );
    Ok(())
}

#[test]
fn absent_payment_settings_keep_the_defaults() -> Result<(), Box<dyn std::error::Error>> {
    let config = x_websearch::load(&fixture("valid.json"))?;
    assert_eq!(config.payment, PaymentConfig::default());
    assert_eq!(config.payment.payer_did, None);
    assert_eq!(config.payment.draw_fee_limit, DEFAULT_DRAW_FEE_LIMIT);
    assert_eq!(DEFAULT_DRAW_FEE_LIMIT, 1_000_000_000_000);
    assert_eq!(config.payment.conformance_suite, None);
    let mut value = valid_json()?;
    value["payment"] = serde_json::json!({});
    assert_eq!(Config::parse(&value.to_string())?, config);
    Ok(())
}

#[test]
fn payment_settings_are_read_from_the_payment_object() -> Result<(), Box<dyn std::error::Error>> {
    let payer = format!("did:layerx:{}", "3c".repeat(32));
    let mut value = valid_json()?;
    value["payment"] = serde_json::json!({
        "payer_did": payer,
        "draw_fee_limit": "340282366920938463463374607431768211455",
        "conformance_suite": "/var/lib/x-websearch/conformance",
    });
    let config = Config::parse(&value.to_string())?;
    assert_eq!(config.payment.payer_did.as_deref(), Some(payer.as_str()));
    assert_eq!(config.payment.draw_fee_limit, u128::MAX);
    assert_eq!(
        config.payment.conformance_suite,
        Some(PathBuf::from("/var/lib/x-websearch/conformance"))
    );
    for (name, setting) in [
        ("payer_did", serde_json::json!("did:web:paxeer.app")),
        ("draw_fee_limit", serde_json::json!("1")),
        ("conformance_suite", serde_json::json!("/srv/suite")),
    ] {
        let mut value = valid_json()?;
        value["payment"] = serde_json::json!({ name: setting });
        let payment = Config::parse(&value.to_string())?.payment;
        let defaults = PaymentConfig::default();
        match name {
            "payer_did" => {
                assert_eq!(payment.payer_did.as_deref(), Some("did:web:paxeer.app"));
                assert_eq!(payment.draw_fee_limit, defaults.draw_fee_limit);
                assert_eq!(payment.conformance_suite, defaults.conformance_suite);
            }
            "draw_fee_limit" => {
                assert_eq!(payment.draw_fee_limit, 1);
                assert_eq!(payment.payer_did, defaults.payer_did);
                assert_eq!(payment.conformance_suite, defaults.conformance_suite);
            }
            _ => {
                assert_eq!(payment.conformance_suite, Some(PathBuf::from("/srv/suite")));
                assert_eq!(payment.payer_did, defaults.payer_did);
                assert_eq!(payment.draw_fee_limit, defaults.draw_fee_limit);
            }
        }
    }
    Ok(())
}

/// Each malformed `payment` setting, the field its refusal names and the
/// refusal.
fn malformed_payment_settings() -> Vec<(&'static str, Value, Refusal)> {
    let mut cases = Vec::new();
    cases.extend(malformed_object());
    cases.extend(malformed_payer_did());
    cases.extend(malformed_draw_fee_limit());
    cases.extend(malformed_conformance_suite());
    cases
}

fn malformed_object() -> Vec<(&'static str, Value, Refusal)> {
    vec![
        ("payment", serde_json::json!("payer"), Refusal::Invalid),
        ("payment", serde_json::json!(null), Refusal::Invalid),
        ("payment", serde_json::json!([]), Refusal::Invalid),
    ]
}

fn malformed_payer_did() -> Vec<(&'static str, Value, Refusal)> {
    vec![
        (
            "payment.payer_did",
            serde_json::json!(null),
            Refusal::Invalid,
        ),
        ("payment.payer_did", serde_json::json!(7), Refusal::Invalid),
        (
            "payment.payer_did",
            serde_json::json!("did:Upper"),
            Refusal::Invalid,
        ),
        (
            "payment.payer_did",
            serde_json::json!("layerx:payer"),
            Refusal::Invalid,
        ),
        (
            "payment.payer_did",
            serde_json::json!("did:layerx:"),
            Refusal::Invalid,
        ),
        (
            "payment.payer_did",
            serde_json::json!("did::layerx"),
            Refusal::Invalid,
        ),
        (
            "payment.payer_did",
            serde_json::json!("did:layerx:payer:asset:sid"),
            Refusal::Invalid,
        ),
        (
            "payment.payer_did",
            serde_json::json!(format!("did:{}", "a".repeat(252))),
            Refusal::Invalid,
        ),
        (
            "payment.payer_did",
            serde_json::json!("<payer did>"),
            Refusal::Placeholder,
        ),
        (
            "payment.payer_did",
            serde_json::json!(""),
            Refusal::Placeholder,
        ),
    ]
}

fn malformed_draw_fee_limit() -> Vec<(&'static str, Value, Refusal)> {
    vec![
        (
            "payment.draw_fee_limit",
            serde_json::json!(null),
            Refusal::Invalid,
        ),
        (
            "payment.draw_fee_limit",
            serde_json::json!(1000),
            Refusal::Invalid,
        ),
        (
            "payment.draw_fee_limit",
            serde_json::json!("0"),
            Refusal::Invalid,
        ),
        (
            "payment.draw_fee_limit",
            serde_json::json!("01"),
            Refusal::Invalid,
        ),
        (
            "payment.draw_fee_limit",
            serde_json::json!("+1"),
            Refusal::Invalid,
        ),
        (
            "payment.draw_fee_limit",
            serde_json::json!("-1"),
            Refusal::Invalid,
        ),
        (
            "payment.draw_fee_limit",
            serde_json::json!("1e12"),
            Refusal::Invalid,
        ),
        (
            "payment.draw_fee_limit",
            serde_json::json!("1 000"),
            Refusal::Invalid,
        ),
        (
            "payment.draw_fee_limit",
            serde_json::json!("340282366920938463463374607431768211456"),
            Refusal::Invalid,
        ),
        (
            "payment.draw_fee_limit",
            serde_json::json!("replace_me"),
            Refusal::Placeholder,
        ),
    ]
}

fn malformed_conformance_suite() -> Vec<(&'static str, Value, Refusal)> {
    vec![
        (
            "payment.conformance_suite",
            serde_json::json!(null),
            Refusal::Invalid,
        ),
        (
            "payment.conformance_suite",
            serde_json::json!(true),
            Refusal::Invalid,
        ),
        (
            "payment.conformance_suite",
            serde_json::json!("tests/fixtures/gateway"),
            Refusal::Invalid,
        ),
        (
            "payment.conformance_suite",
            serde_json::json!("/"),
            Refusal::Invalid,
        ),
        (
            "payment.conformance_suite",
            serde_json::json!("/srv/../etc"),
            Refusal::Invalid,
        ),
        (
            "payment.conformance_suite",
            serde_json::json!("/srv/su\u{7}ite"),
            Refusal::Invalid,
        ),
        (
            "payment.conformance_suite",
            serde_json::json!(format!("/{}", "s".repeat(4_096))),
            Refusal::Invalid,
        ),
        (
            "payment.conformance_suite",
            serde_json::json!("/path/to/suite"),
            Refusal::Placeholder,
        ),
        (
            "payment.conformance_suite",
            serde_json::json!(" "),
            Refusal::Placeholder,
        ),
    ]
}

#[test]
fn malformed_payment_settings_are_refused_naming_the_field(
) -> Result<(), Box<dyn std::error::Error>> {
    let cases = malformed_payment_settings();
    for (field, setting, expected) in cases {
        let mut value = valid_json()?;
        if let Some(name) = field.strip_prefix("payment.") {
            value["payment"] = serde_json::json!({ name: setting });
        } else {
            value["payment"] = setting.clone();
        }
        assert_eq!(
            Config::parse(&value.to_string()).err(),
            Some(refusal(field, expected)),
            "{field} = {setting}"
        );
    }
    let mut value = valid_json()?;
    value["payment"] = serde_json::json!({ "draw_fee_limit": "5", "payer": "did:layerx:a" });
    assert_eq!(
        Config::parse(&value.to_string()).err(),
        Some(refusal("payment.payer", Refusal::Unknown))
    );
    let mut value = valid_json()?;
    value["payment"] = serde_json::json!({ "payer_did": "did:layerx:payer", "receiver_key": "k" });
    assert_eq!(
        Config::parse(&value.to_string()).err(),
        Some(refusal("payment.receiver_key", Refusal::KeyMaterial))
    );
    Ok(())
}
