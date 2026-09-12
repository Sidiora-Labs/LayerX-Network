use super::{http, public_reads, rpc, ws_wire, Config, IncomingRequest};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Write;
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const MAX_SUBSCRIBERS: usize = 32;
const QUEUE_DEPTH: usize = 16;
const MAX_SUBSCRIPTIONS: usize = 8;
const MAX_RESUME: u64 = 16;

#[derive(Default)]
struct Hub {
    next_id: u64,
    clients: BTreeMap<u64, mpsc::SyncSender<(u64, Value)>>,
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
    fn register(&mut self) -> Result<(u64, mpsc::Receiver<(u64, Value)>), String> {
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
    fn publish(&mut self, sequence: u64, value: &Value) {
        self.clients
            .retain(|_, sender| sender.try_send((sequence, value.clone())).is_ok());
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

fn receipt_event(config: &Config, sequence: u64) -> Result<Option<Value>, String> {
    let endpoint = config.public_core.as_ref().ok_or("core unavailable")?;
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
    if answer.status == 202 {
        if document["result"]["state"] != "pending" {
            return Err("invalid receipt wait".into());
        }
        return Ok(None);
    }
    let receipt = &document["result"];
    if receipt["global_sequence"].as_u64() != Some(sequence)
        || receipt["receipt"].as_str().is_none_or(str::is_empty)
    {
        return Err("invalid receipt sequence".into());
    }
    Ok(Some(receipt.clone()))
}

fn feed(config: &Config, mut sequence: u64) -> Result<(), String> {
    loop {
        let (delivered, event) = match receipt_event(config, sequence)? {
            Some(receipt) => {
                let delivered = sequence;
                sequence = sequence
                    .checked_add(1)
                    .ok_or("receipt sequence exhausted")?;
                (delivered, receipt)
            }
            None => {
                std::thread::sleep(Duration::from_millis(100));
                (sequence.saturating_sub(1), Value::Null)
            }
        };
        let mut hub = HUB
            .get_or_init(Mutex::default)
            .lock()
            .map_err(|_| "subscription lock unavailable")?;
        hub.publish(delivered, &event);
    }
}

fn start_feed(config: &Arc<Config>) -> Result<(), String> {
    let sequence = head_sequence(config)
        .and_then(|head| head.checked_add(1))
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
                    if let Some(sequence) =
                        head_sequence(&config).and_then(|head| head.checked_add(1))
                    {
                        next = sequence;
                    }
                }
            }
        });
    });
    Ok(())
}

fn head_sequence(config: &Config) -> Option<u64> {
    rpc::read_result(config, "/v1/node-info")?["chain_head_sequence"]
        .as_str()
        .and_then(sequence_value)
}

fn sequence_value(text: &str) -> Option<u64> {
    let value: u64 = text.parse().ok()?;
    (value.to_string() == text).then_some(value)
}

#[derive(Clone, Debug, PartialEq)]
enum Topic {
    Receipts,
    Checkpoints,
    Account(String),
}

fn account_selector(account: &str) -> bool {
    super::parse_hex32(account).is_ok() && account != "00".repeat(32)
}

fn selector(params: Option<&Value>) -> Result<(Topic, Option<u64>), i32> {
    let Some(Value::Array(args)) = params else {
        return Err(-32602);
    };
    let (topic, cursor) = match args.as_slice() {
        [Value::String(name)] if name == "receipts" => (Topic::Receipts, None),
        [Value::String(name)] if name == "checkpoints" => (Topic::Checkpoints, None),
        [Value::String(name), Value::String(cursor)] if name == "receipts" => {
            (Topic::Receipts, Some(cursor))
        }
        [Value::String(name), Value::String(cursor)] if name == "checkpoints" => {
            (Topic::Checkpoints, Some(cursor))
        }
        [Value::String(name), Value::String(account)]
            if name == "account" && account_selector(account) =>
        {
            (Topic::Account(account.to_ascii_lowercase()), None)
        }
        [Value::String(name), Value::String(account), Value::String(cursor)]
            if name == "account" && account_selector(account) =>
        {
            (Topic::Account(account.to_ascii_lowercase()), Some(cursor))
        }
        _ => return Err(-32602),
    };
    match cursor {
        None => Ok((topic, None)),
        Some(text) => Ok((topic, Some(sequence_value(text).ok_or(-32602)?))),
    }
}

fn cancellation(params: Option<&Value>) -> Result<u64, i32> {
    let Some(Value::Array(args)) = params else {
        return Err(-32602);
    };
    match args.as_slice() {
        [Value::String(subscription)] => sequence_value(subscription).ok_or(-32602),
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
    id: u64,
    topic: Topic,
    last: Option<Value>,
    cursor: Option<u64>,
}

#[derive(Default)]
struct Subscriptions {
    next: u64,
    active: Vec<Subscription>,
}

fn cancel(subscriptions: &mut Subscriptions, subscription: u64) -> bool {
    let before = subscriptions.active.len();
    subscriptions
        .active
        .retain(|entry| entry.id != subscription);
    subscriptions.active.len() != before
}

fn resume_window(topic: &Topic, cursor: u64, head: u64) -> Result<Vec<u64>, i32> {
    if cursor > head {
        return Err(-32602);
    }
    if head.saturating_sub(cursor) > MAX_RESUME {
        return Err(-32005);
    }
    if head == cursor {
        return Ok(Vec::new());
    }
    if matches!(topic, Topic::Receipts) {
        return Ok((cursor.saturating_add(1)..=head).collect());
    }
    Ok(vec![head])
}

fn resume(
    config: &Config,
    subscription: &mut Subscription,
    cursor: u64,
    resumed: &mut Vec<Value>,
) -> Result<(), i32> {
    let head = head_sequence(config).ok_or(-32001)?;
    for sequence in resume_window(&subscription.topic, cursor, head)? {
        let receipt = receipt_event(config, sequence)
            .map_err(|_| -32001)?
            .ok_or(-32001)?;
        if let Some(result) = notification(config, subscription, &receipt) {
            resumed.push(event(subscription.id, &result, sequence));
        }
        subscription.cursor = Some(sequence);
    }
    Ok(())
}

fn command(
    config: &Config,
    request: &IncomingRequest,
    value: &Value,
    subscriptions: &mut Subscriptions,
    resumed: &mut Vec<Value>,
) -> Option<Value> {
    let method = value.get("method").and_then(Value::as_str);
    if !matches!(method, Some("lx_subscribe" | "lx_unsubscribe")) {
        return rpc::dispatch(config, request, value);
    }
    if let Some(error) = rpc::invalid_request(value) {
        return Some(error);
    }
    let id = value.get("id")?;
    if method == Some("lx_unsubscribe") {
        return Some(match cancellation(value.get("params")) {
            Ok(subscription) => {
                if cancel(subscriptions, subscription) {
                    json!({"jsonrpc":"2.0","id":id,"result":true})
                } else {
                    rpc::error(id, -32602, "Unknown subscription")
                }
            }
            Err(code) => rpc::error(id, code, "Invalid params"),
        });
    }
    let (topic, cursor) = match selector(value.get("params")) {
        Ok(selection) => selection,
        Err(code) => return Some(rpc::error(id, code, "Invalid params")),
    };
    let Ok(record) = super::authenticate_key(config, request) else {
        return Some(rpc::error(id, -32002, "Authentication required"));
    };
    if !allowed(&record.scopes, &topic) {
        return Some(rpc::error(id, -32002, "Insufficient scope"));
    }
    if subscriptions.active.len() >= MAX_SUBSCRIPTIONS {
        return Some(rpc::error(id, -32005, "Subscription limit"));
    }
    let Some(next) = subscriptions.next.checked_add(1) else {
        return Some(rpc::error(id, -32005, "Subscription limit"));
    };
    let mut subscription = Subscription {
        id: next,
        topic,
        last: None,
        cursor,
    };
    if let Some(cursor) = cursor {
        if let Err(code) = resume(config, &mut subscription, cursor, resumed) {
            resumed.clear();
            return Some(rpc::error(id, code, resume_refusal(code)));
        }
    }
    subscriptions.next = next;
    subscriptions.active.push(subscription);
    Some(json!({"jsonrpc":"2.0","id":id,"result":next.to_string()}))
}

fn resume_refusal(code: i32) -> &'static str {
    match code {
        -32005 => "Resume window exceeded",
        -32001 => "Read unavailable",
        _ => "Invalid params",
    }
}

fn event(subscription: u64, result: &Value, cursor: u64) -> Value {
    json!({"jsonrpc":"2.0","method":"lx_subscription","params":{"subscription":subscription.to_string(),"result":result,"cursor":cursor.to_string()}})
}

fn advance(cursor: Option<u64>, sequence: u64, receipt: &Value) -> Option<u64> {
    if !receipt.is_null() && cursor.is_some_and(|delivered| sequence <= delivered) {
        return None;
    }
    Some(cursor.map_or(sequence, |delivered| delivered.max(sequence)))
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
    receiver: &mpsc::Receiver<(u64, Value)>,
) -> Result<(), String> {
    let mut reader = ws_wire::Reader::default();
    let mut subscriptions = Subscriptions::default();
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
                let mut resumed = Vec::new();
                let answer = match serde_json::from_slice::<Value>(&body) {
                    Ok(value) => command(config, request, &value, &mut subscriptions, &mut resumed),
                    Err(_) => Some(rpc::error(&Value::Null, -32700, "Parse error")),
                };
                if let Some(answer) = answer {
                    ws_wire::write(
                        stream,
                        1,
                        &serde_json::to_vec(&answer).map_err(|e| e.to_string())?,
                    )?;
                }
                for replayed in resumed {
                    ws_wire::write(
                        stream,
                        1,
                        &serde_json::to_vec(&replayed).map_err(|e| e.to_string())?,
                    )?;
                }
            }
            Ok(_) => (),
            Err(_) => return ws_wire::write(stream, 8, &1002_u16.to_be_bytes()),
        }
        match receiver.try_recv() {
            Ok((sequence, receipt)) => {
                let Ok(record) = super::authenticate_key(config, request) else {
                    return ws_wire::write(stream, 8, &1008_u16.to_be_bytes());
                };
                for subscription in &mut subscriptions.active {
                    if !allowed(&record.scopes, &subscription.topic) {
                        return ws_wire::write(stream, 8, &1008_u16.to_be_bytes());
                    }
                    let Some(position) = advance(subscription.cursor, sequence, &receipt) else {
                        continue;
                    };
                    if let Some(result) = notification(config, subscription, &receipt) {
                        ws_wire::write(
                            stream,
                            1,
                            &serde_json::to_vec(&event(subscription.id, &result, position))
                                .map_err(|e| e.to_string())?,
                        )?;
                    }
                    subscription.cursor = Some(position);
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
            let sequence = u64::try_from(n).unwrap_or_else(|e| panic!("{e}"));
            let value = json!(n);
            hub.publish(sequence, &value);
            assert_eq!(fast.try_recv(), Ok((sequence, value)));
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
        assert_eq!(
            selector(Some(&json!(["receipts"]))),
            Ok((Topic::Receipts, None))
        );
        assert_eq!(
            selector(Some(&json!(["checkpoints"]))),
            Ok((Topic::Checkpoints, None))
        );
        assert_eq!(
            selector(Some(&json!(["account", "ab".repeat(32)]))),
            Ok((Topic::Account("ab".repeat(32)), None))
        );
        for args in [
            json!([]),
            json!(["account"]),
            json!(["account", "00".repeat(32)]),
            json!(["receipts", "x"]),
            json!(["unknown"]),
        ] {
            assert_eq!(selector(Some(&args)), Err(-32602));
        }
        assert!(allowed("receipt:read", &Topic::Receipts));
        assert!(!allowed("state:read", &Topic::Receipts));
        assert!(!allowed("activity:write", &Topic::Checkpoints));
    }

    #[test]
    fn resume_cursors_are_canonical_decimal_positions() {
        assert_eq!(
            selector(Some(&json!(["receipts", "41"]))),
            Ok((Topic::Receipts, Some(41)))
        );
        assert_eq!(
            selector(Some(&json!(["checkpoints", "0"]))),
            Ok((Topic::Checkpoints, Some(0)))
        );
        assert_eq!(
            selector(Some(&json!(["account", "ab".repeat(32), "7"]))),
            Ok((Topic::Account("ab".repeat(32)), Some(7)))
        );
        for args in [
            json!(["receipts", "07"]),
            json!(["receipts", ""]),
            json!(["receipts", "-1"]),
            json!(["receipts", "+1"]),
            json!(["receipts", "18446744073709551616"]),
            json!(["receipts", 41]),
            json!(["checkpoints", "1", "2"]),
            json!(["account", "ab".repeat(32), "07"]),
            json!(["account", "ab".repeat(32), "1", "2"]),
        ] {
            assert_eq!(selector(Some(&args)), Err(-32602));
        }
    }

    #[test]
    fn resume_windows_are_bounded_and_topic_shaped() {
        assert_eq!(resume_window(&Topic::Receipts, 4, 7), Ok(vec![5, 6, 7]));
        assert_eq!(resume_window(&Topic::Receipts, 7, 7), Ok(Vec::new()));
        assert_eq!(
            resume_window(&Topic::Receipts, 0, MAX_RESUME),
            Ok((1..=MAX_RESUME).collect::<Vec<_>>())
        );
        assert_eq!(
            resume_window(&Topic::Receipts, 0, MAX_RESUME + 1),
            Err(-32005)
        );
        assert_eq!(resume_window(&Topic::Receipts, 8, 7), Err(-32602));
        assert_eq!(resume_window(&Topic::Checkpoints, 4, 7), Ok(vec![7]));
        assert_eq!(
            resume_window(&Topic::Account("ab".repeat(32)), 4, 7),
            Ok(vec![7])
        );
        assert_eq!(
            resume_window(&Topic::Account("ab".repeat(32)), 7, 7),
            Ok(Vec::new())
        );
    }

    #[test]
    fn cursors_skip_replayed_receipts_and_track_idle_wakes() {
        let receipt = json!({"global_sequence":9});
        assert_eq!(advance(None, 9, &receipt), Some(9));
        assert_eq!(advance(Some(4), 9, &receipt), Some(9));
        assert_eq!(advance(Some(9), 9, &receipt), None);
        assert_eq!(advance(Some(12), 9, &receipt), None);
        assert_eq!(advance(None, 9, &Value::Null), Some(9));
        assert_eq!(advance(Some(9), 9, &Value::Null), Some(9));
        assert_eq!(advance(Some(12), 9, &Value::Null), Some(12));
    }

    #[test]
    fn cancellations_name_one_canonical_subscription() {
        assert_eq!(cancellation(Some(&json!(["3"]))), Ok(3));
        for args in [
            json!([]),
            json!(["3", "4"]),
            json!([3]),
            json!(["03"]),
            json!([""]),
        ] {
            assert_eq!(cancellation(Some(&args)), Err(-32602));
        }
        assert_eq!(
            cancellation(Some(&json!({"subscription":"3"}))),
            Err(-32602)
        );
        assert_eq!(cancellation(None), Err(-32602));
    }

    #[test]
    fn unsubscribe_releases_one_identifier_without_renumbering_the_others() {
        let mut subscriptions = Subscriptions {
            next: 3,
            active: vec![
                Subscription {
                    id: 1,
                    topic: Topic::Receipts,
                    last: None,
                    cursor: Some(4),
                },
                Subscription {
                    id: 2,
                    topic: Topic::Checkpoints,
                    last: None,
                    cursor: None,
                },
                Subscription {
                    id: 3,
                    topic: Topic::Account("ab".repeat(32)),
                    last: None,
                    cursor: None,
                },
            ],
        };
        assert!(cancel(&mut subscriptions, 2));
        assert!(!cancel(&mut subscriptions, 2));
        assert!(!cancel(&mut subscriptions, 4));
        assert_eq!(
            subscriptions
                .active
                .iter()
                .map(|entry| entry.id)
                .collect::<Vec<_>>(),
            vec![1, 3]
        );
        assert_eq!(subscriptions.next, 3);
        assert_eq!(subscriptions.active[0].cursor, Some(4));
        assert!(cancel(&mut subscriptions, 1));
        assert!(cancel(&mut subscriptions, 3));
        assert!(subscriptions.active.is_empty());
        assert_eq!(subscriptions.next, 3);
    }

    #[test]
    fn notifications_carry_the_subscription_and_its_cursor() {
        let frame = event(3, &json!({"activity_id":"ab"}), 41);
        assert_eq!(
            frame,
            json!({"jsonrpc":"2.0","method":"lx_subscription","params":{"subscription":"3","result":{"activity_id":"ab"},"cursor":"41"}})
        );
        assert_eq!(sequence_value("41"), Some(41));
        assert_eq!(sequence_value("041"), None);
        assert_eq!(sequence_value("0"), Some(0));
        assert_eq!(resume_refusal(-32005), "Resume window exceeded");
        assert_eq!(resume_refusal(-32001), "Read unavailable");
        assert_eq!(resume_refusal(-32602), "Invalid params");
    }
}
