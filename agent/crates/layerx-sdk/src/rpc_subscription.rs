use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

use layerx_types::clock::{Clock, Deadline};

use serde_json::{json, Value};
use tungstenite::{
    client::IntoClientRequest, protocol::WebSocketConfig, stream::MaybeTlsStream, Message,
    WebSocket,
};

use crate::programs::LayerXKeyCredential;
use crate::rpc::{encode_hex, RpcError};

const SUBSCRIBE_ID: &str = "1";
const UNSUBSCRIBE_ID: &str = "2";
const MAX_PENDING_EVENTS: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubscriptionTopic {
    Receipts,
    Checkpoints,
    Account,
}

pub struct RpcSubscription {
    socket: WebSocket<MaybeTlsStream<DeadlineStream>>,
    id: String,
    cursor: Option<u64>,
}

impl RpcSubscription {
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Feed position last delivered on this subscription, or the position it resumed from.
    /// Present it to `RpcClient::subscribe_from` after a lost connection to resume the topic.
    #[must_use]
    pub fn cursor(&self) -> Option<u64> {
        self.cursor
    }

    /// Returns an unverified notification, or None after an idle polling interval.
    /// Reconcile notifications through reads and verified commitment waits.
    /// # Errors
    /// Refuses binary frames, malformed events, mismatched subscriptions and lost connections.
    pub fn next_event(&mut self) -> Result<Option<Value>, RpcError> {
        let Some(value) = receive_json(&mut self.socket)? else {
            return Ok(None);
        };
        let (result, cursor) = notification(&value, &self.id)?;
        self.cursor = Some(cursor);
        Ok(Some(result))
    }

    /// Cancels this subscription on the server and confirms the acknowledgement. Notifications
    /// already in flight are discarded after advancing the cursor past them.
    /// # Errors
    /// Reports refused cancellations, malformed acknowledgements and lost connections.
    pub fn unsubscribe(&mut self) -> Result<(), RpcError> {
        transport(&mut self.socket)?.reset()?;
        self.socket
            .send(Message::Text(
                json!({"jsonrpc":"2.0", "id":UNSUBSCRIBE_ID, "method":"lx_unsubscribe", "params":[self.id.as_str()]})
                    .to_string()
                    .into(),
            ))
            .map_err(|_| RpcError::Transport)?;
        for _ in 0..=MAX_PENDING_EVENTS {
            let value = receive_until(&mut self.socket)?.ok_or(RpcError::Transport)?;
            if value.get("method").and_then(Value::as_str) == Some("lx_subscription") {
                let (_, cursor) = notification(&value, &self.id)?;
                self.cursor = Some(cursor);
                continue;
            }
            return cancellation(&value);
        }
        Err(RpcError::Transport)
    }

    /// # Errors
    /// Reports a failed connection close.
    pub fn close(&mut self) -> Result<(), RpcError> {
        transport(&mut self.socket)?.reset()?;
        self.socket.close(None).map_err(|_| RpcError::Transport)
    }
}

fn parameters(
    topic: SubscriptionTopic,
    account: Option<[u8; 32]>,
    cursor: Option<u64>,
) -> Result<Value, RpcError> {
    let mut params = match (topic, account) {
        (SubscriptionTopic::Receipts, None) => json!(["receipts"]),
        (SubscriptionTopic::Checkpoints, None) => json!(["checkpoints"]),
        (SubscriptionTopic::Account, Some(account)) => json!(["account", encode_hex(&account)]),
        _ => return Err(RpcError::InvalidRequest),
    };
    if let Some(cursor) = cursor {
        params
            .as_array_mut()
            .ok_or(RpcError::InvalidRequest)?
            .push(json!(cursor.to_string()));
    }
    Ok(params)
}

pub(crate) fn connect(
    endpoint: &url::Url,
    credential: Option<&LayerXKeyCredential>,
    tls: Option<std::sync::Arc<rustls::ClientConfig>>,
    clock: Arc<dyn Clock>,
    topic: SubscriptionTopic,
    account: Option<[u8; 32]>,
    cursor: Option<u64>,
) -> Result<RpcSubscription, RpcError> {
    let params = parameters(topic, account, cursor)?;
    let mut endpoint = endpoint.clone();
    endpoint.set_path(&format!("{}/ws", endpoint.path().trim_end_matches('/')));
    let secure = endpoint.scheme() == "https";
    endpoint
        .set_scheme(if secure { "wss" } else { "ws" })
        .map_err(|()| RpcError::InvalidRequest)?;
    let host = endpoint
        .host_str()
        .ok_or(RpcError::InvalidRequest)?
        .trim_start_matches('[')
        .trim_end_matches(']');
    let port = endpoint
        .port_or_known_default()
        .ok_or(RpcError::InvalidRequest)?;
    let mut deadline =
        Deadline::start(clock.as_ref(), Duration::from_secs(30)).map_err(RpcError::Clock)?;
    let mut stream = None;
    for address in resolve(
        host,
        port,
        deadline
            .remaining(clock.as_ref())
            .map_err(RpcError::Clock)?,
    )? {
        let remaining = deadline
            .remaining(clock.as_ref())
            .map_err(RpcError::Clock)?;
        if remaining.is_zero() {
            return Err(RpcError::Transport);
        }
        if let Ok(socket) = TcpStream::connect_timeout(&address, remaining) {
            stream = Some(socket);
            break;
        }
    }
    let stream = DeadlineStream {
        socket: stream.ok_or(RpcError::Transport)?,
        clock,
        deadline,
    };
    let mut request = endpoint
        .as_str()
        .into_client_request()
        .map_err(|_| RpcError::InvalidRequest)?;
    if let Some(credential) = credential {
        let value = credential
            .authorization()
            .map_err(RpcError::Configuration)?;
        request.headers_mut().insert(
            "Authorization",
            value.parse().map_err(|_| RpcError::InvalidRequest)?,
        );
    }
    let config = WebSocketConfig::default()
        .max_message_size(Some(9 * 1_048_576))
        .max_frame_size(Some(9 * 1_048_576))
        .max_write_buffer_size(262_144);
    let (mut socket, _) = tungstenite::client_tls_with_config(
        request,
        stream,
        Some(config),
        tls.map(tungstenite::Connector::Rustls),
    )
    .map_err(|_| RpcError::Transport)?;
    socket
        .send(Message::Text(
            json!({"jsonrpc":"2.0", "id":SUBSCRIBE_ID, "method":"lx_subscribe", "params":params})
                .to_string()
                .into(),
        ))
        .map_err(|_| RpcError::Transport)?;
    let response = receive_until(&mut socket)?.ok_or(RpcError::Transport)?;
    let id = acknowledgement(&response)?;
    Ok(RpcSubscription { socket, id, cursor })
}

fn receive_json(
    socket: &mut WebSocket<MaybeTlsStream<DeadlineStream>>,
) -> Result<Option<Value>, RpcError> {
    transport(socket)?.reset()?;
    receive_until(socket)
}

fn receive_until(
    socket: &mut WebSocket<MaybeTlsStream<DeadlineStream>>,
) -> Result<Option<Value>, RpcError> {
    while !transport(socket)?.remaining()?.is_zero() {
        match socket.read() {
            Ok(Message::Text(text)) => {
                return serde_json::from_str(&text)
                    .map(Some)
                    .map_err(|_| RpcError::InvalidResponse)
            }
            Ok(Message::Ping(_) | Message::Pong(_)) => {
                socket.flush().map_err(|_| RpcError::Transport)?;
            }
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return Ok(None)
            }
            Ok(Message::Close(_)) | Err(_) => return Err(RpcError::Transport),
            Ok(_) => return Err(RpcError::InvalidResponse),
        }
    }
    Ok(None)
}

struct DeadlineStream {
    socket: TcpStream,
    clock: Arc<dyn Clock>,
    deadline: Deadline,
}

impl DeadlineStream {
    fn reset(&mut self) -> Result<(), RpcError> {
        self.deadline = Deadline::start(self.clock.as_ref(), Duration::from_secs(30))
            .map_err(RpcError::Clock)?;
        Ok(())
    }

    fn remaining(&mut self) -> Result<Duration, RpcError> {
        self.deadline
            .remaining(self.clock.as_ref())
            .map_err(RpcError::Clock)
    }

    fn budget(&mut self) -> io::Result<Duration> {
        let remaining = self
            .remaining()
            .map_err(|_| io::Error::other("clock unavailable"))?;
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "subscription deadline reached",
            ));
        }
        Ok(remaining)
    }
}

impl Read for DeadlineStream {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let budget = self.budget()?;
        self.socket.set_read_timeout(Some(budget))?;
        let read = self.socket.read(bytes)?;
        self.budget()?;
        Ok(read)
    }
}

impl Write for DeadlineStream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let budget = self.budget()?;
        self.socket.set_write_timeout(Some(budget))?;
        let written = self.socket.write(bytes)?;
        self.budget()?;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.budget()?;
        self.socket.flush()
    }
}

fn transport(
    socket: &mut WebSocket<MaybeTlsStream<DeadlineStream>>,
) -> Result<&mut DeadlineStream, RpcError> {
    match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => Ok(stream),
        MaybeTlsStream::Rustls(stream) => Ok(&mut stream.sock),
        _ => Err(RpcError::Transport),
    }
}

fn resolve(host: &str, port: u16, budget: Duration) -> Result<Vec<std::net::SocketAddr>, RpcError> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static ACTIVE: AtomicUsize = AtomicUsize::new(0);
    struct Permit;
    impl Drop for Permit {
        fn drop(&mut self) {
            ACTIVE.fetch_sub(1, Ordering::AcqRel);
        }
    }
    ACTIVE
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
            (active < 16).then_some(active + 1)
        })
        .map_err(|_| RpcError::Transport)?;
    let permit = Permit;
    let host = host.to_owned();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("rpc-subscription-resolver".into())
        .spawn(move || {
            let _permit = permit;
            let result = (host.as_str(), port)
                .to_socket_addrs()
                .map(|addresses| addresses.take(16).collect());
            let _ = sender.send(result);
        })
        .map_err(|_| RpcError::Transport)?;
    receiver
        .recv_timeout(budget)
        .map_err(|_| RpcError::Transport)?
        .map_err(|_| RpcError::Transport)
}

fn remote(error: &Value) -> RpcError {
    let Some(code) = error["code"].as_i64() else {
        return RpcError::InvalidResponse;
    };
    let Some(message) = error["message"].as_str() else {
        return RpcError::InvalidResponse;
    };
    RpcError::Remote {
        code,
        message: message.to_owned(),
        data: error.get("data").cloned(),
    }
}

fn acknowledgement(value: &Value) -> Result<String, RpcError> {
    if value.as_object().is_none_or(|object| object.len() != 3)
        || value["jsonrpc"] != "2.0"
        || value["id"] != SUBSCRIBE_ID
    {
        return Err(RpcError::InvalidResponse);
    }
    if let Some(error) = value.get("error") {
        return Err(remote(error));
    }
    let id = value["result"]
        .as_str()
        .filter(|id| !id.is_empty() && id.len() <= 256)
        .ok_or(RpcError::InvalidResponse)?;
    Ok(id.to_owned())
}

fn cancellation(value: &Value) -> Result<(), RpcError> {
    if value.as_object().is_none_or(|object| object.len() != 3)
        || value["jsonrpc"] != "2.0"
        || value["id"] != UNSUBSCRIBE_ID
    {
        return Err(RpcError::InvalidResponse);
    }
    if let Some(error) = value.get("error") {
        return Err(remote(error));
    }
    if value["result"] != Value::Bool(true) {
        return Err(RpcError::InvalidResponse);
    }
    Ok(())
}

fn cursor_value(text: &str) -> Option<u64> {
    let value: u64 = text.parse().ok()?;
    (value.to_string() == text).then_some(value)
}

fn notification(value: &Value, id: &str) -> Result<(Value, u64), RpcError> {
    if value.as_object().is_none_or(|object| object.len() != 3)
        || value["jsonrpc"] != "2.0"
        || value["method"] != "lx_subscription"
        || value["params"]
            .as_object()
            .is_none_or(|object| object.len() != 3)
        || value["params"]["subscription"] != id
        || !value["params"]["result"].is_object()
    {
        return Err(RpcError::InvalidResponse);
    }
    let cursor = value["params"]["cursor"]
        .as_str()
        .and_then(cursor_value)
        .ok_or(RpcError::InvalidResponse)?;
    Ok((value["params"]["result"].clone(), cursor))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn subscriptions_bind_selectors_acknowledgements_and_notifications() {
        assert!(parameters(SubscriptionTopic::Account, None, None).is_err());
        assert!(parameters(SubscriptionTopic::Receipts, Some([1; 32]), None).is_err());
        assert_eq!(
            parameters(SubscriptionTopic::Checkpoints, None, None).ok(),
            Some(json!(["checkpoints"]))
        );
        assert_eq!(
            acknowledgement(&json!({"jsonrpc":"2.0", "id":"1", "result":"sub"}))
                .ok()
                .as_deref(),
            Some("sub")
        );
        assert!(acknowledgement(&json!({"jsonrpc":"2.0", "id":"2", "result":"sub"})).is_err());
        assert!(acknowledgement(
            &json!({"jsonrpc":"2.0", "id":"1", "result":{"state":"accepted"}})
        )
        .is_err());
        let event = json!({"jsonrpc":"2.0", "method":"lx_subscription", "params":{"subscription":"sub", "result":{"state":"pending"}, "cursor":"41"}});
        assert_eq!(
            notification(&event, "sub").ok(),
            Some((json!({"state":"pending"}), 41))
        );
        assert!(notification(&event, "another-subscription").is_err());
    }

    #[test]
    fn resume_selectors_carry_a_canonical_cursor_after_the_topic() {
        assert_eq!(
            parameters(SubscriptionTopic::Receipts, None, Some(41)).ok(),
            Some(json!(["receipts", "41"]))
        );
        assert_eq!(
            parameters(SubscriptionTopic::Checkpoints, None, Some(0)).ok(),
            Some(json!(["checkpoints", "0"]))
        );
        assert_eq!(
            parameters(SubscriptionTopic::Account, Some([0xab; 32]), Some(7)).ok(),
            Some(json!(["account", "ab".repeat(32), "7"]))
        );
        assert_eq!(cursor_value("41"), Some(41));
        assert_eq!(cursor_value("0"), Some(0));
        for text in ["041", "", "-1", "+1", "18446744073709551616", " 1"] {
            assert_eq!(cursor_value(text), None);
        }
    }

    #[test]
    fn notifications_without_a_canonical_cursor_are_refused() {
        for params in [
            json!({"subscription":"sub", "result":{"state":"pending"}}),
            json!({"subscription":"sub", "result":{"state":"pending"}, "cursor":41}),
            json!({"subscription":"sub", "result":{"state":"pending"}, "cursor":"041"}),
            json!({"subscription":"sub", "result":{"state":"pending"}, "cursor":""}),
            json!({"subscription":"sub", "result":"pending", "cursor":"41"}),
        ] {
            assert!(notification(
                &json!({"jsonrpc":"2.0", "method":"lx_subscription", "params":params}),
                "sub"
            )
            .is_err());
        }
    }

    #[test]
    fn cancellations_require_an_affirmative_acknowledgement() {
        assert!(cancellation(&json!({"jsonrpc":"2.0", "id":"2", "result":true})).is_ok());
        for value in [
            json!({"jsonrpc":"2.0", "id":"1", "result":true}),
            json!({"jsonrpc":"2.0", "id":"2", "result":false}),
            json!({"jsonrpc":"2.0", "id":"2", "result":"true"}),
            json!({"jsonrpc":"2.0", "id":"2"}),
        ] {
            assert!(cancellation(&value).is_err());
        }
        assert!(matches!(
            cancellation(
                &json!({"jsonrpc":"2.0", "id":"2", "error":{"code":-32602, "message":"Unknown subscription"}})
            ),
            Err(RpcError::Remote {
                code: -32602,
                ref message,
                data: None
            }) if message == "Unknown subscription"
        ));
    }
}
