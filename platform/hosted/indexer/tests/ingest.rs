use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use layerx_indexer::abi::AbiRegistry;
use layerx_indexer::api;
use layerx_indexer::follow::{FollowPolicy, StepOutcome};
use layerx_indexer::layerx::LayerXIngester;
use layerx_indexer::paxeer::{AttributeEncoding, PaxeerIngester};
use layerx_indexer::store::{AssetRow, Store};
use layerx_indexer::transport::{Endpoint, Security};
use layerx_indexer::IndexError;
use layerx_types::receipt::{ACTIVITY_RECEIPT_FIELDS, LXP_RECEIPT_FIELDS};
use serde_json::{json, Value};

type Handler = dyn Fn(&str, &str, &[u8]) -> (u16, Value) + Send + Sync;

fn fixture(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn precompiles() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../precompiles")
}

fn read_request(stream: &mut TcpStream) -> Option<(String, String, Vec<u8>)> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        let count = stream.read(&mut chunk).ok()?;
        if count == 0 {
            return None;
        }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
    };
    let head = String::from_utf8(bytes[..header_end].to_vec()).ok()?;
    let mut lines = head.split("\r\n");
    let mut start = lines.next()?.split_whitespace();
    let method = start.next()?.to_owned();
    let target = start.next()?.to_owned();
    let length = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while bytes.len() < header_end + length {
        let count = stream.read(&mut chunk).ok()?;
        if count == 0 {
            return None;
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    Some((
        method,
        target,
        bytes[header_end..header_end + length].to_vec(),
    ))
}

fn serve_fixture(handler: Arc<Handler>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("bind: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("address: {error}"));
    thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else {
                continue;
            };
            let handler = Arc::clone(&handler);
            thread::spawn(move || {
                let Some((method, target, body)) = read_request(&mut stream) else {
                    return;
                };
                let (status, value) = handler(&method, &target, &body);
                let body = value.to_string();
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.flush();
            });
        }
    });
    format!("http://{address}")
}

fn endpoint(url: &str) -> Endpoint {
    Endpoint::parse(
        url,
        Security::Plaintext {
            allow_remote: false,
        },
        Duration::from_secs(5),
    )
    .unwrap_or_else(|error| panic!("{error}"))
}

fn relay_server(state: Arc<Mutex<&'static str>>) -> String {
    let relay = fixture("relay_archive_batches.json");
    serve_fixture(Arc::new(move |method, target, _| {
        let chosen = *state.lock().unwrap_or_else(|error| panic!("{error}"));
        let view = &relay[chosen];
        match (method, target) {
            ("GET", "/v1/sync/network") => (200, relay["network"].clone()),
            ("GET", "/v1/sync/head") => (200, view["head"].clone()),
            ("GET", path) => path
                .strip_prefix("/v1/history/batches/")
                .and_then(|number| view["batches"].get(number))
                .map_or_else(
                    || {
                        (
                            404,
                            json!({"error": {"code": "batch_not_found", "retry": "never"}}),
                        )
                    },
                    |batch| (200, batch.clone()),
                ),
            _ => (405, json!({"error": {"code": "method_not_allowed"}})),
        }
    }))
}

fn tx_search_range(query: &str) -> (u64, u64) {
    let mut bounds = query.split(" AND ").map(|part| {
        part.split_once('=')
            .and_then(|(_, value)| value.parse::<u64>().ok())
            .unwrap_or_else(|| panic!("tx_search query {query}"))
    });
    let low = bounds.next().unwrap_or_else(|| panic!("low bound"));
    let high = bounds.next().unwrap_or_else(|| panic!("high bound"));
    (low, high)
}

fn paxeer_server(state: Arc<Mutex<&'static str>>) -> String {
    let chain = fixture("paxeer_blocks.json");
    let search = fixture("tx_search.json");
    serve_fixture(Arc::new(move |method, _, body| {
        assert_eq!(method, "POST");
        let request: Value = serde_json::from_slice(body).unwrap_or_else(|error| panic!("{error}"));
        let chosen = *state.lock().unwrap_or_else(|error| panic!("{error}"));
        let view = &chain[chosen];
        let params = &request["params"];
        let result = match request["method"].as_str().unwrap_or_default() {
            "eth_chainId" => chain["chain_id"].clone(),
            "eth_blockNumber" => view["head"].clone(),
            "eth_getBlockByNumber" => {
                let quantity = params[0].as_str().unwrap_or_default();
                let height =
                    u64::from_str_radix(quantity.trim_start_matches("0x"), 16).unwrap_or(u64::MAX);
                assert_eq!(params[1], Value::Bool(true));
                view["blocks"]
                    .get(height.to_string())
                    .cloned()
                    .unwrap_or(Value::Null)
            }
            "eth_getTransactionReceipt" => {
                let hash = params[0].as_str().unwrap_or_default();
                chain["receipts"].get(hash).cloned().unwrap_or(Value::Null)
            }
            "tx_search" => {
                let (low, high) = tx_search_range(params["query"].as_str().unwrap_or_default());
                let page: usize = params["page"]
                    .as_str()
                    .and_then(|page| page.parse().ok())
                    .unwrap_or(1);
                let per_page: usize = params["per_page"]
                    .as_str()
                    .and_then(|size| size.parse().ok())
                    .unwrap_or(30);
                let fork_heights: Vec<u64> = if chosen == "reorg" {
                    vec![3]
                } else {
                    Vec::new()
                };
                let matching: Vec<Value> = search["result"]["txs"]
                    .as_array()
                    .map_or(&[][..], Vec::as_slice)
                    .iter()
                    .filter(|tx| {
                        let height: u64 = tx["height"]
                            .as_str()
                            .and_then(|text| text.parse().ok())
                            .unwrap_or(0);
                        (low..=high).contains(&height) && !fork_heights.contains(&height)
                    })
                    .cloned()
                    .collect();
                let total = matching.len();
                let items: Vec<Value> = matching
                    .into_iter()
                    .skip((page - 1) * per_page)
                    .take(per_page)
                    .collect();
                json!({ "txs": items, "total_count": total.to_string() })
            }
            other => panic!("unexpected JSON-RPC method {other}"),
        };
        (
            200,
            json!({ "jsonrpc": "2.0", "id": request["id"], "result": result }),
        )
    }))
}

fn set(state: &Arc<Mutex<&'static str>>, view: &'static str) {
    *state.lock().unwrap_or_else(|error| panic!("{error}")) = view;
}

fn step_ok<F: Fn() -> Result<StepOutcome, IndexError>>(step: F) -> StepOutcome {
    step().unwrap_or_else(|error| panic!("{error}"))
}

fn by(rows: &[Value], field: &str, value: &str) -> Vec<Value> {
    rows.iter()
        .filter(|row| row[field] == value)
        .cloned()
        .collect()
}

const SENDER: &str = "3aa29bcf27f39c8bcfe4017be09686802ed23412631c141902d715255ec8acba";
const RECEIVER: &str = "29b27231b5c9eb1fee8193f1334c6a44c34f68f20c38f905001fca9f8ce6b553";
const SEND_ASSET: &str = "cd4caf041d0f03a1f172d10218e890ede6e1f93dfa8702c43f25e56c4c2fcdb5";
const SEND_ACTIVITY: &str = "a1f5977a9aa9cb2d167a315c9a8769083dfa9a68225c757e7e96beef275662e4";

#[test]
fn layerx_relay_batches_decode_receipts_and_roll_back_reorgs() {
    let state = Arc::new(Mutex::new("canonical"));
    let relay = relay_server(Arc::clone(&state));
    let store = Store::open_in_memory().unwrap_or_else(|error| panic!("{error}"));
    let policy = FollowPolicy {
        finality_depth: 1,
        max_units_per_step: 16,
    };
    let ingester = LayerXIngester::new(endpoint(&relay), policy, None);

    assert_eq!(
        step_ok(|| ingester.step(&store)),
        StepOutcome::Advanced {
            units: 3,
            position: 3
        }
    );
    assert_eq!(step_ok(|| ingester.step(&store)), StepOutcome::Idle);

    let transfers = store
        .transfers("layerx")
        .unwrap_or_else(|error| panic!("{error}"));
    let send = by(&transfers, "tx_id", SEND_ACTIVITY);
    assert_eq!(send.len(), 2);
    let out = &by(&send, "direction", "out")[0];
    assert_eq!(out["kind"], "lxp_transfer");
    assert_eq!(out["account"], SENDER);
    assert_eq!(out["counterparty"], RECEIVER);
    assert_eq!(out["asset"], SEND_ASSET);
    assert_eq!(out["amount"], "1");
    assert_eq!(out["height_or_seq"], "5");
    let inbound = &by(&send, "direction", "in")[0];
    assert_eq!(inbound["account"], RECEIVER);
    assert_eq!(inbound["counterparty"], SENDER);

    let lxp = &out["decoded"]["lxp_receipt"];
    let activity = &out["decoded"]["activity_receipt"];
    assert_eq!(
        lxp.as_object().map(serde_json::Map::len),
        Some(LXP_RECEIPT_FIELDS.len())
    );
    assert_eq!(
        activity.as_object().map(serde_json::Map::len),
        Some(ACTIVITY_RECEIPT_FIELDS.len())
    );
    assert_eq!(LXP_RECEIPT_FIELDS.len(), 21);
    assert_eq!(ACTIVITY_RECEIPT_FIELDS.len(), 15);
    for field in LXP_RECEIPT_FIELDS {
        assert!(lxp.get(field).is_some(), "lxp field {field} missing");
    }
    for field in ACTIVITY_RECEIPT_FIELDS {
        assert!(
            activity.get(field).is_some(),
            "activity field {field} missing"
        );
    }
    assert_eq!(out["decoded"]["supply"]["before"], "0");

    let events = store
        .events("layerx")
        .unwrap_or_else(|error| panic!("{error}"));
    let activities = by(&events, "name", "activity");
    assert_eq!(activities.len(), 3);
    let effects = by(&events, "source", "layerx-effect");
    let names: Vec<&str> = effects
        .iter()
        .filter_map(|event| event["name"].as_str())
        .collect();
    assert!(names.contains(&"module8/event1"));
    assert!(names.contains(&"module8/event2"));
    assert!(names.contains(&"module9/event7"));
    let emulator = by(
        &activities,
        "tx_id",
        "a790ebe72b3431e6bd06d85a26db8809c73b2f713102b47923208a76d80e0eab",
    );
    assert!(!emulator[0]["decoded"]["program_outcome"].is_null());
    assert_eq!(by(&events, "source", "layerx-maintenance").len(), 1);
    assert!(store
        .account_known("layerx", RECEIVER)
        .unwrap_or_else(|error| panic!("{error}")));

    let asset = store
        .asset(SEND_ASSET)
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!("asset indexed"));
    assert_eq!(asset["transfer_legs"], "2");

    let cursor = store
        .cursor("layerx")
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!("cursor"));
    assert_eq!(cursor.position, 3);
    assert_eq!(
        cursor.hash,
        "7f98bf6fc1f8cff652cb9ec527c1c8484cce46ea50a4222449716aa5bbf0bd36"
    );
    assert_eq!(cursor.finalized_position, Some(2));

    set(&state, "reorg");
    assert_eq!(
        step_ok(|| ingester.step(&store)),
        StepOutcome::RolledBack { fork: Some(2) }
    );
    let transfers = store
        .transfers("layerx")
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(by(&transfers, "tx_id", SEND_ACTIVITY).is_empty());
    let events = store
        .events("layerx")
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(by(&events, "tx_id", SEND_ACTIVITY).is_empty());
    assert_eq!(by(&events, "name", "activity").len(), 2);
    assert_eq!(
        store
            .cursor("layerx")
            .unwrap_or_else(|error| panic!("{error}"))
            .map(|cursor| cursor.position),
        Some(2)
    );
    assert_eq!(
        step_ok(|| ingester.step(&store)),
        StepOutcome::Advanced {
            units: 2,
            position: 4
        }
    );

    set(&state, "deep_reorg");
    match ingester.step(&store) {
        Err(IndexError::ReorgBeyondFinality { source, .. }) => assert_eq!(source, "layerx"),
        other => panic!("a reorg below finality must stop indexing, got {other:?}"),
    }
    assert_eq!(
        store
            .cursor("layerx")
            .unwrap_or_else(|error| panic!("{error}"))
            .map(|cursor| cursor.position),
        Some(4)
    );
}

#[test]
fn layerx_rejects_a_receipt_filed_under_another_batch() {
    let relay = fixture("relay_archive_batches.json");
    let mut batch = relay["canonical"]["batches"]["3"].clone();
    batch["batch_id"] = json!("00".repeat(32));
    match layerx_indexer::layerx::decode_batch(&batch) {
        Err(IndexError::Integrity(_)) => {}
        other => panic!("expected an integrity refusal, got {other:?}"),
    }
}

fn address_of(label: &str) -> String {
    let chain = fixture("paxeer_blocks.json");
    let block = &chain["canonical"]["blocks"]["2"];
    match label {
        "alice" => block["transactions"][0]["from"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        "bob" => block["transactions"][0]["to"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        "carol" => block["transactions"][3]["to"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        _ => panic!("unknown label"),
    }
}

#[test]
fn paxeer_blocks_logs_and_tx_search_decode_paginate_and_roll_back() {
    let alice = address_of("alice");
    let bob = address_of("bob");
    let carol = address_of("carol");
    let chain = fixture("paxeer_blocks.json");
    let pointer = chain["pointer"]["address"]
        .as_str()
        .unwrap_or_default()
        .to_owned();

    let state = Arc::new(Mutex::new("canonical"));
    let rpc = paxeer_server(Arc::clone(&state));
    let store = Store::open_in_memory().unwrap_or_else(|error| panic!("{error}"));
    store
        .register_assets(&[AssetRow {
            asset: format!("evm:{pointer}"),
            chain: "paxeer".to_owned(),
            kind: "pointer".to_owned(),
            address: Some(pointer.clone()),
            denom: Some("upax".to_owned()),
            metadata: json!({}),
        }])
        .unwrap_or_else(|error| panic!("{error}"));
    let registry = AbiRegistry::load_dir(&precompiles()).unwrap_or_else(|error| panic!("{error}"));
    let ingester = PaxeerIngester::new(
        endpoint(&rpc),
        Some(endpoint(&rpc)),
        registry,
        FollowPolicy {
            finality_depth: 2,
            max_units_per_step: 16,
        },
        1,
        Some(0xe5),
        AttributeEncoding::Base64,
    );

    assert_eq!(
        step_ok(|| ingester.step(&store)),
        StepOutcome::Advanced {
            units: 3,
            position: 3
        }
    );

    let transfers = store
        .transfers("paxeer")
        .unwrap_or_else(|error| panic!("{error}"));
    let native = by(&transfers, "kind", "native_transfer");
    assert_eq!(native.len(), 2);
    assert_eq!(
        by(&native, "direction", "out")[0]["amount"],
        "1000000000000000000"
    );
    assert!(by(&transfers, "account", &carol)
        .iter()
        .all(|row| row["kind"] != "native_transfer"));

    let erc20 = by(&transfers, "kind", "erc20_transfer");
    assert_eq!(erc20.len(), 2);
    let erc20_out = &by(&erc20, "direction", "out")[0];
    assert_eq!(erc20_out["account"], alice.as_str());
    assert_eq!(erc20_out["counterparty"], bob.as_str());
    assert_eq!(erc20_out["amount"], "1000000");
    assert_eq!(
        erc20_out["asset"],
        "evm:0x7b79995e5f793a07bc00c21412e50ecae098e7f9"
    );

    let pointer_legs = by(&transfers, "kind", "pointer_transfer");
    assert_eq!(pointer_legs.len(), 2);
    assert_eq!(
        by(&pointer_legs, "direction", "in")[0]["account"],
        carol.as_str()
    );
    assert_eq!(pointer_legs[0]["amount"], "42");

    let deposit = by(&transfers, "kind", "custody_deposit");
    assert_eq!(deposit.len(), 2);
    let deposit_in = &by(&deposit, "direction", "in")[0];
    assert_eq!(deposit_in["account"], RECEIVER);
    assert_eq!(deposit_in["asset"], SEND_ASSET);
    assert_eq!(deposit_in["amount"], "2500");
    assert_eq!(
        by(&deposit, "direction", "out")[0]["account"],
        alice.as_str()
    );

    let bank = by(&transfers, "kind", "bank_transfer");
    assert_eq!(bank.len(), 4);
    let bank_assets: Vec<&str> = bank
        .iter()
        .filter_map(|row| row["asset"].as_str())
        .collect();
    assert!(bank_assets.contains(&"denom:upax"));
    assert!(bank_assets.contains(&"denom:uatom"));
    assert!(bank
        .iter()
        .all(|row| row["amount"] != "99" && row["amount"] != "11"));
    let mint = by(&transfers, "kind", "tokenfactory_mint");
    assert_eq!(mint.len(), 1);
    assert_eq!(mint[0]["amount"], "5000");

    let events = store
        .events("paxeer")
        .unwrap_or_else(|error| panic!("{error}"));
    let custody = by(&events, "name", "CustodyDeposit");
    assert_eq!(custody.len(), 1);
    assert_eq!(custody[0]["contract"], "layerxcustody");
    assert_eq!(custody[0]["account"], alice.as_str());
    assert_eq!(custody[0]["decoded"]["args"]["nonce"], "7");
    assert_eq!(
        custody[0]["decoded"]["address"],
        "0x0000000000000000000000000000000000001013"
    );
    let checkpoint = by(&events, "name", "layerx_checkpoint_submitted");
    assert_eq!(checkpoint.len(), 1);
    assert_eq!(checkpoint[0]["decoded"]["attributes"]["batch_number"], "3");
    assert_eq!(by(&events, "name", "create_denom").len(), 1);

    let first = api::route(&store, "GET", &format!("/v1/history/{alice}?limit=2"));
    assert_eq!(first.status, 200);
    let first: Value = serde_json::from_str(&first.body).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(first["items"].as_array().map(Vec::len), Some(2));
    let next = first["next_cursor"]
        .as_str()
        .unwrap_or_else(|| panic!("second page cursor"))
        .to_owned();
    let second = api::route(
        &store,
        "GET",
        &format!(
            "/v1/history/{}?limit=2&cursor={next}",
            alice.to_uppercase().replacen("0X", "0x", 1)
        ),
    );
    let second: Value =
        serde_json::from_str(&second.body).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(second["items"].as_array().map(Vec::len), Some(1));
    assert!(second["next_cursor"].is_null());
    let mut kinds: Vec<String> = first["items"]
        .as_array()
        .into_iter()
        .chain(second["items"].as_array())
        .flatten()
        .filter_map(|item| item["kind"].as_str().map(str::to_owned))
        .collect();
    kinds.sort();
    assert_eq!(
        kinds,
        ["custody_deposit", "erc20_transfer", "native_transfer"]
    );
    let ids: Vec<u64> = first["items"]
        .as_array()
        .into_iter()
        .chain(second["items"].as_array())
        .flatten()
        .filter_map(|item| item["id"].as_str().and_then(|id| id.parse().ok()))
        .collect();
    assert!(ids.windows(2).all(|pair| pair[0] > pair[1]));
    let filtered = api::route(
        &store,
        "GET",
        &format!("/v1/history/{alice}?kind=erc20_transfer"),
    );
    let filtered: Value =
        serde_json::from_str(&filtered.body).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(filtered["items"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        api::route(&store, "GET", "/v1/history/x?limit=0").status,
        400
    );
    assert_eq!(
        api::route(&store, "GET", "/v1/history/x?bogus=1").status,
        400
    );
    assert_eq!(api::route(&store, "POST", "/healthz").status, 405);

    let assets = api::route(&store, "GET", "/v1/assets?limit=100");
    let assets: Value =
        serde_json::from_str(&assets.body).unwrap_or_else(|error| panic!("{error}"));
    let listed: Vec<&str> = assets["items"]
        .as_array()
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .filter_map(|asset| asset["asset"].as_str())
        .collect();
    let denom = "denom:factory/pax1qyqszqgpqyqszqgpqyqszqgpqyqszqgp8apuk5/ulx";
    assert!(listed.contains(&denom));
    assert!(listed.contains(&"evm:native"));
    let one = api::route(
        &store,
        "GET",
        &format!("/v1/assets/{}", denom.replace('/', "%2F")),
    );
    assert_eq!(one.status, 200);
    let one: Value = serde_json::from_str(&one.body).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(one["asset"]["kind"], "tokenfactory");
    assert_eq!(one["asset"]["transfer_legs"], "1");
    assert_eq!(
        api::route(&store, "GET", "/v1/assets/denom:none").status,
        404
    );

    let paged = api::route(&store, "GET", "/v1/assets?limit=1");
    let paged: Value = serde_json::from_str(&paged.body).unwrap_or_else(|error| panic!("{error}"));
    let cursor = paged["next_cursor"]
        .as_str()
        .unwrap_or_else(|| panic!("asset cursor"))
        .to_owned();
    let rest = api::route(
        &store,
        "GET",
        &format!("/v1/assets?limit=500&cursor={cursor}"),
    );
    let rest: Value = serde_json::from_str(&rest.body).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        rest["items"].as_array().map(Vec::len).unwrap_or_default() + 1,
        listed.len()
    );

    set(&state, "reorg");
    assert_eq!(
        step_ok(|| ingester.step(&store)),
        StepOutcome::RolledBack { fork: Some(2) }
    );
    let transfers = store
        .transfers("paxeer")
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(by(&transfers, "kind", "pointer_transfer").is_empty());
    assert!(by(&transfers, "kind", "tokenfactory_mint").is_empty());
    assert_eq!(by(&transfers, "kind", "erc20_transfer").len(), 2);
    let events = store
        .events("paxeer")
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(by(&events, "name", "create_denom").is_empty());
    assert_eq!(
        step_ok(|| ingester.step(&store)),
        StepOutcome::Advanced {
            units: 1,
            position: 3
        }
    );
    let cursor = store
        .cursor("paxeer")
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!("cursor"));
    assert_eq!(
        cursor.hash,
        chain["reorg"]["blocks"]["3"]["hash"]
            .as_str()
            .unwrap_or_default()
    );
    let carol_history = store
        .history(&carol, None, 10, None)
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(carol_history.items.is_empty());
}

#[test]
fn a_dropped_in_abi_is_decoded_without_code_changes() {
    let directory = std::env::temp_dir().join(format!("layerx-indexer-abi-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(directory.join("launchpad")).unwrap_or_else(|error| panic!("{error}"));
    std::fs::write(
        directory.join("launchpad/abi.json"),
        json!([{
            "type": "event",
            "name": "TokenLaunched",
            "anonymous": false,
            "inputs": [
                {"name": "token", "type": "address", "indexed": true},
                {"name": "supply", "type": "uint256", "indexed": false}
            ]
        }])
        .to_string(),
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let registry = AbiRegistry::load_dir(&directory).unwrap_or_else(|error| panic!("{error}"));
    let mut emitter = [0_u8; 20];
    emitter[18] = 0x10;
    emitter[19] = 0x17;
    let topic = layerx_indexer::abi::keccak(b"TokenLaunched(address,uint256)");
    let mut token = [0_u8; 32];
    token[31] = 0x42;
    let mut data = [0_u8; 32];
    data[31] = 9;
    let (_, decoded) = registry
        .decode_log(emitter, &[topic, token], &data)
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!("dropped-in event must decode"));
    assert_eq!(decoded.contract, "launchpad");
    assert_eq!(decoded.arg("supply"), Some(&json!("9")));
    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn the_api_answers_over_a_real_socket() {
    let store = Arc::new(Store::open_in_memory().unwrap_or_else(|error| panic!("{error}")));
    let listener = api::bind(
        "127.0.0.1:0"
            .parse()
            .unwrap_or_else(|error| panic!("{error}")),
        false,
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("{error}"));
    let served = Arc::clone(&store);
    thread::spawn(move || api::serve(&listener, &served, None));
    let client = endpoint(&format!("http://{address}"));
    let (status, body) = client
        .get("/healthz")
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(status, 200);
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap_or_default()["status"],
        "ok"
    );
    let page = client
        .get_json("/v1/history/0xabc?limit=5")
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        page,
        json!({"version": 1, "items": [], "next_cursor": null})
    );
    assert!(api::bind(
        "0.0.0.0:0"
            .parse()
            .unwrap_or_else(|error| panic!("{error}")),
        false
    )
    .is_err());
}
