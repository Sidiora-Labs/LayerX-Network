#[path = "../../tests/support/tls_boundary.rs"]
mod tls_boundary;

#[test]
fn public_http_uses_system_trust_and_checks_server_identity() {
    tls_boundary::qualify(
        "public_http_uses_system_trust_and_checks_server_identity",
        |endpoint| {
            let value = layerx_platform_cli::http::Client::new(endpoint, None)?.get("/livez")?;
            serde_json::to_vec(&value).map_err(|error| error.to_string())
        },
    );
}

#[test]
fn websocket_client_preserves_frame_bounds_and_server_mask_refusal() {
    use std::io::Cursor;
    use tungstenite::protocol::{Role, WebSocketConfig};
    use tungstenite::{Message, WebSocket};

    let config = WebSocketConfig::default()
        .max_frame_size(Some(16))
        .max_message_size(Some(16));
    let mut valid = WebSocket::from_raw_socket(
        Cursor::new(vec![0x81, 2, b'o', b'k']),
        Role::Client,
        Some(config),
    );
    assert_eq!(
        valid
            .read()
            .unwrap_or_else(|error| panic!("canonical server frame: {error}")),
        Message::Text("ok".into())
    );
    let mut oversized =
        WebSocket::from_raw_socket(Cursor::new(vec![0x81, 17]), Role::Client, Some(config));
    assert!(matches!(
        oversized.read(),
        Err(tungstenite::Error::Capacity(_))
    ));
    let mut masked = WebSocket::from_raw_socket(
        Cursor::new(vec![0x81, 0x82, 0, 0, 0, 0, b'o', b'k']),
        Role::Client,
        Some(config),
    );
    assert!(matches!(
        masked.read(),
        Err(tungstenite::Error::Protocol(_))
    ));
}
