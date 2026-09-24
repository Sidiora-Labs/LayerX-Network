use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use layerx_indexer::abi::AbiRegistry;
use layerx_indexer::backfill::Backfill;
use layerx_indexer::blockscout::{rpc_blocks, BlockscoutRange};
use layerx_indexer::follow::{FollowPolicy, StepOutcome};
use layerx_indexer::paxeer::{AttributeEncoding, PaxeerIngester, TX_INDEX_DISABLED};
use layerx_indexer::paxscan::{PaxscanDatabase, PaxscanTls};
use layerx_indexer::store::{AssetRow, Store};
use layerx_indexer::transport::{Endpoint, Security};
use layerx_indexer::IndexError;
use serde_json::{json, Value};

type Handler = dyn Fn(&[u8]) -> Value + Send + Sync;

fn fixture_at(path: &Path) -> Value {
    let text =
        std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn fixture(name: &str) -> Value {
    fixture_at(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(name),
    )
}

fn blockscout() -> BlockscoutRange {
    let value = fixture_at(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join("backfill_blockscout.json"),
    );
    serde_json::from_value(value).unwrap_or_else(|error| panic!("blockscout fixture: {error}"))
}

fn within(range: &BlockscoutRange, from: u64, to: u64, absent: &[u64]) -> BlockscoutRange {
    let keep = |height: u64| (from..=to).contains(&height) && !absent.contains(&height);
    BlockscoutRange {
        blocks: range
            .blocks
            .iter()
            .filter(|row| keep(row.number))
            .cloned()
            .collect(),
        transactions: range
            .transactions
            .iter()
            .filter(|row| keep(row.block_number))
            .cloned()
            .collect(),
        logs: range
            .logs
            .iter()
            .filter(|row| keep(row.block_number))
            .cloned()
            .collect(),
        token_transfers: range
            .token_transfers
            .iter()
            .filter(|row| keep(row.block_number))
            .cloned()
            .collect(),
        internal_transactions: range
            .internal_transactions
            .iter()
            .filter(|row| keep(row.block_number))
            .cloned()
            .collect(),
        tokens: range.tokens.clone(),
    }
}

fn precompiles() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../precompiles")
}

fn read_request(stream: &mut TcpStream) -> Option<Vec<u8>> {
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
    let length = head
        .split("\r\n")
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
    Some(bytes[header_end..header_end + length].to_vec())
}

fn serve(handler: Arc<Handler>) -> String {
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
                let Some(body) = read_request(&mut stream) else {
                    return;
                };
                let body = handler(&body).to_string();
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
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

/// How the recorded node answers `tx_search`.
#[derive(Clone, Copy)]
enum TxIndex {
    On,
    Off,
    Broken,
}

/// A JSON-RPC node answering from the recorded `paxeer_blocks.json` and
/// `tx_search.json` fixtures, serving hash-only blocks for verification.
fn node(tx_index: TxIndex) -> String {
    let chain = fixture("paxeer_blocks.json");
    let search = fixture("tx_search.json");
    serve(Arc::new(move |body| {
        let request: Value = serde_json::from_slice(body).unwrap_or_else(|error| panic!("{error}"));
        let params = &request["params"];
        let view = &chain["canonical"];
        let answer =
            |result: Value| json!({"jsonrpc": "2.0", "id": request["id"], "result": result});
        match request["method"].as_str().unwrap_or_default() {
            "eth_chainId" => answer(chain["chain_id"].clone()),
            "eth_blockNumber" => answer(view["head"].clone()),
            "eth_getBlockByNumber" => {
                let quantity = params[0].as_str().unwrap_or_default();
                let height =
                    u64::from_str_radix(quantity.trim_start_matches("0x"), 16).unwrap_or(u64::MAX);
                let mut block = view["blocks"]
                    .get(height.to_string())
                    .cloned()
                    .unwrap_or(Value::Null);
                if params[1] == Value::Bool(false) {
                    if let Some(transactions) =
                        block.get_mut("transactions").and_then(Value::as_array_mut)
                    {
                        for transaction in transactions.iter_mut() {
                            *transaction = transaction["hash"].clone();
                        }
                    }
                }
                answer(block)
            }
            "eth_getTransactionReceipt" => {
                let hash = params[0].as_str().unwrap_or_default();
                answer(chain["receipts"].get(hash).cloned().unwrap_or(Value::Null))
            }
            "tx_search" => match tx_index {
                TxIndex::Off => json!({
                    "jsonrpc": "2.0",
                    "id": request["id"],
                    "error": {"code": -32603, "message": "Internal error", "data": TX_INDEX_DISABLED}
                }),
                TxIndex::Broken => json!({
                    "jsonrpc": "2.0",
                    "id": request["id"],
                    "error": {"code": -32603, "message": "Internal error", "data": "database is locked"}
                }),
                TxIndex::On => {
                    let (low, high) = tx_search_range(params["query"].as_str().unwrap_or_default());
                    let matching: Vec<Value> = search["result"]["txs"]
                        .as_array()
                        .map_or(&[][..], Vec::as_slice)
                        .iter()
                        .filter(|tx| {
                            let height: u64 = tx["height"]
                                .as_str()
                                .and_then(|text| text.parse().ok())
                                .unwrap_or(0);
                            (low..=high).contains(&height)
                        })
                        .cloned()
                        .collect();
                    let total = matching.len();
                    answer(json!({ "txs": matching, "total_count": total.to_string() }))
                }
            },
            other => panic!("unexpected JSON-RPC method {other}"),
        }
    }))
}

fn store() -> Store {
    let chain = fixture("paxeer_blocks.json");
    let pointer = chain["pointer"]["address"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let store = Store::open_in_memory().unwrap_or_else(|error| panic!("{error}"));
    store
        .register_assets(&[AssetRow {
            asset: format!("evm:{pointer}"),
            chain: "paxeer".to_owned(),
            kind: "pointer".to_owned(),
            address: Some(pointer),
            denom: Some("upax".to_owned()),
            metadata: json!({}),
        }])
        .unwrap_or_else(|error| panic!("{error}"));
    store
}

fn ingester(url: &str) -> PaxeerIngester {
    let registry = AbiRegistry::load_dir(&precompiles()).unwrap_or_else(|error| panic!("{error}"));
    PaxeerIngester::new(
        endpoint(url),
        Some(endpoint(url)),
        registry,
        FollowPolicy {
            finality_depth: 2,
            max_units_per_step: 16,
        },
        1,
        Some(0xe5),
        AttributeEncoding::Base64,
    )
}

fn assets(store: &Store) -> Vec<Value> {
    let page = store
        .assets(None, 500)
        .unwrap_or_else(|error| panic!("{error}"));
    page.items
        .into_iter()
        .map(|mut asset| {
            if let Some(object) = asset.as_object_mut() {
                object.remove("first_seen");
            }
            asset
        })
        .collect()
}

fn history(store: &Store) -> (Vec<Value>, Vec<Value>, Vec<Value>) {
    (
        store
            .transfers("paxeer")
            .unwrap_or_else(|error| panic!("{error}")),
        store
            .events("paxeer")
            .unwrap_or_else(|error| panic!("{error}")),
        assets(store),
    )
}

fn live_rows() -> (Vec<Value>, Vec<Value>, Vec<Value>) {
    let url = node(TxIndex::On);
    let live = ingester(&url);
    let store = store();
    assert_eq!(
        live.step(&store).unwrap_or_else(|error| panic!("{error}")),
        StepOutcome::Advanced {
            units: 3,
            position: 3
        }
    );
    history(&store)
}

fn kinds(rows: &[Value]) -> Vec<String> {
    let mut kinds: Vec<String> = rows
        .iter()
        .filter_map(|row| row["kind"].as_str().map(str::to_owned))
        .collect();
    kinds.sort();
    kinds.dedup();
    kinds
}

#[test]
fn backfill_blockscout_rows_convert_to_the_live_rpc_shapes() {
    let chain = fixture("paxeer_blocks.json");
    let converted = rpc_blocks(&blockscout()).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(converted.keys().copied().collect::<Vec<_>>(), vec![1, 2, 3]);
    for (height, block) in &converted {
        let live = &chain["canonical"]["blocks"][height.to_string()];
        for field in [
            "number",
            "hash",
            "parentHash",
            "timestamp",
            "miner",
            "gasUsed",
        ] {
            assert_eq!(block.block[field], live[field], "block {height} {field}");
        }
        let live_transactions = live["transactions"]
            .as_array()
            .map_or(&[][..], Vec::as_slice);
        let transactions = block.block["transactions"]
            .as_array()
            .map_or(&[][..], Vec::as_slice);
        assert_eq!(transactions.len(), live_transactions.len());
        for (ours, theirs) in transactions.iter().zip(live_transactions) {
            for field in [
                "hash",
                "blockHash",
                "blockNumber",
                "transactionIndex",
                "from",
                "to",
                "value",
                "input",
            ] {
                assert_eq!(ours[field], theirs[field], "block {height} tx {field}");
            }
            let hash = theirs["hash"].as_str().unwrap_or_default();
            let receipt = &block.receipts[hash];
            let live_receipt = &chain["receipts"][hash];
            for field in [
                "transactionHash",
                "transactionIndex",
                "blockHash",
                "blockNumber",
                "from",
                "to",
                "gasUsed",
                "contractAddress",
                "status",
                "logs",
            ] {
                assert_eq!(
                    receipt[field], live_receipt[field],
                    "receipt {hash} {field}"
                );
            }
        }
    }

    let mut orphan = blockscout();
    orphan.internal_transactions[0].transaction_index = 9;
    assert!(matches!(rpc_blocks(&orphan), Err(IndexError::Integrity(_))));
    let mut disagreeing = blockscout();
    disagreeing.token_transfers[0].amount = Some("999".to_owned());
    assert!(matches!(
        rpc_blocks(&disagreeing),
        Err(IndexError::Integrity(_))
    ));
}

#[test]
fn backfill_from_the_blockscout_fixture_writes_the_live_rows() {
    let live = live_rows();
    let url = node(TxIndex::On);
    let node = ingester(&url);
    let store = store();
    let rows = blockscout();
    let plan = Backfill::new(&node, 3, 10);
    let cutover = plan
        .run(&store, |from, to| Ok(within(&rows, from, to, &[])))
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(cutover, 3);
    let backfilled = history(&store);
    assert_eq!(backfilled, live);
    assert!(kinds(&backfilled.0).contains(&"erc20_transfer".to_owned()));
    assert!(kinds(&backfilled.0).contains(&"native_transfer".to_owned()));
    assert!(kinds(&backfilled.0).contains(&"pointer_transfer".to_owned()));
}

#[test]
fn backfill_fills_a_height_absent_from_paxscan_from_the_node() {
    let live = live_rows();
    let url = node(TxIndex::On);
    let node = ingester(&url);
    let store = store();
    let rows = within(&blockscout(), 1, 3, &[2]);
    assert!(rows.blocks.iter().all(|block| block.number != 2));
    let plan = Backfill::new(&node, 3, 10);
    let filled = plan
        .apply_range(&store, 1, 3, &rows)
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(filled, 1);
    let cursor = store
        .backfill_cursor("paxeer")
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!("backfill cursor"));
    assert_eq!(cursor.position, 3);
    assert!(store
        .cursor("paxeer")
        .unwrap_or_else(|error| panic!("{error}"))
        .is_none());
    store
        .finish_backfill("paxeer", 3, 2)
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(history(&store), live);
}

#[test]
fn backfill_stops_on_a_paxscan_hash_the_node_disagrees_with() {
    let url = node(TxIndex::On);
    let node = ingester(&url);
    let store = store();
    let mut rows = blockscout();
    rows.blocks[2].hash[31] ^= 1;
    let error = Backfill::new(&node, 3, 10)
        .apply_range(&store, 1, 3, &rows)
        .err()
        .unwrap_or_else(|| panic!("a hash mismatch must stop the backfill"));
    assert!(
        matches!(&error, IndexError::Integrity(message) if message.contains("paxscan block 3 hash")),
        "{error}"
    );
    assert!(store
        .backfill_cursor("paxeer")
        .unwrap_or_else(|error| panic!("{error}"))
        .is_none());
    assert!(store
        .transfers("paxeer")
        .unwrap_or_else(|error| panic!("{error}"))
        .is_empty());
}

#[test]
fn backfill_cutover_hands_live_ingestion_the_next_height() {
    let live = live_rows();
    let url = node(TxIndex::On);
    let node = ingester(&url);
    let store = store();
    let rows = blockscout();
    let chain = fixture("paxeer_blocks.json");
    let plan = Backfill::new(&node, 2, 1);
    let mut asked = Vec::new();
    let cutover = plan
        .run(&store, |from, to| {
            asked.push((from, to));
            Ok(within(&rows, from, to, &[]))
        })
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(cutover, 2);
    assert_eq!(asked, vec![(1, 1), (2, 2)]);
    let cursor = store
        .cursor("paxeer")
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!("live cursor"));
    assert_eq!(cursor.position, 2);
    assert_eq!(
        cursor.hash,
        chain["canonical"]["blocks"]["2"]["hash"]
            .as_str()
            .unwrap_or_default()
    );
    assert!(plan
        .next_height(&store)
        .unwrap_or_else(|error| panic!("{error}"))
        .is_none());
    assert_eq!(
        node.step(&store).unwrap_or_else(|error| panic!("{error}")),
        StepOutcome::Advanced {
            units: 1,
            position: 3
        }
    );
    assert_eq!(history(&store), live);
    let refused = plan.next_height(&store);
    assert!(
        matches!(&refused, Err(IndexError::Config(message)) if message.contains("past the cutover")),
        "{refused:?}"
    );
    let late = Backfill::new(&node, 2, 1).run(&store, |_, _| {
        panic!("a backfill behind the live cursor must not read paxscan")
    });
    assert!(late.is_err());
}

#[test]
fn backfill_era_node_with_tx_index_off_steps_evm_only() {
    let url = node(TxIndex::Off);
    let live = ingester(&url);
    let store = store();
    for _ in 0..2 {
        let outcome = live.step(&store).unwrap_or_else(|error| panic!("{error}"));
        assert!(matches!(
            outcome,
            StepOutcome::Advanced { .. } | StepOutcome::Idle
        ));
    }
    let cursor = store
        .cursor("paxeer")
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!("live cursor"));
    assert_eq!(cursor.position, 3);
    let transfers = store
        .transfers("paxeer")
        .unwrap_or_else(|error| panic!("{error}"));
    let seen = kinds(&transfers);
    for evm in ["erc20_transfer", "native_transfer", "pointer_transfer"] {
        assert!(seen.contains(&evm.to_owned()), "{seen:?}");
    }
    for cosmos in ["bank_transfer", "tokenfactory_mint"] {
        assert!(!seen.contains(&cosmos.to_owned()), "{seen:?}");
    }

    let broken = ingester(&node(TxIndex::Broken));
    let error = broken.step(&self::store());
    assert!(
        matches!(&error, Err(IndexError::Source(message)) if message.contains("tx_search")),
        "{error:?}"
    );
}

#[test]
#[ignore = "reads the live paxscan database; needs PAXSCAN_DATABASE_PUBLIC_URL"]
fn backfill_reads_the_live_paxscan_database() {
    let Ok(url) = std::env::var("PAXSCAN_DATABASE_PUBLIC_URL") else {
        return;
    };
    let tls = std::env::var("LAYERX_INDEXER_PAXSCAN_CERT_SHA256").map_or(
        PaxscanTls::Unauthenticated,
        |pin| {
            let bytes: Vec<u8> = (0..pin.len())
                .step_by(2)
                .filter_map(|index| u8::from_str_radix(pin.get(index..index + 2)?, 16).ok())
                .collect();
            PaxscanTls::PinnedLeaf(
                bytes
                    .try_into()
                    .unwrap_or_else(|_| panic!("the certificate pin must be 32 bytes")),
            )
        },
    );
    let mut database =
        PaxscanDatabase::connect(&url, &tls).unwrap_or_else(|error| panic!("{error}"));
    let rows = database
        .range(1_000_000, 1_000_099)
        .unwrap_or_else(|error| panic!("{error}"));
    let converted = rpc_blocks(&rows).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(converted.len(), rows.blocks.len());
    assert!(converted
        .keys()
        .all(|height| (1_000_000..=1_000_099).contains(height)));
}
