use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tungstenite::{
    client::IntoClientRequest, protocol::WebSocketConfig, stream::MaybeTlsStream, Message,
    WebSocket,
};

use crate::programs::LayerXKeyCredential;
use crate::rpc::{encode_hex, RpcError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubscriptionTopic {
    Receipts,
    Checkpoints,
    Account,
}

pub struct RpcSubscription {
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
    id: String,
}

impl RpcSubscription {
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns an unverified notification, or None after an idle polling interval.
    /// Reconcile notifications through reads and verified commitment waits.
    /// # Errors
    /// Refuses binary frames, malformed events, mismatched subscriptions and lost connections.
    pub fn next_event(&mut self) -> Result<Option<Value>, RpcError> {
        receive_json(&mut self.socket)?
            .map(|value| notification(&value, &self.id))
            .transpose()
    }

    /// # Errors
    /// Reports a failed connection close.
    pub fn close(&mut self) -> Result<(), RpcError> {
        self.socket.close(None).map_err(|_| RpcError::Transport)
    }
}

fn parameters(topic: SubscriptionTopic, account: Option<[u8; 32]>) -> Result<Value, RpcError> {
    match (topic, account) {
        (SubscriptionTopic::Receipts, None) => Ok(json!(["receipts"])),
        (SubscriptionTopic::Checkpoints, None) => Ok(json!(["checkpoints"])),
        (SubscriptionTopic::Account, Some(account)) => Ok(json!(["account", encode_hex(&account)])),
        _ => Err(RpcError::InvalidRequest),
    }
}

pub(crate) fn connect(
    endpoint: &url::Url,
    credential: Option<&LayerXKeyCredential>,
    topic: SubscriptionTopic,
    account: Option<[u8; 32]>,
) -> Result<RpcSubscription, RpcError> {
    let params = parameters(topic, account)?;
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
            json!({"jsonrpc":"2.0", "id":"1", "method":"lx_subscribe", "params":params})
                .to_string()
                .into(),
        ))
        .map_err(|_| RpcError::Transport)?;
    let response = receive_json(&mut socket)?.ok_or(RpcError::Transport)?;
    let id = acknowledgement(&response)?;
    Ok(RpcSubscription { socket, id })
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

fn acknowledgement(value: &Value) -> Result<String, RpcError> {
    if value.as_object().is_none_or(|object| object.len() != 3)
        || value["jsonrpc"] != "2.0"
        || value["id"] != "1"
    {
        return Err(RpcError::InvalidResponse);
    }
    if let Some(error) = value.get("error") {
        return Err(RpcError::Remote {
            code: error["code"].as_i64().ok_or(RpcError::InvalidResponse)?,
            message: error["message"]
                .as_str()
                .ok_or(RpcError::InvalidResponse)?
                .to_owned(),
            data: error.get("data").cloned(),
        });
    }
    let id = value["result"]
        .as_str()
        .filter(|id| !id.is_empty() && id.len() <= 256)
        .ok_or(RpcError::InvalidResponse)?;
    Ok(id.to_owned())
}

fn notification(value: &Value, id: &str) -> Result<Value, RpcError> {
    if value.as_object().is_none_or(|object| object.len() != 3)
        || value["jsonrpc"] != "2.0"
        || value["method"] != "lx_subscription"
        || value["params"]
            .as_object()
            .is_none_or(|object| object.len() != 2)
        || value["params"]["subscription"] != id
        || !value["params"]["result"].is_object()
    {
        return Err(RpcError::InvalidResponse);
    }
    Ok(value["params"]["result"].clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn subscriptions_bind_selectors_acknowledgements_and_notifications() {
        assert!(parameters(SubscriptionTopic::Account, None).is_err());
        assert!(parameters(SubscriptionTopic::Receipts, Some([1; 32])).is_err());
        assert_eq!(
            parameters(SubscriptionTopic::Checkpoints, None).ok(),
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
        let event = json!({"jsonrpc":"2.0", "method":"lx_subscription", "params":{"subscription":"sub", "result":{"state":"pending"}}});
        assert_eq!(
            notification(&event, "sub").ok(),
            Some(json!({"state":"pending"}))
        );
        assert!(notification(&event, "another-subscription").is_err());
    }
}
