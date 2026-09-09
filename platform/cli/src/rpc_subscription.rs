use std::net::{TcpStream, ToSocketAddrs as _};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tungstenite::client::IntoClientRequest as _;
use tungstenite::{Message, WebSocket};
use zeroize::Zeroizing;

use crate::rpc::{decode_response, request};

type Socket = WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>>;

pub(crate) fn next(
    url: &str,
    credential: &str,
    params: &Value,
    timeout: Duration,
) -> Result<Value, String> {
    let command = request("lx_subscribe", params)?;
    if timeout.is_zero() || timeout > Duration::from_secs(300) {
        return Err("subscription timeout must be 1–300 seconds".into());
    }
    let deadline = Instant::now() + timeout;
    let mut socket = connect(url, credential, deadline)?;
    socket
        .send(Message::Text(command.to_string().into()))
        .map_err(|_| "subscription request failed".to_owned())?;
    let response = read_json(&mut socket, deadline)?;
    let subscription = decode_response("lx_subscribe", &response)?;
    let event = read_json(&mut socket, deadline)?;
    let result = notification(&event, &subscription)?;
    Ok(
        json!({"topic":params[0],"subscription":subscription,"verified":false,"notification":result}),
    )
}

fn connect(url: &str, credential: &str, deadline: Instant) -> Result<Socket, String> {
    let ws = if let Some(base) = url.strip_prefix("https://") {
        format!("wss://{base}/ws")
    } else if let Some(base) = url.strip_prefix("http://") {
        format!("ws://{base}/ws")
    } else {
        return Err("invalid subscription endpoint".into());
    };
    let mut upgrade = ws
        .into_client_request()
        .map_err(|_| "invalid subscription endpoint".to_owned())?;
    let host = upgrade.uri().host().ok_or("subscription host missing")?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let port = upgrade
        .uri()
        .port_u16()
        .unwrap_or(if url.starts_with("https://") { 443 } else { 80 });
    let addresses = (host, port)
        .to_socket_addrs()
        .map_err(|_| "subscription host resolution failed".to_owned())?;
    let mut connected = None;
    for address in addresses {
        if let Ok(stream) = TcpStream::connect_timeout(&address, remaining(deadline)?) {
            connected = Some(stream);
            break;
        }
    }
    let stream = connected.ok_or("subscription connection unavailable")?;
    stream
        .set_read_timeout(Some(remaining(deadline)?))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(remaining(deadline)?))
        .map_err(|e| e.to_string())?;
    let authorization = Zeroizing::new(format!("LayerX-Key {credential}"));
    let mut header = tungstenite::http::HeaderValue::from_str(&authorization)
        .map_err(|_| "invalid gateway authorization".to_owned())?;
    header.set_sensitive(true);
    upgrade.headers_mut().insert("Authorization", header);
    let config = tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(16 * 1024 * 1024))
        .max_frame_size(Some(16 * 1024 * 1024));
    tungstenite::client_tls_with_config(upgrade, stream, Some(config), None)
        .map(|(socket, _)| socket)
        .map_err(|_| {
            "subscription_upgrade_unavailable: authenticated WebSocket upgrade failed".to_owned()
        })
}

fn remaining(deadline: Instant) -> Result<Duration, String> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|value| !value.is_zero())
        .ok_or_else(|| "subscription_timeout: reconcile through RPC reads".to_owned())
}

fn read_json(socket: &mut Socket, deadline: Instant) -> Result<Value, String> {
    loop {
        let timeout = remaining(deadline)?;
        let stream = match socket.get_ref() {
            tungstenite::stream::MaybeTlsStream::Plain(stream) => stream,
            tungstenite::stream::MaybeTlsStream::NativeTls(stream) => stream.get_ref(),
            _ => return Err("unsupported subscription TLS stream".into()),
        };
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|e| e.to_string())?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(|e| e.to_string())?;
        match socket.read().map_err(|_| {
            "subscription_unavailable: stream lost or timed out; reconcile through RPC reads"
                .to_owned()
        })? {
            Message::Text(text) => return serde_json::from_str(&text).map_err(|e| e.to_string()),
            Message::Ping(_) => socket
                .flush()
                .map_err(|_| "subscription pong failed".to_owned())?,
            Message::Pong(_) => {}
            Message::Close(_) => {
                return Err(
                    "subscription_closed: reconcile through RPC reads before reconnecting".into(),
                )
            }
            _ => return Err("subscription returned a non-JSON frame".into()),
        }
    }
}

fn notification(event: &Value, subscription: &Value) -> Result<Value, String> {
    if event["jsonrpc"] != "2.0"
        || event["method"] != "lx_subscription"
        || event.get("id").is_some()
        || event.pointer("/params/subscription") != Some(subscription)
        || event.get("error").is_some()
    {
        return Err("invalid or unbound subscription notification".into());
    }
    event
        .pointer("/params/result")
        .filter(|result| result.is_object())
        .cloned()
        .ok_or_else(|| "subscription notification omitted object result".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notifications_are_bound_and_never_commitment_evidence() -> Result<(), String> {
        let event = json!({"jsonrpc":"2.0","method":"lx_subscription","params":{"subscription":"1","result":{"state":"pending"}}});
        assert_eq!(notification(&event, &json!("1"))?["state"], "pending");
        assert!(notification(&event, &json!("2")).is_err());
        for field in ["jsonrpc", "method", "params"] {
            let mut invalid = event.clone();
            invalid
                .as_object_mut()
                .ok_or("object missing")?
                .remove(field);
            assert!(notification(&invalid, &json!("1")).is_err());
        }
        let ack = json!({"jsonrpc":"2.0","id":1,"result":"1"});
        assert!(notification(&ack, &json!("1")).is_err());
        Ok(())
    }
}
