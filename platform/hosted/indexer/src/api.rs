//! The read API: `/healthz`, `/v1/history/{account}`, `/v1/assets` and
//! `/v1/assets/{id}`, served over plain HTTP on loopback or over TLS.

use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use layerx_platform_internal::http::{json, read_http_message, refusal, write_response, Response};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use serde_json::json;

use crate::codec::percent_decode;
use crate::store::{Page, Store};
use crate::IndexError;

/// Default page size.
pub const DEFAULT_LIMIT: usize = 50;
/// Largest page size a client may ask for.
pub const MAXIMUM_LIMIT: usize = 500;

const MAXIMUM_REQUEST_BYTES: usize = 16 * 1024;
const MAXIMUM_CONNECTIONS: usize = 256;
const IO_TIMEOUT: Duration = Duration::from_secs(10);

static ACTIVE: AtomicUsize = AtomicUsize::new(0);

struct Query {
    cursor: Option<u64>,
    limit: usize,
    kind: Option<String>,
}

fn parse_query(query: Option<&str>, allow_kind: bool) -> Result<Query, Response> {
    let mut parsed = Query {
        cursor: None,
        limit: DEFAULT_LIMIT,
        kind: None,
    };
    let mut seen: Vec<String> = Vec::new();
    for pair in query
        .unwrap_or_default()
        .split('&')
        .filter(|pair| !pair.is_empty())
    {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        let name = percent_decode(name, true).map_err(|_| refusal(400, "invalid_query", None))?;
        let value = percent_decode(value, true).map_err(|_| refusal(400, "invalid_query", None))?;
        if seen.contains(&name) {
            return Err(refusal(400, "duplicate_query_parameter", None));
        }
        match name.as_str() {
            "cursor" => {
                parsed.cursor = Some(
                    value
                        .parse::<u64>()
                        .ok()
                        .filter(|cursor| *cursor > 0 && i64::try_from(*cursor).is_ok())
                        .ok_or_else(|| refusal(400, "invalid_cursor", None))?,
                );
            }
            "limit" => {
                parsed.limit = value
                    .parse::<usize>()
                    .ok()
                    .filter(|limit| (1..=MAXIMUM_LIMIT).contains(limit))
                    .ok_or_else(|| refusal(400, "invalid_limit", None))?;
            }
            "kind" if allow_kind => {
                if value.is_empty()
                    || value.len() > 64
                    || !value.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'
                    })
                {
                    return Err(refusal(400, "invalid_kind", None));
                }
                parsed.kind = Some(value);
            }
            _ => return Err(refusal(400, "unknown_query_parameter", None)),
        }
        seen.push(name);
    }
    Ok(parsed)
}

fn page(page: Page) -> Response {
    json(
        200,
        &json!({ "version": 1, "items": page.items, "next_cursor": page.next_cursor }),
    )
}

fn failure(error: &IndexError) -> Response {
    eprintln!("layerx-indexer request failed: {error}");
    refusal(503, "store_unavailable", Some(1))
}

fn identifier(segment: &str) -> Result<String, Response> {
    let decoded = percent_decode(segment, false).map_err(|_| refusal(400, "invalid_path", None))?;
    if decoded.is_empty()
        || decoded.len() > 256
        || decoded
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err(refusal(400, "invalid_path", None));
    }
    Ok(decoded)
}

/// Answers one request `method` `target` (path plus optional query).
#[must_use]
pub fn route(store: &Store, method: &str, target: &str) -> Response {
    if method != "GET" {
        return refusal(405, "method_not_allowed", None);
    }
    let (path, query) = target
        .split_once('?')
        .map_or((target, None), |(path, query)| (path, Some(query)));
    if path == "/healthz" {
        return match store.ping() {
            Ok(()) => json(200, &json!({ "status": "ok" })),
            Err(error) => failure(&error),
        };
    }
    if path == "/v1/assets" {
        let query = match parse_query(query, false) {
            Ok(query) => query,
            Err(response) => return response,
        };
        return store
            .assets(query.cursor, query.limit)
            .map_or_else(|error| failure(&error), page);
    }
    if let Some(segment) = path.strip_prefix("/v1/assets/") {
        if query.is_some_and(|query| !query.is_empty()) {
            return refusal(400, "unknown_query_parameter", None);
        }
        let asset = match identifier(segment) {
            Ok(asset) => asset,
            Err(response) => return response,
        };
        return match store.asset(&asset) {
            Ok(Some(document)) => json(200, &json!({ "version": 1, "asset": document })),
            Ok(None) => refusal(404, "asset_not_found", None),
            Err(error) => failure(&error),
        };
    }
    if let Some(segment) = path.strip_prefix("/v1/history/") {
        let account = match identifier(segment) {
            Ok(account) if !account.contains('/') => account.to_ascii_lowercase(),
            Ok(_) => return refusal(404, "not_found", None),
            Err(response) => return response,
        };
        let query = match parse_query(query, true) {
            Ok(query) => query,
            Err(response) => return response,
        };
        return store
            .history(&account, query.cursor, query.limit, query.kind.as_deref())
            .map_or_else(|error| failure(&error), page);
    }
    refusal(404, "not_found", None)
}

fn answer<S: std::io::Read + std::io::Write>(store: &Store, stream: &mut S) -> Result<(), String> {
    let response = match read_http_message(stream, MAXIMUM_REQUEST_BYTES) {
        Ok(request) => {
            let start = request.headers.get("").cloned().unwrap_or_default();
            let mut parts = start.split_whitespace();
            match (parts.next(), parts.next(), parts.next(), parts.next()) {
                (Some(method), Some(target), Some("HTTP/1.1"), None)
                    if target.starts_with('/') && request.headers.contains_key("host") =>
                {
                    route(store, method, target)
                }
                _ => refusal(400, "invalid_request", None),
            }
        }
        Err(_) => refusal(400, "invalid_request", None),
    };
    write_response(stream, &response)?;
    stream.flush().map_err(|error| error.to_string())
}

fn connection(
    store: &Store,
    tls: Option<&Arc<ServerConfig>>,
    tcp: TcpStream,
) -> Result<(), String> {
    tcp.set_read_timeout(Some(IO_TIMEOUT))
        .map_err(|error| error.to_string())?;
    tcp.set_write_timeout(Some(IO_TIMEOUT))
        .map_err(|error| error.to_string())?;
    match tls {
        None => {
            let mut tcp = tcp;
            answer(store, &mut tcp)
        }
        Some(config) => {
            let server =
                ServerConnection::new(Arc::clone(config)).map_err(|error| error.to_string())?;
            let mut stream = StreamOwned::new(server, tcp);
            answer(store, &mut stream)?;
            stream.conn.send_close_notify();
            let _ = stream.conn.write_tls(&mut stream.sock);
            Ok(())
        }
    }
}

/// Binds `listen`. Plain HTTP is only admitted on a loopback address.
///
/// # Errors
/// Refuses a non-loopback plaintext listener and bind failures.
pub fn bind(listen: SocketAddr, tls: bool) -> Result<TcpListener, IndexError> {
    if !tls && !listen.ip().is_loopback() {
        return Err(IndexError::Config(
            "plain HTTP listeners must be loopback; set LAYERX_INDEXER_TLS_CERT_DER".to_owned(),
        ));
    }
    TcpListener::bind(listen).map_err(|error| IndexError::Config(format!("bind {listen}: {error}")))
}

/// Serves the API on `listener` until it fails.
pub fn serve(listener: &TcpListener, store: &Arc<Store>, tls: Option<&Arc<ServerConfig>>) {
    for incoming in listener.incoming() {
        let tcp = match incoming {
            Ok(tcp) => tcp,
            Err(error) => {
                eprintln!("layerx-indexer accept failed: {error}");
                continue;
            }
        };
        if ACTIVE
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                (active < MAXIMUM_CONNECTIONS).then_some(active + 1)
            })
            .is_err()
        {
            continue;
        }
        let store = Arc::clone(store);
        let tls = tls.cloned();
        thread::spawn(move || {
            if let Err(error) = connection(&store, tls.as_ref(), tcp) {
                eprintln!("layerx-indexer connection failed: {error}");
            }
            ACTIVE.fetch_sub(1, Ordering::AcqRel);
        });
    }
}
