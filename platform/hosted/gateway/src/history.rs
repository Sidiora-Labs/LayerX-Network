//! Account history on the single network endpoint: `lx_getHistory`,
//! `px_getHistory` and `px_getUnifiedHistory`, answered from the
//! `layerx-indexer` read API.
//!
//! Every answer is the indexer's own rows, newest first in the indexer's
//! commit order, each joined with its asset's indexer record plus the
//! symbol and decimals the `LayerX` asset registry declares for it. Cursors
//! are the indexer's opaque row cursors, passed through unchanged; because
//! both chains share one row sequence, a unified page is the exact merge of
//! every side's page at the same cursor.

use super::{public_reads, Config, Endpoint};
use layerx_platform_gateway::evm;
use layerx_platform_gateway::http::{self, Client, UpstreamResponse};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::net::SocketAddr;

/// Rows per page when the caller names no limit.
pub(super) const DEFAULT_LIMIT: u64 = 50;
/// The largest page the gateway asks the indexer for.
pub(super) const MAXIMUM_LIMIT: u64 = 100;
const MAXIMUM_CURSOR: usize = 32;
const MAXIMUM_KIND: usize = 64;

/// Where the indexer read API lives.
pub(super) enum Indexer {
    /// A TLS endpoint verified with the gateway's component trust roots.
    Tls(Endpoint),
    /// A co-located indexer on a loopback address, spoken to in plain HTTP.
    Loopback {
        address: SocketAddr,
        authority: String,
        base_path: String,
    },
}

impl Indexer {
    pub(super) fn parse(value: &str) -> Result<Self, String> {
        let Some(rest) = value.strip_prefix("http://") else {
            return Endpoint::parse(value).map(Self::Tls);
        };
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        if path.contains(['?', '#', '\\']) {
            return Err("indexer endpoint is not canonical".to_owned());
        }
        let address = authority
            .parse::<SocketAddr>()
            .map_err(|_| "plain HTTP indexer endpoints must name an IP address and port")?;
        if !address.ip().is_loopback() {
            return Err("plain HTTP indexer endpoints must be loopback".to_owned());
        }
        Ok(Self::Loopback {
            address,
            authority: authority.to_owned(),
            base_path: if path.is_empty() {
                String::new()
            } else {
                format!("/{}", path.trim_end_matches('/'))
            },
        })
    }
}

pub(super) fn configured_endpoint() -> Result<Option<Indexer>, String> {
    std::env::var("LAYERX_GATEWAY_INDEXER_URL")
        .ok()
        .map(|url| Indexer::parse(&url))
        .transpose()
}

/// One way of reaching the indexer: the configured endpoint and, for a TLS
/// endpoint, the gateway's component client.
pub(super) struct Source<'a> {
    pub(super) indexer: &'a Indexer,
    pub(super) client: Option<&'a Client>,
}

impl Source<'_> {
    fn get(&self, path: &str, query: &str) -> Result<(u16, Value), &'static str> {
        let upstream: Result<UpstreamResponse, String> = match self.indexer {
            Indexer::Tls(endpoint) => match self.client {
                Some(client) => client.get_with_query(endpoint, path, query),
                None => Err("no component client".to_owned()),
            },
            Indexer::Loopback {
                address,
                authority,
                base_path,
            } => http::loopback_get(*address, authority, &format!("{base_path}{path}"), query),
        };
        let upstream = upstream.map_err(|_| "indexer_unreachable")?;
        if upstream.content_type != "application/json" {
            return Err("invalid_indexer_response");
        }
        let body =
            serde_json::from_slice(&upstream.body).map_err(|_| "invalid_indexer_response")?;
        Ok((upstream.status, body))
    }
}

/// The paging selector of one history read.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Query {
    pub(super) cursor: Option<String>,
    pub(super) limit: u64,
    pub(super) kind: Option<String>,
}

impl Query {
    fn encoded(&self) -> String {
        let mut pairs = vec![format!("limit={}", self.limit)];
        if let Some(cursor) = &self.cursor {
            pairs.push(format!("cursor={}", encode(cursor)));
        }
        if let Some(kind) = &self.kind {
            pairs.push(format!("kind={}", encode(kind)));
        }
        pairs.join("&")
    }
}

/// Why a history read has no page.
#[derive(Debug, PartialEq)]
pub(super) enum Failure {
    /// The indexer refused the selector; its refusal document is kept.
    Refused(Value),
    /// The indexer could not answer.
    Unavailable(&'static str),
}

fn encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

/// Splits `[account, cursor?, limit?, kind?]`.
pub(super) fn params(params: Option<&Value>) -> Result<(String, Query), i32> {
    let Some(Value::Array(args)) = params else {
        return Err(-32602);
    };
    if args.is_empty() || args.len() > 4 {
        return Err(-32602);
    }
    let Value::String(account) = &args[0] else {
        return Err(-32602);
    };
    let cursor = match args.get(1) {
        None | Some(Value::Null) => None,
        Some(Value::String(cursor))
            if (1..=MAXIMUM_CURSOR).contains(&cursor.len())
                && cursor.bytes().all(|byte| byte.is_ascii_alphanumeric()) =>
        {
            Some(cursor.clone())
        }
        Some(_) => return Err(-32602),
    };
    let limit = match args.get(2) {
        None | Some(Value::Null) => DEFAULT_LIMIT,
        Some(Value::Number(limit)) => limit
            .as_u64()
            .filter(|limit| (1..=MAXIMUM_LIMIT).contains(limit))
            .ok_or(-32602)?,
        Some(_) => return Err(-32602),
    };
    let kind = match args.get(3) {
        None | Some(Value::Null) => None,
        Some(Value::String(kind))
            if (1..=MAXIMUM_KIND).contains(&kind.len())
                && kind.bytes().all(|byte| {
                    byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'
                }) =>
        {
            Some(kind.clone())
        }
        Some(_) => return Err(-32602),
    };
    Ok((
        account.clone(),
        Query {
            cursor,
            limit,
            kind,
        },
    ))
}

/// A `LayerX` account identifier as the indexer keys it: 64 lowercase hex.
pub(super) fn layerx_key(account: &str) -> Option<String> {
    super::parse_hex32(account)
        .ok()
        .filter(|key| key.iter().any(|byte| *byte != 0))
        .map(|key| super::hex(&key))
}

/// A Paxeer account as the indexer keys it: a lowercase `0x` address.
pub(super) fn paxeer_key(account: &str) -> Option<String> {
    account
        .starts_with("0x")
        .then(|| evm::parse_address(account))
        .flatten()
        .map(|address| evm::address_hex(&address))
}

struct Page {
    items: Vec<(u64, Value)>,
    next_cursor: Option<String>,
}

fn page(source: &Source<'_>, account: &str, query: &Query) -> Result<Page, Failure> {
    let (status, body) = source
        .get(
            &format!("/v1/history/{}", encode(account)),
            &query.encoded(),
        )
        .map_err(Failure::Unavailable)?;
    match status {
        200 => {}
        400 => return Err(Failure::Refused(body)),
        _ => return Err(Failure::Unavailable("indexer_unavailable")),
    }
    let invalid = || Failure::Unavailable("invalid_indexer_response");
    if body.get("version") != Some(&json!(1)) {
        return Err(invalid());
    }
    let next_cursor = match body.get("next_cursor") {
        Some(Value::Null) => None,
        Some(Value::String(cursor)) => Some(cursor.clone()),
        _ => return Err(invalid()),
    };
    let rows = body
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    if usize::try_from(query.limit).map_or(true, |limit| rows.len() > limit) {
        return Err(invalid());
    }
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let id = row
            .get("id")
            .and_then(Value::as_str)
            .and_then(|id| id.parse::<u64>().ok())
            .filter(|_| row.get("asset").is_some_and(Value::is_string))
            .ok_or_else(invalid)?;
        if items.last().is_some_and(|(last, _)| *last <= id) {
            return Err(invalid());
        }
        items.push((id, row.clone()));
    }
    Ok(Page { items, next_cursor })
}

fn text<'a>(document: &'a Value, field: &str) -> Option<&'a str> {
    document.get(field).and_then(Value::as_str)
}

/// The joined metadata of one asset: the indexer's record, the registry
/// symbol and decimals, the `LayerX` asset it is (or points at) and, for a
/// pointer contract, its EVM address.
fn asset_metadata(asset: &str, record: &Value, registry: &[Value]) -> Value {
    if !record.is_object() {
        return Value::Null;
    }
    let kind = text(record, "kind").unwrap_or_default();
    let denom = text(record, "denom");
    let registered = registry.iter().find(|entry| {
        if kind == "layerx" {
            text(entry, "asset_id") == Some(asset)
        } else {
            denom.is_some() && text(entry, "denom") == denom
        }
    });
    let metadata = record.get("metadata").cloned().unwrap_or(Value::Null);
    let symbol = text(&metadata, "symbol")
        .or_else(|| registered.and_then(|entry| text(entry, "symbol")))
        .map(str::to_owned);
    let decimals = metadata
        .get("decimals")
        .and_then(Value::as_u64)
        .or_else(|| registered.and_then(|entry| entry.get("decimals").and_then(Value::as_u64)))
        .or_else(|| (kind == "native").then_some(18));
    let native_id = if kind == "layerx" {
        Some(asset.to_owned())
    } else {
        registered
            .and_then(|entry| text(entry, "asset_id"))
            .map(str::to_owned)
    };
    json!({
        "asset": asset,
        "chain": record.get("chain").cloned().unwrap_or(Value::Null),
        "kind": kind,
        "address": record.get("address").cloned().unwrap_or(Value::Null),
        "denom": denom,
        "symbol": symbol,
        "decimals": decimals,
        "native_id": native_id,
        "pointer": if kind == "pointer" { record.get("address").cloned().unwrap_or(Value::Null) } else { Value::Null },
        "metadata": metadata,
    })
}

fn join_assets(
    source: &Source<'_>,
    registry: &dyn Fn() -> Vec<Value>,
    items: &mut [Value],
) -> Result<(), Failure> {
    if items.is_empty() {
        return Ok(());
    }
    let mut records: BTreeMap<String, Value> = BTreeMap::new();
    for item in items.iter() {
        let Some(asset) = text(item, "asset") else {
            continue;
        };
        if records.contains_key(asset) {
            continue;
        }
        let (status, body) = source
            .get(&format!("/v1/assets/{}", encode(asset)), "")
            .map_err(Failure::Unavailable)?;
        let record = match status {
            200 => body
                .get("asset")
                .filter(|record| text(record, "asset") == Some(asset))
                .cloned()
                .ok_or(Failure::Unavailable("invalid_indexer_response"))?,
            404 => Value::Null,
            _ => return Err(Failure::Unavailable("indexer_unavailable")),
        };
        records.insert(asset.to_owned(), record);
    }
    let registry = registry();
    for item in items.iter_mut() {
        let asset = text(item, "asset").unwrap_or_default().to_owned();
        let joined = records.get(&asset).map_or(Value::Null, |record| {
            asset_metadata(&asset, record, &registry)
        });
        if let Some(row) = item.as_object_mut() {
            row.insert("asset_metadata".to_owned(), joined);
        }
    }
    Ok(())
}

/// One page of the merged history of `accounts`, each tagged with the side
/// it answers for. A single account's page and cursor are the indexer's own;
/// several accounts are merged newest first with each row tagged by `side`.
pub(super) fn history(
    source: &Source<'_>,
    registry: &dyn Fn() -> Vec<Value>,
    accounts: &[(&'static str, String)],
    query: &Query,
) -> Result<Value, Failure> {
    let limit = usize::try_from(query.limit).unwrap_or(usize::MAX);
    let (mut items, next_cursor): (Vec<Value>, Option<String>) = if let [(_, account)] = accounts {
        let page = page(source, account, query)?;
        (
            page.items.into_iter().map(|(_, item)| item).collect(),
            page.next_cursor,
        )
    } else {
        let mut merged: BTreeMap<u64, Value> = BTreeMap::new();
        let mut more = false;
        for (side, account) in accounts {
            let page = page(source, account, query)?;
            more |= page.next_cursor.is_some();
            for (id, mut item) in page.items {
                if let Some(row) = item.as_object_mut() {
                    row.insert("side".to_owned(), json!(side));
                }
                merged.entry(id).or_insert(item);
            }
        }
        more |= merged.len() > limit;
        let newest: Vec<(u64, Value)> = merged.into_iter().rev().take(limit).collect();
        let next_cursor = if more {
            newest.last().map(|(id, _)| id.to_string())
        } else {
            None
        };
        (
            newest.into_iter().map(|(_, item)| item).collect(),
            next_cursor,
        )
    };
    join_assets(source, registry, &mut items)?;
    Ok(json!({"items": items, "next_cursor": next_cursor}))
}

fn refusal(id: &Value, code: i32, message: &str, data: Value) -> Value {
    let mut refusal = super::rpc::error(id, code, message);
    refusal["error"]["data"] = data;
    refusal
}

fn registry(config: &Config) -> Vec<Value> {
    super::rpc::read_result(config, "/v1/assets")
        .and_then(|result| result.get("assets").and_then(Value::as_array).cloned())
        .unwrap_or_default()
}

fn answer(config: &Config, method: &str, id: &Value, params: Option<&Value>) -> Value {
    let (account, query) = match self::params(params) {
        Ok(selected) => selected,
        Err(code) => return super::rpc::error(id, code, "Invalid params"),
    };
    let (document, accounts) = match method {
        "lx_getHistory" => match layerx_key(&account) {
            Some(key) => (json!(key.clone()), vec![("layerx", key)]),
            None => return super::rpc::error(id, -32602, "Invalid params"),
        },
        "px_getHistory" => match paxeer_key(&account) {
            Some(key) => (json!(key.clone()), vec![("paxeer", key)]),
            None => return super::rpc::error(id, -32602, "Invalid params"),
        },
        _ => match super::paxeer::history_accounts(config, id, &account) {
            Ok(resolved) => resolved,
            Err(refused) => return refused,
        },
    };
    let Some(indexer) = &config.indexer else {
        return refusal(
            id,
            -32001,
            "History unavailable",
            json!({"code": "indexer_not_configured"}),
        );
    };
    if !public_reads::consume_read() {
        return refusal(
            id,
            -32005,
            "Read unavailable",
            json!({"code": "public_read_rate_limit"}),
        );
    }
    let source = Source {
        indexer,
        client: Some(&config.client),
    };
    let mut result = match history(&source, &|| registry(config), &accounts, &query) {
        Ok(result) => result,
        Err(Failure::Refused(body)) => return refusal(id, -32602, "Invalid params", body),
        Err(Failure::Unavailable(code)) => {
            return refusal(id, -32001, "History unavailable", json!({"code": code}))
        }
    };
    if let Some(object) = result.as_object_mut() {
        object.insert("account".to_owned(), document);
        if method == "px_getUnifiedHistory" {
            let sides: Vec<Value> = accounts
                .iter()
                .map(|(side, account)| json!({"side": side, "account": account}))
                .collect();
            object.insert("accounts".to_owned(), Value::Array(sides));
        }
    }
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

/// Answers the three history methods, or `None` for any other method.
pub(super) fn dispatch(
    config: &Config,
    method: &str,
    id: &Value,
    params: Option<&Value>,
) -> Option<Value> {
    matches!(
        method,
        "lx_getHistory" | "px_getHistory" | "px_getUnifiedHistory"
    )
    .then(|| answer(config, method, id, params))
}

/// The indexer's state for the gateway's own status document.
pub(super) fn status(config: &Config) -> &'static str {
    let Some(indexer) = &config.indexer else {
        return "not_configured";
    };
    let source = Source {
        indexer,
        client: Some(&config.client),
    };
    match source.get("/healthz", "") {
        Ok((200, _)) => "available",
        _ => "unavailable",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layerx_indexer::store::{AssetRow, Store, TransferRow, Unit};
    use std::net::TcpListener;
    use std::sync::Arc;

    const LXP: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const ALICE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const BOB: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const EVE: &str = "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    const FRANK: &str = "0xffffffffffffffffffffffffffffffffffffffff";
    const POINTER: &str = "0xcccccccccccccccccccccccccccccccccccccccc";
    const TICKET: &str = "denom:factory/pax1issuer/ticket";

    fn leg(
        position: u64,
        kind: &str,
        direction: &'static str,
        account: &str,
        counterparty: &str,
        asset: &str,
        amount: &str,
    ) -> TransferRow {
        TransferRow {
            height_or_seq: position,
            kind: kind.to_owned(),
            direction,
            account: account.to_owned(),
            counterparty: Some(counterparty.to_owned()),
            asset: asset.to_owned(),
            amount: amount.to_owned(),
            tx_id: format!("{kind}-{position}"),
            ordinal: 0,
            decoded: json!({"position": position.to_string()}),
        }
    }

    fn asset(
        asset: &str,
        chain: &str,
        kind: &str,
        address: Option<&str>,
        denom: Option<&str>,
    ) -> AssetRow {
        AssetRow {
            asset: asset.to_owned(),
            chain: chain.to_owned(),
            kind: kind.to_owned(),
            address: address.map(str::to_owned),
            denom: denom.map(str::to_owned),
            metadata: json!({}),
        }
    }

    fn unit(
        chain: &str,
        position: u64,
        transfers: Vec<TransferRow>,
        assets: Vec<AssetRow>,
    ) -> Unit {
        Unit {
            chain: chain.to_owned(),
            position,
            hash: format!("{chain}-{position}"),
            parent: format!("{chain}-{}", position - 1),
            link: format!("{chain}-{position}"),
            boundary: position,
            transfers,
            assets,
            ..Unit::default()
        }
    }

    /// A real indexer store and read API on a loopback port, filled with both
    /// chains interleaved in commit order. Row ids: 1-2 LayerX transfer
    /// alice->bob, 3-4 Paxeer native eve->frank, 5 LayerX credit to alice,
    /// 6-7 Paxeer pointer frank->eve, 8 Paxeer custody deposit into alice,
    /// 9-10 Paxeer bank ticket eve->frank, 11 an unregistered asset to eve.
    fn indexer() -> Indexer {
        let store = Store::open_in_memory().unwrap_or_else(|error| panic!("{error}"));
        store
            .register_assets(&[AssetRow {
                asset: format!("evm:{POINTER}"),
                ..asset("", "paxeer", "pointer", Some(POINTER), Some("ulxp"))
            }])
            .unwrap_or_else(|error| panic!("{error}"));
        let units = [
            unit(
                "layerx",
                1,
                vec![
                    leg(1, "lxp_transfer", "out", ALICE, BOB, LXP, "5"),
                    leg(1, "lxp_transfer", "in", BOB, ALICE, LXP, "5"),
                ],
                vec![AssetRow {
                    metadata: json!({"supply": "100"}),
                    ..asset(LXP, "layerx", "layerx", None, None)
                }],
            ),
            unit(
                "paxeer",
                10,
                vec![
                    leg(
                        10,
                        "native_transfer",
                        "out",
                        EVE,
                        FRANK,
                        "evm:native",
                        "1000000000000000000",
                    ),
                    leg(
                        10,
                        "native_transfer",
                        "in",
                        FRANK,
                        EVE,
                        "evm:native",
                        "1000000000000000000",
                    ),
                ],
                vec![asset("evm:native", "paxeer", "native", None, None)],
            ),
            unit(
                "layerx",
                2,
                vec![TransferRow {
                    counterparty: None,
                    ..leg(2, "lxp_credit", "in", ALICE, "", LXP, "7")
                }],
                Vec::new(),
            ),
            unit(
                "paxeer",
                11,
                vec![
                    leg(
                        11,
                        "pointer_transfer",
                        "in",
                        EVE,
                        FRANK,
                        &format!("evm:{POINTER}"),
                        "3",
                    ),
                    leg(
                        11,
                        "pointer_transfer",
                        "out",
                        FRANK,
                        EVE,
                        &format!("evm:{POINTER}"),
                        "3",
                    ),
                    leg(11, "custody_deposit", "in", ALICE, EVE, LXP, "9"),
                ],
                Vec::new(),
            ),
            unit(
                "paxeer",
                12,
                vec![
                    leg(12, "bank_transfer", "out", EVE, FRANK, TICKET, "2"),
                    leg(12, "bank_transfer", "in", FRANK, EVE, TICKET, "2"),
                    leg(
                        12,
                        "erc20_transfer",
                        "in",
                        EVE,
                        FRANK,
                        "evm:0x0000000000000000000000000000000000000abc",
                        "4",
                    ),
                ],
                vec![asset(
                    TICKET,
                    "paxeer",
                    "tokenfactory",
                    None,
                    Some("factory/pax1issuer/ticket"),
                )],
            ),
        ];
        for unit in &units {
            store
                .commit(unit, 64)
                .unwrap_or_else(|error| panic!("{error}"));
        }
        let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("{error}"));
        let address = listener
            .local_addr()
            .unwrap_or_else(|error| panic!("{error}"));
        let store = Arc::new(store);
        std::thread::spawn(move || layerx_indexer::api::serve(&listener, &store, None));
        Indexer::parse(&format!("http://{address}")).unwrap_or_else(|error| panic!("{error}"))
    }

    fn registry() -> Vec<Value> {
        vec![
            json!({"asset_id": LXP, "symbol": "LXP", "name": "LayerX Points", "decimals": 6, "denom": "ulxp"}),
        ]
    }

    fn query(cursor: Option<&str>, limit: u64, kind: Option<&str>) -> Query {
        Query {
            cursor: cursor.map(str::to_owned),
            limit,
            kind: kind.map(str::to_owned),
        }
    }

    fn read(indexer: &Indexer, accounts: &[(&'static str, String)], query: &Query) -> Value {
        history(
            &Source {
                indexer,
                client: None,
            },
            &registry,
            accounts,
            query,
        )
        .unwrap_or_else(|failure| panic!("{failure:?}"))
    }

    fn ids(page: &Value) -> Vec<u64> {
        page["items"]
            .as_array()
            .unwrap_or_else(|| panic!("no items in {page}"))
            .iter()
            .map(|item| {
                item["id"]
                    .as_str()
                    .and_then(|id| id.parse().ok())
                    .unwrap_or_else(|| panic!("row id in {item}"))
            })
            .collect()
    }

    fn raw(indexer: &Indexer, path: &str, query: &str) -> Value {
        Source {
            indexer,
            client: None,
        }
        .get(path, query)
        .unwrap_or_else(|code| panic!("{code}"))
        .1
    }

    #[test]
    fn layerx_history_joins_registry_metadata_and_passes_the_indexer_cursor_through() {
        let indexer = indexer();
        let alice = [("layerx", ALICE.to_owned())];
        let first = read(&indexer, &alice, &query(None, 2, None));
        assert_eq!(ids(&first), vec![8, 5]);
        let upstream = raw(&indexer, &format!("/v1/history/{ALICE}"), "limit=2");
        assert_eq!(first["next_cursor"], upstream["next_cursor"]);
        assert_eq!(first["next_cursor"], json!("5"));
        for (item, row) in first["items"]
            .as_array()
            .unwrap_or_else(|| panic!("items"))
            .iter()
            .zip(
                upstream["items"]
                    .as_array()
                    .unwrap_or_else(|| panic!("rows")),
            )
        {
            let mut stripped = item.clone();
            if let Some(object) = stripped.as_object_mut() {
                object.remove("asset_metadata");
            }
            assert_eq!(&stripped, row);
        }
        let joined = &first["items"][1]["asset_metadata"];
        assert_eq!(joined["asset"], json!(LXP));
        assert_eq!(joined["chain"], json!("layerx"));
        assert_eq!(joined["kind"], json!("layerx"));
        assert_eq!(joined["symbol"], json!("LXP"));
        assert_eq!(joined["decimals"], json!(6));
        assert_eq!(joined["native_id"], json!(LXP));
        assert_eq!(joined["pointer"], Value::Null);
        assert_eq!(joined["metadata"], json!({"supply": "100"}));
        assert_eq!(first["items"][0]["chain"], json!("paxeer"));
        assert_eq!(first["items"][0]["kind"], json!("custody_deposit"));
        let cursor = first["next_cursor"].as_str().unwrap_or_default();
        let second = read(&indexer, &alice, &query(Some(cursor), 2, None));
        assert_eq!(ids(&second), vec![1]);
        assert_eq!(second["next_cursor"], Value::Null);
        assert_eq!(second["items"][0]["direction"], json!("out"));
        assert_eq!(second["items"][0]["counterparty"], json!(BOB));
        let credits = read(&indexer, &alice, &query(None, 10, Some("lxp_credit")));
        assert_eq!(ids(&credits), vec![5]);
        assert_eq!(credits["items"][0]["amount"], json!("7"));
    }

    #[test]
    fn paxeer_history_joins_native_pointer_denom_and_unknown_assets() {
        let indexer = indexer();
        let eve = [("paxeer", EVE.to_owned())];
        let page = read(&indexer, &eve, &query(None, 50, None));
        assert_eq!(ids(&page), vec![11, 9, 6, 3]);
        assert_eq!(page["next_cursor"], Value::Null);
        let metadata = |index: usize| page["items"][index]["asset_metadata"].clone();
        assert_eq!(metadata(0), Value::Null);
        let ticket = metadata(1);
        assert_eq!(ticket["asset"], json!(TICKET));
        assert_eq!(ticket["kind"], json!("tokenfactory"));
        assert_eq!(ticket["denom"], json!("factory/pax1issuer/ticket"));
        assert_eq!(ticket["symbol"], Value::Null);
        assert_eq!(ticket["decimals"], Value::Null);
        assert_eq!(ticket["native_id"], Value::Null);
        let pointer = metadata(2);
        assert_eq!(pointer["asset"], json!(format!("evm:{POINTER}")));
        assert_eq!(pointer["chain"], json!("paxeer"));
        assert_eq!(pointer["kind"], json!("pointer"));
        assert_eq!(pointer["pointer"], json!(POINTER));
        assert_eq!(pointer["denom"], json!("ulxp"));
        assert_eq!(pointer["symbol"], json!("LXP"));
        assert_eq!(pointer["decimals"], json!(6));
        assert_eq!(pointer["native_id"], json!(LXP));
        let native = metadata(3);
        assert_eq!(native["asset"], json!("evm:native"));
        assert_eq!(native["kind"], json!("native"));
        assert_eq!(native["decimals"], json!(18));
        assert_eq!(native["symbol"], Value::Null);
        assert_eq!(native["pointer"], Value::Null);
        assert_eq!(page["items"][3]["amount"], json!("1000000000000000000"));
        assert_eq!(page["items"][3]["direction"], json!("out"));
        let paged = read(&indexer, &eve, &query(None, 3, None));
        assert_eq!(ids(&paged), vec![11, 9, 6]);
        assert_eq!(paged["next_cursor"], json!("6"));
        let rest = read(&indexer, &eve, &query(Some("6"), 3, None));
        assert_eq!(ids(&rest), vec![3]);
        assert_eq!(rest["next_cursor"], Value::Null);
    }

    #[test]
    fn unified_history_merges_both_sides_newest_first_across_pages() {
        let indexer = indexer();
        let sides = [("paxeer", EVE.to_owned()), ("layerx", ALICE.to_owned())];
        let whole = read(&indexer, &sides, &query(None, 100, None));
        assert_eq!(ids(&whole), vec![11, 9, 8, 6, 5, 3, 1]);
        assert_eq!(whole["next_cursor"], Value::Null);
        let tags: Vec<&str> = whole["items"]
            .as_array()
            .unwrap_or_else(|| panic!("items"))
            .iter()
            .map(|item| item["side"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(
            tags,
            vec!["paxeer", "paxeer", "layerx", "paxeer", "layerx", "paxeer", "layerx"]
        );
        let chains: Vec<&str> = whole["items"]
            .as_array()
            .unwrap_or_else(|| panic!("items"))
            .iter()
            .map(|item| item["chain"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(
            chains,
            vec!["paxeer", "paxeer", "paxeer", "paxeer", "layerx", "paxeer", "layerx"]
        );
        assert!(whole["items"]
            .as_array()
            .unwrap_or_else(|| panic!("items"))
            .iter()
            .all(|item| item.get("asset_metadata").is_some()));
        let mut walked = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let page = read(&indexer, &sides, &query(cursor.as_deref(), 2, None));
            assert!(ids(&page).len() <= 2);
            walked.extend(ids(&page));
            match page["next_cursor"].as_str() {
                Some(next) => {
                    assert_eq!(Some(next.to_owned()), ids(&page).last().map(u64::to_string));
                    cursor = Some(next.to_owned());
                }
                None => break,
            }
        }
        assert_eq!(walked, ids(&whole));
        let deposits = read(&indexer, &sides, &query(None, 10, Some("custody_deposit")));
        assert_eq!(ids(&deposits), vec![8]);
        assert_eq!(deposits["items"][0]["side"], json!("layerx"));
    }

    #[test]
    fn unknown_account_history_is_an_empty_page() {
        let indexer = indexer();
        for accounts in [
            vec![("layerx", "cd".repeat(32))],
            vec![("paxeer", format!("0x{}", "12".repeat(20)))],
            vec![
                ("paxeer", format!("0x{}", "12".repeat(20))),
                ("layerx", "cd".repeat(32)),
            ],
        ] {
            let page = read(&indexer, &accounts, &query(None, 10, None));
            assert_eq!(page, json!({"items": [], "next_cursor": null}));
        }
    }

    #[test]
    fn history_refusals_keep_the_indexer_answer() {
        let indexer = indexer();
        let refused = history(
            &Source {
                indexer: &indexer,
                client: None,
            },
            &registry,
            &[("layerx", ALICE.to_owned())],
            &query(Some("abc"), 10, None),
        );
        match refused {
            Err(Failure::Refused(body)) => assert!(body.to_string().contains("invalid_cursor")),
            other => panic!("{other:?}"),
        }
        let closed = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("{error}"));
        let address = closed
            .local_addr()
            .unwrap_or_else(|error| panic!("{error}"));
        drop(closed);
        let gone =
            Indexer::parse(&format!("http://{address}")).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            history(
                &Source {
                    indexer: &gone,
                    client: None,
                },
                &registry,
                &[("layerx", ALICE.to_owned())],
                &query(None, 10, None),
            ),
            Err(Failure::Unavailable("indexer_unreachable"))
        );
    }

    #[test]
    fn history_params_accounts_and_endpoints_are_exact() {
        assert_eq!(
            params(Some(&json!([ALICE]))),
            Ok((ALICE.to_owned(), query(None, DEFAULT_LIMIT, None)))
        );
        assert_eq!(
            params(Some(&json!([EVE, "42", 7, "erc20_transfer"]))),
            Ok((EVE.to_owned(), query(Some("42"), 7, Some("erc20_transfer"))))
        );
        assert_eq!(
            params(Some(&json!([EVE, null, null, null]))),
            Ok((EVE.to_owned(), query(None, DEFAULT_LIMIT, None)))
        );
        for invalid in [
            json!([]),
            json!({}),
            json!([1]),
            json!([EVE, 5]),
            json!([EVE, ""]),
            json!([EVE, "1&limit=500"]),
            json!([EVE, "x".repeat(33)]),
            json!([EVE, null, 0]),
            json!([EVE, null, 101]),
            json!([EVE, null, "5"]),
            json!([EVE, null, 5.5]),
            json!([EVE, null, null, "Transfer"]),
            json!([EVE, null, null, ""]),
            json!([EVE, null, null, "a".repeat(65)]),
            json!([EVE, null, null, null, null]),
        ] {
            assert_eq!(params(Some(&invalid)), Err(-32602), "{invalid}");
        }
        assert_eq!(params(None), Err(-32602));
        assert_eq!(
            query(Some("42"), 7, Some("erc20_transfer")).encoded(),
            "limit=7&cursor=42&kind=erc20_transfer"
        );
        assert_eq!(encode(TICKET), "denom%3Afactory%2Fpax1issuer%2Fticket");
        assert_eq!(
            layerx_key(&ALICE.to_ascii_uppercase()),
            Some(ALICE.to_owned())
        );
        assert_eq!(layerx_key(&"00".repeat(32)), None);
        assert_eq!(layerx_key(EVE), None);
        assert_eq!(
            paxeer_key(&EVE.to_ascii_uppercase().replacen("0X", "0x", 1)),
            Some(EVE.to_owned())
        );
        assert_eq!(paxeer_key(&EVE[2..]), None);
        assert_eq!(paxeer_key(ALICE), None);
        assert!(matches!(
            Indexer::parse("http://127.0.0.1:8480/indexer/"),
            Ok(Indexer::Loopback { ref base_path, .. }) if base_path == "/indexer"
        ));
        assert!(matches!(
            Indexer::parse("http://[::1]:8480"),
            Ok(Indexer::Loopback { .. })
        ));
        assert!(matches!(
            Indexer::parse("https://indexer.internal:8480"),
            Ok(Indexer::Tls(_))
        ));
        for refused in [
            "http://10.0.0.8:8480",
            "http://indexer.internal:8480",
            "http://127.0.0.1",
            "http://127.0.0.1:8480/?x",
            "ftp://indexer.internal",
            "https://10.0.0.8:8480",
        ] {
            assert!(Indexer::parse(refused).is_err(), "{refused}");
        }
    }
}
