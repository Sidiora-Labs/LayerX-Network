//! Which fork surfaces the Paxeer chain answers for.
//!
//! The exchange, bridge and launchpad precompiles exist only from the Paxeer X
//! fork on; before it `eth_getCode` at their addresses answers `0x`. The
//! gateway probes the three addresses at one node height, keeps the answer
//! for a short TTL, publishes it as `px_getCapabilities`, and refuses a raw
//! transaction addressed to a surface whose precompile has no code with the
//! typed `surface_unavailable` error instead of relaying it.

use super::{paxeer, Config};
use layerx_platform_gateway::evm;
use serde_json::{json, Value};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// JSON-RPC code of the typed refusal, between `-32002` (insufficient scope)
/// and `-32004` (WebSocket required) in the gateway's scheme.
pub(super) const SURFACE_UNAVAILABLE: i32 = -32003;

/// Relayed EVM methods that submit a signed transaction to the node.
pub(super) const SUBMISSION_METHODS: [&str; 2] =
    ["eth_sendRawTransaction", "eth_sendRawTransactionSync"];

const DEFAULT_TTL_SECONDS: u64 = 15;
const MAX_TTL_SECONDS: u64 = 300;
const MAX_RAW_TRANSACTION_BYTES: usize = 4 * 1024 * 1024;

/// One fork surface and the precompile that carries it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Surface {
    Exchange,
    Bridge,
    Launchpad,
}

impl Surface {
    pub(super) const ALL: [Self; 3] = [Self::Exchange, Self::Bridge, Self::Launchpad];

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Exchange => "exchange",
            Self::Bridge => "bridge",
            Self::Launchpad => "launchpad",
        }
    }

    pub(super) fn address(self) -> [u8; 20] {
        match self {
            Self::Exchange => evm::EXCHANGE_PRECOMPILE,
            Self::Bridge => evm::BRIDGE_PRECOMPILE,
            Self::Launchpad => evm::LAUNCHPAD_PRECOMPILE,
        }
    }

    fn of(address: &[u8; 20]) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|surface| surface.address() == *address)
    }
}

/// One probe of the three surfaces at one node height.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Capabilities {
    pub(super) exchange: bool,
    pub(super) bridge: bool,
    pub(super) launchpad: bool,
    pub(super) probed_at: u64,
    pub(super) rpc_height: u64,
}

impl Capabilities {
    pub(super) fn live(&self, surface: Surface) -> bool {
        match surface {
            Surface::Exchange => self.exchange,
            Surface::Bridge => self.bridge,
            Surface::Launchpad => self.launchpad,
        }
    }

    pub(super) fn document(&self) -> Value {
        json!({
            "exchange": self.exchange,
            "bridge": self.bridge,
            "launchpad": self.launchpad,
            "probed_at": self.probed_at,
            "rpc_height": self.rpc_height.to_string()
        })
    }
}

/// The last probe and when it was taken.
pub(super) struct Cache {
    ttl: Duration,
    entry: Mutex<Option<(Instant, Capabilities)>>,
}

impl Cache {
    pub(super) fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            entry: Mutex::new(None),
        }
    }

    /// The cached answer while it is younger than the TTL, otherwise a fresh
    /// probe. A failed probe is returned and never cached.
    pub(super) fn current(
        &self,
        now: Instant,
        probe: impl FnOnce() -> Result<Capabilities, &'static str>,
    ) -> Result<Capabilities, &'static str> {
        let mut entry = self
            .entry
            .lock()
            .map_err(|_| "capabilities_cache_unavailable")?;
        if let Some((taken, capabilities)) = entry.as_ref() {
            if now.saturating_duration_since(*taken) < self.ttl {
                return Ok(capabilities.clone());
            }
        }
        let fresh = probe()?;
        *entry = Some((now, fresh.clone()));
        Ok(fresh)
    }
}

pub(super) fn configured() -> Result<Cache, String> {
    let seconds = std::env::var("LAYERX_GATEWAY_CAPABILITIES_TTL_SECONDS")
        .unwrap_or_else(|_| DEFAULT_TTL_SECONDS.to_string())
        .parse::<u64>()
        .map_err(|_| "gateway capabilities TTL is invalid".to_owned())?;
    if !(1..=MAX_TTL_SECONDS).contains(&seconds) {
        return Err("gateway capabilities TTL is outside its bound".to_owned());
    }
    Ok(Cache::new(Duration::from_secs(seconds)))
}

/// Whether one `eth_getCode` answer carries code: `0x` is absent, any
/// non-empty byte string is live, anything else is refused.
pub(super) fn has_code(answer: &Value) -> Result<bool, &'static str> {
    let text = answer
        .get("result")
        .and_then(Value::as_str)
        .ok_or("paxeer_code_refused")?;
    let digits = text.strip_prefix("0x").ok_or("invalid_paxeer_response")?;
    if !digits.len().is_multiple_of(2) || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("invalid_paxeer_response");
    }
    Ok(!digits.is_empty())
}

fn height(answer: &Value) -> Result<u64, &'static str> {
    answer
        .get("result")
        .and_then(Value::as_str)
        .and_then(|text| text.strip_prefix("0x"))
        .filter(|digits| !digits.is_empty() && digits.len() <= 16)
        .and_then(|digits| u64::from_str_radix(digits, 16).ok())
        .ok_or("invalid_paxeer_response")
}

/// Reads the node head, then `eth_getCode` for every surface at that head.
pub(super) fn probe(
    mut node: impl FnMut(&Value) -> Result<Value, &'static str>,
    probed_at: u64,
) -> Result<Capabilities, &'static str> {
    let head = node(&json!({
        "jsonrpc": "2.0",
        "id": "px-capabilities",
        "method": "eth_blockNumber",
        "params": []
    }))?;
    let rpc_height = height(&head)?;
    let tag = format!("0x{rpc_height:x}");
    let mut live = [false; 3];
    for (slot, surface) in live.iter_mut().zip(Surface::ALL) {
        let answer = node(&json!({
            "jsonrpc": "2.0",
            "id": "px-capabilities",
            "method": "eth_getCode",
            "params": [evm::address_hex(&surface.address()), tag]
        }))?;
        *slot = has_code(&answer)?;
    }
    let [exchange, bridge, launchpad] = live;
    Ok(Capabilities {
        exchange,
        bridge,
        launchpad,
        probed_at,
        rpc_height,
    })
}

fn current(config: &Config) -> Result<Capabilities, &'static str> {
    config.capabilities.current(Instant::now(), || {
        let probed_at = super::now().map_err(|_| "clock_unavailable")?;
        probe(|request| paxeer::node(config, request), probed_at)
    })
}

/// `px_getCapabilities`.
pub(super) fn get(config: &Config, id: &Value, params: Option<&Value>) -> Value {
    if config.paxeer.is_none() {
        return paxeer::unconfigured(id);
    }
    if !paxeer::no_params(params) {
        return super::rpc::error(id, -32602, "Invalid params");
    }
    match current(config) {
        Ok(capabilities) => json!({"jsonrpc":"2.0","id":id,"result":capabilities.document()}),
        Err(code) => paxeer::unavailable(id, code),
    }
}

/// The typed refusal for a write to a surface whose precompile has no code.
pub(super) fn refusal(id: &Value, surface: Surface) -> Value {
    let mut refusal = super::rpc::error(id, SURFACE_UNAVAILABLE, "Surface unavailable");
    refusal["error"]["data"] = json!({
        "code": "surface_unavailable",
        "surface": surface.name(),
        "address": evm::address_hex(&surface.address())
    });
    refusal
}

/// The fork surface a submission writes to, or `None` when the method is not
/// a submission or its transaction is not addressed to a surface precompile.
pub(super) fn submission_surface(method: &str, params: Option<&Value>) -> Option<Surface> {
    if !SUBMISSION_METHODS.contains(&method) {
        return None;
    }
    let Some(Value::Array(args)) = params else {
        return None;
    };
    let digits = args.first()?.as_str()?.strip_prefix("0x")?;
    let raw = super::decode_hex(digits, MAX_RAW_TRANSACTION_BYTES).ok()?;
    Surface::of(&transaction_target(&raw)?)
}

/// Consults the capabilities for a surface write and answers the refusal, or
/// `None` when the request may be relayed.
pub(super) fn gate_with(
    cache: &Cache,
    now: Instant,
    probe: impl FnOnce() -> Result<Capabilities, &'static str>,
    id: &Value,
    method: &str,
    params: Option<&Value>,
) -> Option<Value> {
    let surface = submission_surface(method, params)?;
    match cache.current(now, probe) {
        Ok(capabilities) if capabilities.live(surface) => None,
        Ok(_) => Some(refusal(id, surface)),
        Err(code) => Some(paxeer::unavailable(id, code)),
    }
}

pub(super) fn gate(
    config: &Config,
    id: &Value,
    method: &str,
    params: Option<&Value>,
) -> Option<Value> {
    config.paxeer.as_ref()?;
    gate_with(
        &config.capabilities,
        Instant::now(),
        || {
            let probed_at = super::now().map_err(|_| "clock_unavailable")?;
            probe(|request| paxeer::node(config, request), probed_at)
        },
        id,
        method,
        params,
    )
}

fn rlp_length(bytes: &[u8]) -> Option<usize> {
    if bytes.is_empty() || bytes.len() > 8 || bytes.first() == Some(&0) {
        return None;
    }
    bytes.iter().try_fold(0_usize, |length, byte| {
        length.checked_mul(256)?.checked_add(usize::from(*byte))
    })
}

/// One RLP item at the start of `bytes`: payload offset, payload length and
/// whether it is a list.
fn rlp_item(bytes: &[u8]) -> Option<(usize, usize, bool)> {
    let first = *bytes.first()?;
    let (offset, length, list) = match first {
        0x00..=0x7f => (0, 1, false),
        0x80..=0xb7 => (1, usize::from(first - 0x80), false),
        0xb8..=0xbf => {
            let width = usize::from(first - 0xb7);
            (1 + width, rlp_length(bytes.get(1..=width)?)?, false)
        }
        0xc0..=0xf7 => (1, usize::from(first - 0xc0), true),
        0xf8..=0xff => {
            let width = usize::from(first - 0xf7);
            (1 + width, rlp_length(bytes.get(1..=width)?)?, true)
        }
    };
    (offset.checked_add(length)? <= bytes.len()).then_some((offset, length, list))
}

/// The `to` field of a signed legacy, EIP-2930, EIP-1559, EIP-4844 or
/// EIP-7702 transaction, or `None` for a contract creation or bytes that do
/// not decode.
pub(super) fn transaction_target(raw: &[u8]) -> Option<[u8; 20]> {
    let (body, field) = match *raw.first()? {
        0x01 => (raw.get(1..)?, 4),
        0x02..=0x04 => (raw.get(1..)?, 5),
        0xc0..=0xff => (raw, 3),
        _ => return None,
    };
    let (offset, length, list) = rlp_item(body)?;
    if !list || offset + length != body.len() {
        return None;
    }
    let mut fields = body.get(offset..offset + length)?;
    if raw.first() == Some(&0x03) {
        let (inner, inner_length, inner_list) = rlp_item(fields)?;
        if inner_list {
            fields = fields.get(inner..inner + inner_length)?;
        }
    }
    for _ in 0..field {
        let (skip, skipped, _) = rlp_item(fields)?;
        fields = fields.get(skip + skipped..)?;
    }
    let (offset, length, list) = rlp_item(fields)?;
    if list || offset != 1 || length != 20 {
        return None;
    }
    fields.get(1..21)?.try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn rlp_bytes(bytes: &[u8]) -> Vec<u8> {
        match bytes {
            [single] if *single < 0x80 => vec![*single],
            _ => {
                let mut out = rlp_header(0x80, bytes.len());
                out.extend_from_slice(bytes);
                out
            }
        }
    }

    fn rlp_list(items: &[Vec<u8>]) -> Vec<u8> {
        let payload: Vec<u8> = items.concat();
        let mut out = rlp_header(0xc0, payload.len());
        out.extend(payload);
        out
    }

    fn rlp_header(base: u8, length: usize) -> Vec<u8> {
        if length <= 55 {
            return vec![base + u8::try_from(length).unwrap_or_else(|e| panic!("{e}"))];
        }
        let width: Vec<u8> = length
            .to_be_bytes()
            .into_iter()
            .skip_while(|byte| *byte == 0)
            .collect();
        let mut out = vec![base + 55 + u8::try_from(width.len()).unwrap_or_else(|e| panic!("{e}"))];
        out.extend(width);
        out
    }

    fn eip1559(to: &[u8]) -> Vec<u8> {
        let mut raw = vec![0x02];
        raw.extend(rlp_list(&[
            rlp_bytes(&[0x01, 0x2d]),
            rlp_bytes(&[0x07]),
            rlp_bytes(&[0x3b, 0x9a, 0xca, 0x00]),
            rlp_bytes(&[0x3b, 0x9a, 0xca, 0x00]),
            rlp_bytes(&[0x01, 0x86, 0xa0]),
            rlp_bytes(to),
            rlp_bytes(&[]),
            rlp_bytes(&[0xab; 100]),
            rlp_list(&[]),
            rlp_bytes(&[0x01]),
            rlp_bytes(&[0x11; 32]),
            rlp_bytes(&[0x22; 32]),
        ]));
        raw
    }

    fn legacy(to: &[u8]) -> Vec<u8> {
        rlp_list(&[
            rlp_bytes(&[0x07]),
            rlp_bytes(&[0x3b, 0x9a, 0xca, 0x00]),
            rlp_bytes(&[0x52, 0x08]),
            rlp_bytes(to),
            rlp_bytes(&[0x01]),
            rlp_bytes(&[]),
            rlp_bytes(&[0x02, 0x7e]),
            rlp_bytes(&[0x11; 32]),
            rlp_bytes(&[0x22; 32]),
        ])
    }

    fn submission(raw: &[u8]) -> Value {
        json!([format!("0x{}", crate::hex(raw))])
    }

    fn capabilities(exchange: bool, bridge: bool, launchpad: bool) -> Capabilities {
        Capabilities {
            exchange,
            bridge,
            launchpad,
            probed_at: 1_790_000_000,
            rpc_height: 23_860_000,
        }
    }

    /// The recorded Paxeer answers: pre-fork `0x` at every surface address,
    /// and the post-fork state with the three precompiles carrying code.
    fn recorded_node(
        forked: bool,
        calls: &Cell<usize>,
    ) -> impl FnMut(&Value) -> Result<Value, &'static str> + '_ {
        move |request: &Value| {
            calls.set(calls.get() + 1);
            let result = match request["method"].as_str() {
                Some("eth_blockNumber") => json!("0x16c1320"),
                Some("eth_getCode") => {
                    assert_eq!(request["params"][1], json!("0x16c1320"));
                    let address = request["params"][0].as_str().unwrap_or_default();
                    assert!(Surface::ALL
                        .iter()
                        .any(|surface| evm::address_hex(&surface.address()) == address));
                    json!(if forked { "0x01" } else { "0x" })
                }
                other => panic!("unexpected probe method {other:?}"),
            };
            Ok(json!({"jsonrpc": "2.0", "id": request["id"], "result": result}))
        }
    }

    #[test]
    fn capabilities_probe_reads_empty_code_as_absent_and_bytecode_as_live() {
        assert_eq!(has_code(&json!({"result": "0x"})), Ok(false));
        assert_eq!(has_code(&json!({"result": "0x01"})), Ok(true));
        assert_eq!(has_code(&json!({"result": "0x6080604052"})), Ok(true));
        for invalid in [
            json!({"result": "0x0"}),
            json!({"result": "6080"}),
            json!({"result": "0xzz"}),
        ] {
            assert_eq!(
                has_code(&invalid),
                Err("invalid_paxeer_response"),
                "{invalid}"
            );
        }
        assert_eq!(
            has_code(&json!({"error": {"code": -32000, "message": "header not found"}})),
            Err("paxeer_code_refused")
        );

        let calls = Cell::new(0);
        let before = probe(recorded_node(false, &calls), 1_790_000_000)
            .unwrap_or_else(|code| panic!("{code}"));
        assert_eq!(calls.get(), 4);
        assert_eq!(before, {
            let mut expected = capabilities(false, false, false);
            expected.rpc_height = 0x16c_1320;
            expected
        });
        assert_eq!(
            before.document(),
            json!({
                "exchange": false,
                "bridge": false,
                "launchpad": false,
                "probed_at": 1_790_000_000_u64,
                "rpc_height": "23860000"
            })
        );
        let after = probe(recorded_node(true, &calls), 1_790_000_015)
            .unwrap_or_else(|code| panic!("{code}"));
        assert!(Surface::ALL.into_iter().all(|surface| after.live(surface)));

        assert_eq!(
            probe(
                |_: &Value| Ok(json!({"jsonrpc": "2.0", "id": "px-capabilities", "result": "0x"})),
                0
            ),
            Err("invalid_paxeer_response")
        );
        assert_eq!(
            probe(|_: &Value| Err("paxeer_unreachable"), 0),
            Err("paxeer_unreachable")
        );
    }

    #[test]
    fn capabilities_cache_answers_within_the_ttl_and_reprobes_after_it() {
        let cache = Cache::new(Duration::from_secs(15));
        let start = Instant::now();
        let probes = Cell::new(0);
        let probe_as = |answer: Capabilities| {
            let probes = &probes;
            move || {
                probes.set(probes.get() + 1);
                Ok(answer)
            }
        };
        assert_eq!(
            cache.current(start, probe_as(capabilities(false, false, false))),
            Ok(capabilities(false, false, false))
        );
        assert_eq!(
            cache.current(
                start + Duration::from_secs(14),
                probe_as(capabilities(true, true, true))
            ),
            Ok(capabilities(false, false, false))
        );
        assert_eq!(probes.get(), 1);
        assert_eq!(
            cache.current(
                start + Duration::from_secs(15),
                probe_as(capabilities(true, true, true))
            ),
            Ok(capabilities(true, true, true))
        );
        assert_eq!(probes.get(), 2);
        assert_eq!(
            cache.current(start + Duration::from_secs(40), || Err(
                "paxeer_unreachable"
            )),
            Err("paxeer_unreachable")
        );
        assert_eq!(
            cache.current(
                start + Duration::from_secs(41),
                probe_as(capabilities(false, true, false))
            ),
            Ok(capabilities(false, true, false))
        );
        assert_eq!(probes.get(), 3);
    }

    #[test]
    fn capabilities_refuse_surface_writes_with_the_typed_error_until_code_exists() {
        for (surface, address) in [
            (
                Surface::Exchange,
                "0x0000000000000000000000000000000000001015",
            ),
            (
                Surface::Bridge,
                "0x0000000000000000000000000000000000001016",
            ),
            (
                Surface::Launchpad,
                "0x0000000000000000000000000000000000001017",
            ),
        ] {
            assert_eq!(evm::address_hex(&surface.address()), address);
        }
        let id = json!(9);
        for surface in Surface::ALL {
            for raw in [eip1559(&surface.address()), legacy(&surface.address())] {
                assert_eq!(transaction_target(&raw), Some(surface.address()));
                let params = submission(&raw);
                let cache = Cache::new(Duration::from_secs(15));
                let refused = gate_with(
                    &cache,
                    Instant::now(),
                    || Ok(capabilities(false, false, false)),
                    &id,
                    "eth_sendRawTransaction",
                    Some(&params),
                )
                .unwrap_or_else(|| panic!("{} write was relayed", surface.name()));
                assert_eq!(refused["id"], id);
                assert_eq!(refused["error"]["code"], json!(SURFACE_UNAVAILABLE));
                assert_eq!(refused["error"]["message"], "Surface unavailable");
                assert_eq!(
                    refused["error"]["data"],
                    json!({
                        "code": "surface_unavailable",
                        "surface": surface.name(),
                        "address": evm::address_hex(&surface.address())
                    })
                );
                assert!(refused.get("result").is_none());
                let mut live = capabilities(false, false, false);
                match surface {
                    Surface::Exchange => live.exchange = true,
                    Surface::Bridge => live.bridge = true,
                    Surface::Launchpad => live.launchpad = true,
                }
                let fresh = Cache::new(Duration::from_secs(15));
                assert_eq!(
                    gate_with(
                        &fresh,
                        Instant::now(),
                        || Ok(live),
                        &id,
                        "eth_sendRawTransactionSync",
                        Some(&params)
                    ),
                    None
                );
            }
        }
        let unavailable = gate_with(
            &Cache::new(Duration::from_secs(15)),
            Instant::now(),
            || Err("paxeer_unreachable"),
            &id,
            "eth_sendRawTransaction",
            Some(&submission(&eip1559(&evm::BRIDGE_PRECOMPILE))),
        )
        .unwrap_or_else(|| panic!("an unprobed bridge write was relayed"));
        assert_eq!(unavailable["error"]["code"], json!(-32001));
        assert_eq!(unavailable["error"]["data"]["code"], "paxeer_unreachable");

        let untouched = || -> Result<Capabilities, &'static str> {
            panic!("a request outside the fork surfaces consulted the capabilities")
        };
        let cache = Cache::new(Duration::from_secs(15));
        let other = [0x42_u8; 20];
        assert_eq!(transaction_target(&eip1559(&other)), Some(other));
        assert_eq!(transaction_target(&eip1559(&[])), None);
        let calldata =
            json!([{"to": evm::address_hex(&evm::EXCHANGE_PRECOMPILE), "data": "0x"}, "latest"]);
        for (method, params) in [
            ("eth_sendRawTransaction", submission(&eip1559(&other))),
            ("eth_sendRawTransaction", submission(&legacy(&[]))),
            ("eth_sendRawTransaction", json!(["0x02zz"])),
            ("eth_sendRawTransaction", json!([])),
            ("eth_call", calldata.clone()),
            ("eth_estimateGas", calldata),
            (
                "eth_getCode",
                json!([evm::address_hex(&evm::LAUNCHPAD_PRECOMPILE), "latest"]),
            ),
            (
                "px_getHistory",
                json!(["0x0000000000000000000000000000000000001015"]),
            ),
        ] {
            assert_eq!(
                gate_with(
                    &cache,
                    Instant::now(),
                    untouched,
                    &id,
                    method,
                    Some(&params)
                ),
                None,
                "{method} {params}"
            );
        }
    }

    #[test]
    fn capabilities_method_is_published_in_openrpc() {
        let schema: Value = serde_json::from_slice(include_bytes!("../openrpc.json"))
            .unwrap_or_else(|e| panic!("{e}"));
        let methods = schema["methods"]
            .as_array()
            .unwrap_or_else(|| panic!("methods missing"));
        let entry = methods
            .iter()
            .find(|method| method["name"] == "px_getCapabilities")
            .unwrap_or_else(|| panic!("px_getCapabilities missing"));
        assert_eq!(entry["paramStructure"], "by-position");
        assert_eq!(entry["params"], json!([]));
        let result = &entry["result"]["schema"];
        assert_eq!(result["type"], "object");
        for field in ["exchange", "bridge", "launchpad"] {
            assert_eq!(result["properties"][field]["type"], "boolean", "{field}");
        }
        assert_eq!(result["properties"]["probed_at"]["type"], "integer");
        assert_eq!(result["properties"]["rpc_height"]["type"], "string");
        assert_eq!(
            result["required"],
            json!(["exchange", "bridge", "launchpad", "probed_at", "rpc_height"])
        );
        assert!(entry["errors"]
            .as_array()
            .unwrap_or_else(|| panic!("errors missing"))
            .iter()
            .any(|error| error["code"] == json!(-32001)));
        let description = entry["description"].as_str().unwrap_or_default();
        assert!(description.contains("surface_unavailable"));
        assert!(description.contains(&SURFACE_UNAVAILABLE.to_string()));
        for surface in Surface::ALL {
            assert!(description.contains(&evm::address_hex(&surface.address())));
        }
    }
}
