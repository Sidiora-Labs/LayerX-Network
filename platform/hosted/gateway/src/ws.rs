use super::{http, public_reads, rpc, ws_wire, Config, IncomingRequest};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Write;
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const MAX_SUBSCRIBERS: usize = 32;
const QUEUE_DEPTH: usize = 16;
const MAX_SUBSCRIPTIONS: usize = 8;

#[derive(Default)]
struct Hub {
    next_id: u64,
    clients: BTreeMap<u64, mpsc::SyncSender<Value>>,
}
static HUB: OnceLock<Mutex<Hub>> = OnceLock::new();
static WORKER: std::sync::Once = std::sync::Once::new();
static SOCKETS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

struct SocketGuard;
impl SocketGuard {
    fn acquire() -> Option<Self> {
        use std::sync::atomic::Ordering;
        SOCKETS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < MAX_SUBSCRIBERS).then_some(count + 1)
            })
            .ok()
            .map(|_| Self)
    }
}
impl Drop for SocketGuard {
    fn drop(&mut self) {
        SOCKETS.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
    }
}

impl Hub {
    fn register(&mut self) -> Result<(u64, mpsc::Receiver<Value>), String> {
        if self.clients.len() >= MAX_SUBSCRIBERS {
            return Err("subscription capacity exhausted".into());
        }
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or("subscription identifier exhausted")?;
        let (sender, receiver) = mpsc::sync_channel(QUEUE_DEPTH);
        self.clients.insert(self.next_id, sender);
        Ok((self.next_id, receiver))
    }
    fn publish(&mut self, value: &Value) {
        self.clients
            .retain(|_, sender| sender.try_send(value.clone()).is_ok());
    }
}

struct Registration(u64);
impl Drop for Registration {
    fn drop(&mut self) {
        if let Ok(mut hub) = HUB.get_or_init(Mutex::default).lock() {
            hub.clients.remove(&self.0);
        }
    }
}

fn feed(config: &Config, mut sequence: u64) -> Result<(), String> {
    let endpoint = config.public_core.as_ref().ok_or("core unavailable")?;
    loop {
        let answer = super::upstream_json(
            config,
            endpoint,
            config.component_token.as_str(),
            "GET",
            &format!("/internal/v1/receipt-events/{sequence}"),
            None,
            &[],
        )
        .map_err(|_| "receipt feed unavailable")?;
        if !matches!(answer.status, 200 | 202) || answer.content_type != "application/json" {
            return Err("receipt feed refused".into());
        }
        let document: Value =
            serde_json::from_slice(&answer.body).map_err(|_| "invalid receipt feed")?;
        let event = if answer.status == 200 {
            let receipt = &document["result"];
            if receipt["global_sequence"].as_u64() != Some(sequence)
                || receipt["receipt"].as_str().is_none_or(str::is_empty)
            {
                return Err("invalid receipt sequence".into());
            }
            sequence = sequence
                .checked_add(1)
                .ok_or("receipt sequence exhausted")?;
            receipt.clone()
        } else {
            if document["result"]["state"] != "pending" {
                return Err("invalid receipt wait".into());
            }
            Value::Null
        };
        let mut hub = HUB
            .get_or_init(Mutex::default)
            .lock()
            .map_err(|_| "subscription lock unavailable")?;
        hub.publish(&event);
    }
}

fn start_feed(config: &Arc<Config>) -> Result<(), String> {
    let node = rpc::read_result(config, "/v1/node-info").ok_or("node unavailable")?;
    let sequence = node["chain_head_sequence"]
        .as_str()
        .and_then(|s| s.parse::<u64>().ok())
        .and_then(|n| n.checked_add(1))
        .ok_or("invalid node head")?;
    WORKER.call_once(|| {
        let config = Arc::clone(config);
        std::thread::spawn(move || {
            let mut next = sequence;
            loop {
                if feed(&config, next).is_err() {
                    if let Ok(mut hub) = HUB.get_or_init(Mutex::default).lock() {
                        hub.clients.clear();
                    }
                    std::thread::sleep(Duration::from_secs(1));
                    if let Some(node) = rpc::read_result(&config, "/v1/node-info") {
                        if let Some(sequence) = node["chain_head_sequence"]
                            .as_str()
                            .and_then(|s| s.parse::<u64>().ok())
                            .and_then(|n| n.checked_add(1))
                        {
                            next = sequence;
                        }
                    }
                }
            }
        });
    });
    Ok(())
}

#[derive(Clone, Debug, PartialEq)]
enum Topic {
    Receipts,
    Checkpoints,
    Account(String),
}

fn topic(params: Option<&Value>) -> Result<Topic, i32> {
    let Some(Value::Array(args)) = params else {
        return Err(-32602);
    };
    match args.as_slice() {
        [Value::String(name)] if name == "receipts" => Ok(Topic::Receipts),
        [Value::String(name)] if name == "checkpoints" => Ok(Topic::Checkpoints),
        [Value::String(name), Value::String(account)]
            if name == "account"
                && super::parse_hex32(account).is_ok()
                && account != &"00".repeat(32) =>
        {
            Ok(Topic::Account(account.to_ascii_lowercase()))
        }
        _ => Err(-32602),
    }
}

fn allowed(scopes: &str, topic: &Topic) -> bool {
    scopes.split(',').any(|scope| {
        scope
            == match topic {
                Topic::Receipts => "receipt:read",
                _ => "state:read",
            }
    })
}

struct Subscription {
    topic: Topic,
    last: Option<Value>,
}

fn command(
    config: &Config,
    request: &IncomingRequest,
    value: &Value,
    subscriptions: &mut Vec<Subscription>,
) -> Option<Value> {
    if value.get("method").and_then(Value::as_str) != Some("lx_subscribe") {
        return rpc::dispatch(config, request, value);
    }
    if let Some(error) = rpc::invalid_request(value) {
        return Some(error);
    }
    let id = value.get("id")?;
    let topic = match topic(value.get("params")) {
        Ok(topic) => topic,
        Err(code) => return Some(rpc::error(id, code, "Invalid params")),
    };
    let Ok(record) = super::authenticate_key(config, request) else {
        return Some(rpc::error(id, -32002, "Authentication required"));
    };
    if !allowed(&record.scopes, &topic) {
        return Some(rpc::error(id, -32002, "Insufficient scope"));
    }
    if subscriptions.len() >= MAX_SUBSCRIPTIONS {
        return Some(rpc::error(id, -32005, "Subscription limit"));
    }
    subscriptions.push(Subscription { topic, last: None });
    Some(json!({"jsonrpc":"2.0","id":id,"result":subscriptions.len().to_string()}))
}

fn notification(
    config: &Config,
    subscription: &mut Subscription,
    receipt: &Value,
) -> Option<Value> {
    let value = match &subscription.topic {
        Topic::Receipts => {
            if receipt.is_null() {
                return None;
            }
            receipt.clone()
        }
        Topic::Account(account) => {
            if receipt.is_null() {
                return None;
            }
            rpc::read_result(config, &format!("/v1/accounts/{account}/balance"))?
        }
        Topic::Checkpoints => {
            let node = rpc::read_result(config, "/v1/node-info")?;
            let checkpoint = node["latest_finalised_checkpoint"].as_str()?;
            if super::parse_hex32(checkpoint).is_err() || checkpoint == "00".repeat(32) {
                return None;
            }
            rpc::read_result(config, &format!("/v1/checkpoints/{checkpoint}"))?
        }
    };
    if subscription.last.as_ref() == Some(&value) {
        return None;
    }
    subscription.last = Some(value.clone());
    Some(value)
}

fn valid_upgrade(request: &IncomingRequest) -> Option<String> {
    let header = |name: &str| request.headers.get(name).map_or("", String::as_str);
    if request.method != "GET"
        || !request.body.is_empty()
        || !header("upgrade").eq_ignore_ascii_case("websocket")
        || !header("connection")
            .split(',')
            .any(|s| s.trim().eq_ignore_ascii_case("upgrade"))
        || header("sec-websocket-version") != "13"
        || request.headers.contains_key("origin")
    {
        return None;
    }
    ws_wire::accept(header("sec-websocket-key"))
}

pub(super) fn serve(
    config: &Arc<Config>,
    request: &IncomingRequest,
    stream: &mut rustls::StreamOwned<rustls::ServerConnection, std::net::TcpStream>,
) -> Result<(), String> {
    let Some(accept) = valid_upgrade(request) else {
        return http::write_response(
            stream,
            &super::response(400, "invalid_websocket_upgrade", None),
        );
    };
    let record = match super::authenticate_key(config, request) {
        Ok(record) => record,
        Err(answer) => return http::write_response(stream, &answer),
    };
    if ![Topic::Receipts, Topic::Checkpoints]
        .iter()
        .any(|topic| allowed(&record.scopes, topic))
    {
        return http::write_response(stream, &super::response(403, "insufficient_scope", None));
    }
    let Some(_socket) = SocketGuard::acquire() else {
        return http::write_response(stream, &super::response(429, "subscription_limit", Some(1)));
    };
    let (id, receiver) = {
        let mut hub = HUB
            .get_or_init(Mutex::default)
            .lock()
            .map_err(|_| "subscription lock unavailable")?;
        match hub.register() {
            Ok(value) => value,
            Err(_) => {
                return http::write_response(
                    stream,
                    &super::response(429, "subscription_limit", Some(1)),
                )
            }
        }
    };
    let _registration = Registration(id);
    start_feed(config)?;
    write!(stream, "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n").and_then(|()| stream.flush()).map_err(|e| e.to_string())?;
    stream
        .sock
        .set_read_timeout(Some(Duration::from_millis(50)))
        .map_err(|e| e.to_string())?;
    stream
        .sock
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;
    session(config, request, stream, &receiver)
}

fn session(
    config: &Config,
    request: &IncomingRequest,
    stream: &mut rustls::StreamOwned<rustls::ServerConnection, std::net::TcpStream>,
    receiver: &mpsc::Receiver<Value>,
) -> Result<(), String> {
    let mut reader = ws_wire::Reader::default();
    let mut subscriptions = Vec::new();
    let started = Instant::now();
    let mut last_input = Instant::now();
    let mut last_ping = Instant::now();
    loop {
        if started.elapsed() > Duration::from_secs(3600)
            || last_input.elapsed() > Duration::from_secs(60)
        {
            return ws_wire::write(stream, 8, &1000_u16.to_be_bytes());
        }
        match reader.read(stream) {
            Ok(Some((8, body))) => return ws_wire::write(stream, 8, &body),
            Ok(Some((9, body))) => {
                last_input = Instant::now();
                ws_wire::write(stream, 10, &body)?;
            }
            Ok(Some((10, _))) => last_input = Instant::now(),
            Ok(Some((1, body))) => {
                if super::authenticate_key(config, request).is_err() {
                    return ws_wire::write(stream, 8, &1008_u16.to_be_bytes());
                }
                last_input = Instant::now();
                if !public_reads::consume_read() {
                    return ws_wire::write(stream, 8, &1013_u16.to_be_bytes());
                }
                let answer = match serde_json::from_slice::<Value>(&body) {
                    Ok(value) => command(config, request, &value, &mut subscriptions),
                    Err(_) => Some(rpc::error(&Value::Null, -32700, "Parse error")),
                };
                if let Some(answer) = answer {
                    ws_wire::write(
                        stream,
                        1,
                        &serde_json::to_vec(&answer).map_err(|e| e.to_string())?,
                    )?;
                }
            }
            Ok(_) => (),
            Err(_) => return ws_wire::write(stream, 8, &1002_u16.to_be_bytes()),
        }
        match receiver.try_recv() {
            Ok(receipt) => {
                let Ok(record) = super::authenticate_key(config, request) else {
                    return ws_wire::write(stream, 8, &1008_u16.to_be_bytes());
                };
                for (index, subscription) in subscriptions.iter_mut().enumerate() {
                    if !allowed(&record.scopes, &subscription.topic) {
                        return ws_wire::write(stream, 8, &1008_u16.to_be_bytes());
                    }
                    if let Some(result) = notification(config, subscription, &receipt) {
                        let event = json!({"jsonrpc":"2.0","method":"lx_subscription","params":{"subscription":(index+1).to_string(),"result":result}});
                        ws_wire::write(
                            stream,
                            1,
                            &serde_json::to_vec(&event).map_err(|e| e.to_string())?,
                        )?;
                    }
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                return ws_wire::write(stream, 8, &1013_u16.to_be_bytes())
            }
            Err(mpsc::TryRecvError::Empty) => (),
        }
        if last_ping.elapsed() >= Duration::from_secs(5) {
            if super::authenticate_key(config, request).is_err() {
                return ws_wire::write(stream, 8, &1008_u16.to_be_bytes());
            }
            ws_wire::write(stream, 9, b"lx")?;
            last_ping = Instant::now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn socket_capacity_is_held_until_connection_closes() {
        let sockets: Vec<_> = (0..MAX_SUBSCRIBERS)
            .map(|_| SocketGuard::acquire().unwrap_or_else(|| panic!("slot")))
            .collect();
        assert!(SocketGuard::acquire().is_none());
        drop(sockets);
        assert!(SocketGuard::acquire().is_some());
    }

    #[test]
    fn bounded_fanout_disconnects_slow_consumers() {
        let mut hub = Hub::default();
        let (_, slow) = hub.register().unwrap_or_else(|e| panic!("{e}"));
        let (_, fast) = hub.register().unwrap_or_else(|e| panic!("{e}"));
        for n in 0..=QUEUE_DEPTH {
            let value = json!(n);
            hub.publish(&value);
            assert_eq!(fast.try_recv(), Ok(value));
        }
        assert_eq!(hub.clients.len(), 1);
        for _ in 0..QUEUE_DEPTH {
            assert!(slow.try_recv().is_ok());
        }
        assert_eq!(slow.try_recv(), Err(mpsc::TryRecvError::Disconnected));
        let receivers: Vec<_> = (1..MAX_SUBSCRIBERS)
            .map(|_| hub.register().unwrap_or_else(|e| panic!("{e}")).1)
            .collect();
        assert!(hub.register().is_err());
        drop(receivers);
    }
    #[test]
    fn upgrade_rejects_wrong_version_origin_body_and_connection() {
        let mut request = IncomingRequest {
            method: "GET".into(),
            path: "/rpc/ws".into(),
            body: Vec::new(),
            headers: BTreeMap::from([
                ("upgrade".into(), "websocket".into()),
                ("connection".into(), "keep-alive, Upgrade".into()),
                ("sec-websocket-version".into(), "13".into()),
                (
                    "sec-websocket-key".into(),
                    "dGhlIHNhbXBsZSBub25jZQ==".into(),
                ),
            ]),
        };
        assert!(valid_upgrade(&request).is_some());
        for (header, invalid) in [
            ("upgrade", "http"),
            ("connection", "close"),
            ("sec-websocket-version", "12"),
            ("sec-websocket-key", "bad"),
        ] {
            let original = request.headers.insert(header.into(), invalid.into());
            assert!(valid_upgrade(&request).is_none());
            request
                .headers
                .insert(header.into(), original.unwrap_or_default());
        }
        request
            .headers
            .insert("origin".into(), "https://example.com".into());
        assert!(valid_upgrade(&request).is_none());
        request.headers.remove("origin");
        request.body.push(1);
        assert!(valid_upgrade(&request).is_none());
        request.body.clear();
        request.method = "POST".into();
        assert!(valid_upgrade(&request).is_none());
    }

    #[test]
    fn topics_and_scopes_are_exact() {
        assert_eq!(topic(Some(&json!(["receipts"]))), Ok(Topic::Receipts));
        assert_eq!(topic(Some(&json!(["checkpoints"]))), Ok(Topic::Checkpoints));
        assert_eq!(
            topic(Some(&json!(["account", "ab".repeat(32)]))),
            Ok(Topic::Account("ab".repeat(32)))
        );
        for args in [
            json!([]),
            json!(["account"]),
            json!(["account", "00".repeat(32)]),
            json!(["receipts", "x"]),
            json!(["unknown"]),
        ] {
            assert_eq!(topic(Some(&args)), Err(-32602));
        }
        assert!(allowed("receipt:read", &Topic::Receipts));
        assert!(!allowed("state:read", &Topic::Receipts));
        assert!(!allowed("activity:write", &Topic::Checkpoints));
    }
}
