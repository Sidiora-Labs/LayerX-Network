use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

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
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
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
        self.socket
            .send(Message::Text(
                json!({"jsonrpc":"2.0", "id":UNSUBSCRIBE_ID, "method":"lx_unsubscribe", "params":[self.id.as_str()]})
                    .to_string()
                    .into(),
            ))
            .map_err(|_| RpcError::Transport)?;
        for _ in 0..=MAX_PENDING_EVENTS {
            let value = receive_json(&mut self.socket)?.ok_or(RpcError::Transport)?;
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
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut stream = None;
    for address in (host, port)
        .to_socket_addrs()
        .map_err(|_| RpcError::Transport)?
        .take(16)
    {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(RpcError::Transport);
        }
        if let Ok(socket) = TcpStream::connect_timeout(&address, remaining) {
            stream = Some(socket);
            break;
        }
    }
    let stream = stream.ok_or(RpcError::Transport)?;
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(|_| RpcError::Transport)?;
    stream
        .set_write_timeout(Some(Duration::from_secs(30)))
        .map_err(|_| RpcError::Transport)?;
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
    let (mut socket, _) = tungstenite::client_tls_with_config(request, stream, Some(config), None)
        .map_err(|_| RpcError::Transport)?;
    socket
        .send(Message::Text(
            json!({"jsonrpc":"2.0", "id":SUBSCRIBE_ID, "method":"lx_subscribe", "params":params})
                .to_string()
                .into(),
        ))
        .map_err(|_| RpcError::Transport)?;
    let response = receive_json(&mut socket)?.ok_or(RpcError::Transport)?;
    let id = acknowledgement(&response)?;
    Ok(RpcSubscription { socket, id, cursor })
}

fn receive_json(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
) -> Result<Option<Value>, RpcError> {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(None);
        }
        let stream = match socket.get_mut() {
            MaybeTlsStream::Plain(stream) => stream,
            MaybeTlsStream::NativeTls(stream) => stream.get_ref(),
            _ => return Err(RpcError::Transport),
        };
        stream
            .set_read_timeout(Some(remaining))
            .map_err(|_| RpcError::Transport)?;
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
