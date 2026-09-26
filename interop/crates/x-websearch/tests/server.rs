use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::thread;
use std::time::Duration;
use x_websearch::server::{QueryError, RouteError};
use x_websearch::{Limits, Request, Response, Route, RouteTable, RunningServer, Server};

const DIGEST: &str = "4f2c9a1e7b3d5f6071829304a5b6c7d8e9f0a1b2c3d4e5f60718293a4b5c6d7e";

fn test_limits() -> Limits {
    Limits {
        workers: 2,
        queue: 4,
        request_line_bytes: 256,
        header_bytes: 512,
        header_count: 8,
        read_timeout: Duration::from_millis(400),
        write_timeout: Duration::from_secs(2),
    }
}

fn start(limits: Limits, routes: RouteTable) -> std::io::Result<RunningServer> {
    let address: SocketAddr = "127.0.0.1:0"
        .parse()
        .map_err(|_| std::io::Error::other("address"))?;
    Server::bind(address, limits, routes)?.spawn()
}

fn filled_routes() -> Result<RouteTable, RouteError> {
    let mut routes = RouteTable::new();
    routes.set(Route::Search, |request: &Request| {
        match request.query_param("q") {
            Ok(Some(query)) => Response::json(
                200,
                serde_json::json!({ "q": query, "path": request.path })
                    .to_string()
                    .into_bytes(),
            ),
            Ok(None) => Response::error(400, "missing_query"),
            Err(QueryError::Duplicate) => Response::error(400, "duplicate_query"),
            Err(QueryError::Malformed) => Response::error(400, "malformed_query"),
        }
    })?;
    routes.set(Route::Content, |request: &Request| {
        let digest = request.digest.map_or_else(String::new, |digest| {
            digest.iter().fold(String::new(), |mut text, byte| {
                let _ = write!(text, "{byte:02x}");
                text
            })
        });
        Response::new(200, "application/octet-stream", digest.into_bytes()).with_header(
            "PAYMENT-RESPONSE",
            request.header("payment-signature").unwrap_or("none"),
        )
    })?;
    routes.set(Route::Fetch, |request: &Request| {
        assert!(
            request.query.as_deref() != Some("panic=1"),
            "handler failure"
        );
        Response::json(200, b"{}".to_vec()).with_header("X-Bad", "line\r\nInjected: yes")
    })?;
    Ok(routes)
}

struct Reply {
    status: u16,
    head: String,
    body: Vec<u8>,
}

impl Reply {
    fn header(&self, name: &str) -> Option<&str> {
        self.head.split("\r\n").skip(1).find_map(|line| {
            let (key, value) = line.split_once(": ")?;
            key.eq_ignore_ascii_case(name).then_some(value)
        })
    }
}

fn read_reply(stream: &mut TcpStream) -> Result<Reply, Box<dyn std::error::Error>> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes)?;
    let split = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or("no header terminator")?;
    let head = String::from_utf8(bytes[..split].to_vec())?;
    let body = bytes[split + 4..].to_vec();
    let status = head.split(' ').nth(1).ok_or("no status")?.parse::<u16>()?;
    let reply = Reply { status, head, body };
    let length: usize = reply.header("Content-Length").ok_or("length")?.parse()?;
    assert_eq!(length, reply.body.len());
    assert_eq!(reply.header("Connection"), Some("close"));
    Ok(reply)
}

fn exchange(address: SocketAddr, raw: &[u8]) -> Result<Reply, Box<dyn std::error::Error>> {
    let mut stream = TcpStream::connect(address)?;
    stream.write_all(raw)?;
    read_reply(&mut stream)
}

fn get(address: SocketAddr, target: &str) -> Result<Reply, Box<dyn std::error::Error>> {
    exchange(
        address,
        format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes(),
    )
}

fn error_code(reply: &Reply) -> Result<String, Box<dyn std::error::Error>> {
    let body: serde_json::Value = serde_json::from_slice(&reply.body)?;
    Ok(body["error"].as_str().ok_or("error code")?.to_owned())
}

#[test]
fn health_is_served_free_and_paid_routes_wait_for_handlers(
) -> Result<(), Box<dyn std::error::Error>> {
    let server = start(test_limits(), RouteTable::new())?;
    let address = server.local_addr();
    let health = get(address, "/health")?;
    assert_eq!(health.status, 200);
    assert!(health.head.starts_with("HTTP/1.1 200 OK\r\n"));
    assert_eq!(health.header("Content-Type"), Some("application/json"));
    assert_eq!(health.header("Cache-Control"), Some("no-store"));
    assert_eq!(health.body, b"{\"status\":\"ok\"}");
    let health_query = get(address, "/health?verbose=1")?;
    assert_eq!(health_query.status, 200);
    let legacy = exchange(address, b"GET /health HTTP/1.0\r\n\r\n")?;
    assert_eq!(legacy.status, 200);
    for target in ["/search?q=paxeer", "/fetch?url=https%3A%2F%2Fpaxeer.app%2F"] {
        let reply = get(address, target)?;
        assert_eq!(reply.status, 503, "{target}");
        assert_eq!(error_code(&reply)?, "route_unavailable");
    }
    let content = get(address, &format!("/content/{DIGEST}"))?;
    assert_eq!(content.status, 503);
    server.shutdown()?;
    Ok(())
}

#[test]
fn route_table_dispatches_filled_routes() -> Result<(), Box<dyn std::error::Error>> {
    let server = start(test_limits(), filled_routes()?)?;
    let address = server.local_addr();
    let search = get(address, "/search?q=paxeer+x%20network&lang=en")?;
    assert_eq!(search.status, 200);
    let body: serde_json::Value = serde_json::from_slice(&search.body)?;
    assert_eq!(body["q"], "paxeer x network");
    assert_eq!(body["path"], "/search");
    assert_eq!(error_code(&get(address, "/search")?)?, "missing_query");
    assert_eq!(
        error_code(&get(address, "/search?q=a&q=b")?)?,
        "duplicate_query"
    );
    assert_eq!(
        error_code(&get(address, "/search?q=%zz")?)?,
        "malformed_query"
    );
    assert_eq!(
        error_code(&get(address, "/search?q=%ff")?)?,
        "malformed_query"
    );
    let content = exchange(
        address,
        format!(
            "GET /content/{} HTTP/1.1\r\nHost: 127.0.0.1\r\nPayment-Signature: signed\r\n\r\n",
            DIGEST.to_ascii_uppercase()
        )
        .as_bytes(),
    )?;
    assert_eq!(content.status, 200);
    assert_eq!(
        content.header("Content-Type"),
        Some("application/octet-stream")
    );
    assert_eq!(content.header("PAYMENT-RESPONSE"), Some("signed"));
    assert_eq!(content.body, DIGEST.as_bytes());
    server.shutdown()?;
    Ok(())
}

#[test]
fn route_table_refuses_the_health_route_and_a_second_handler() {
    let mut routes = RouteTable::new();
    assert_eq!(
        routes.set(Route::Health, |_: &Request| Response::error(500, "x")),
        Err(RouteError::BuiltIn)
    );
    assert_eq!(
        routes.set(Route::Search, |_: &Request| Response::error(500, "x")),
        Ok(())
    );
    assert_eq!(
        routes.set(Route::Search, |_: &Request| Response::error(500, "x")),
        Err(RouteError::AlreadySet)
    );
    assert!(!Route::Health.is_paid());
    assert!(Route::Search.is_paid() && Route::Fetch.is_paid() && Route::Content.is_paid());
}

#[test]
fn unknown_paths_answer_404_and_other_methods_405() -> Result<(), Box<dyn std::error::Error>> {
    let server = start(test_limits(), filled_routes()?)?;
    let address = server.local_addr();
    for target in ["/", "/healthz", "/search/", "/content", "/Content/x"] {
        let reply = get(address, target)?;
        assert_eq!(reply.status, 404, "{target}");
        assert_eq!(error_code(&reply)?, "not_found");
    }
    let unknown_post = exchange(
        address,
        b"POST /nowhere HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
    )?;
    assert_eq!(unknown_post.status, 404);
    for method in ["POST", "PUT", "DELETE", "HEAD", "OPTIONS", "PATCH", "get"] {
        for target in [
            "/health",
            "/search?q=x",
            "/fetch",
            &format!("/content/{DIGEST}"),
        ] {
            let reply = exchange(
                address,
                format!(
                    "{method} {target} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\n\r\n"
                )
                .as_bytes(),
            )?;
            assert_eq!(reply.status, 405, "{method} {target}");
            assert_eq!(reply.header("Allow"), Some("GET"));
            assert_eq!(error_code(&reply)?, "method_not_allowed");
        }
    }
    server.shutdown()?;
    Ok(())
}

#[test]
fn malformed_requests_answer_400() -> Result<(), Box<dyn std::error::Error>> {
    let server = start(test_limits(), filled_routes()?)?;
    let address = server.local_addr();
    let cases: [(&[u8], &str); 16] = [
        (b"GARBAGE\r\n\r\n", "malformed_request"),
        (b"GET /health\r\n\r\n", "malformed_request"),
        (
            b"GET  /health HTTP/1.1\r\nHost: a\r\n\r\n",
            "malformed_request",
        ),
        (
            b"GET /health HTTP/2.0\r\nHost: a\r\n\r\n",
            "malformed_request",
        ),
        (
            b"GET health HTTP/1.1\r\nHost: a\r\n\r\n",
            "malformed_request",
        ),
        (
            b"GET /health#top HTTP/1.1\r\nHost: a\r\n\r\n",
            "malformed_request",
        ),
        (
            b"G(T /health HTTP/1.1\r\nHost: a\r\n\r\n",
            "malformed_request",
        ),
        (
            b"GET /health HTTP/1.1\r\nHost a\r\n\r\n",
            "malformed_request",
        ),
        (
            b"GET /health HTTP/1.1\r\nHo st: a\r\n\r\n",
            "malformed_request",
        ),
        (
            b"GET /health HTTP/1.1\r\nHost: a\x01\r\n\r\n",
            "malformed_request",
        ),
        (
            b"GET /h\xffalth HTTP/1.1\r\nHost: a\r\n\r\n",
            "malformed_request",
        ),
        (b"GET /health HTTP/1.1\r\n\r\n", "missing_host"),
        (
            b"GET /health HTTP/1.1\r\nHost: a\r\nhost: b\r\n\r\n",
            "duplicate_header",
        ),
        (
            b"GET /health HTTP/1.1\r\nHost: a\r\nContent-Length: 5\r\n\r\nhello",
            "body_not_accepted",
        ),
        (
            b"GET /health HTTP/1.1\r\nHost: a\r\nTransfer-Encoding: chunked\r\n\r\n",
            "body_not_accepted",
        ),
        (
            b"GET /content/abc HTTP/1.1\r\nHost: a\r\n\r\n",
            "malformed_digest",
        ),
    ];
    for (raw, code) in cases {
        let reply = exchange(address, raw)?;
        assert_eq!(reply.status, 400, "{}", String::from_utf8_lossy(raw));
        assert_eq!(
            error_code(&reply)?,
            code,
            "{}",
            String::from_utf8_lossy(raw)
        );
    }
    let prefixed = get(address, &format!("/content/0x{}", &DIGEST[..62]))?;
    assert_eq!(prefixed.status, 400);
    let trailing = exchange(
        address,
        b"GET /health HTTP/1.1\r\nHost: a\r\n\r\nGET /health HTTP/1.1\r\n\r\n",
    )?;
    assert_eq!(trailing.status, 400);
    assert_eq!(error_code(&trailing)?, "unexpected_bytes");
    assert_eq!(get(address, "/content/")?.status, 400);
    let truncated = {
        let mut stream = TcpStream::connect(address)?;
        stream.write_all(b"GET /health HTTP/1.1\r\nHost: a\r\n")?;
        stream.shutdown(std::net::Shutdown::Write)?;
        read_reply(&mut stream)?
    };
    assert_eq!(truncated.status, 400);
    server.shutdown()?;
    Ok(())
}

#[test]
fn request_line_header_and_count_bounds_are_enforced() -> Result<(), Box<dyn std::error::Error>> {
    let limits = test_limits();
    let server = start(limits, filled_routes()?)?;
    let address = server.local_addr();
    let fits = format!("/search?q={}", "a".repeat(limits.request_line_bytes - 24));
    let fitting = get(address, &fits)?;
    assert_eq!(fitting.status, 200);
    let long = format!("/search?q={}", "a".repeat(limits.request_line_bytes));
    let too_long = get(address, &long)?;
    assert_eq!(too_long.status, 414);
    assert_eq!(error_code(&too_long)?, "request_line_too_long");
    let unterminated = exchange(
        address,
        "A".repeat(limits.request_line_bytes * 4).as_bytes(),
    )?;
    assert_eq!(unterminated.status, 414);
    let big_header = format!(
        "GET /health HTTP/1.1\r\nHost: a\r\nX-Filler: {}\r\n\r\n",
        "b".repeat(limits.header_bytes)
    );
    let too_large = exchange(address, big_header.as_bytes())?;
    assert_eq!(too_large.status, 431);
    assert_eq!(error_code(&too_large)?, "headers_too_large");
    let mut many = String::from("GET /health HTTP/1.1\r\nHost: a\r\n");
    for index in 0..limits.header_count {
        write!(many, "X-H{index}: v\r\n")?;
    }
    many.push_str("\r\n");
    let too_many = exchange(address, many.as_bytes())?;
    assert_eq!(too_many.status, 431);
    let mut enough = String::from("GET /health HTTP/1.1\r\nHost: a\r\n");
    for index in 1..limits.header_count {
        write!(enough, "X-H{index}: v\r\n")?;
    }
    enough.push_str("\r\n");
    assert_eq!(exchange(address, enough.as_bytes())?.status, 200);
    server.shutdown()?;
    Ok(())
}

#[test]
fn a_slow_request_is_answered_408_within_the_read_limit() -> Result<(), Box<dyn std::error::Error>>
{
    let limits = test_limits();
    let server = start(limits, RouteTable::new())?;
    let address = server.local_addr();
    let started = std::time::Instant::now();
    let mut stream = TcpStream::connect(address)?;
    stream.write_all(b"GET /health HTTP/1.1\r\n")?;
    for _ in 0..3 {
        thread::sleep(limits.read_timeout / 4);
        stream.write_all(b"X")?;
    }
    let reply = read_reply(&mut stream)?;
    assert_eq!(reply.status, 408);
    assert_eq!(error_code(&reply)?, "request_timeout");
    assert!(started.elapsed() < limits.read_timeout * 3);
    let silent = {
        let mut stream = TcpStream::connect(address)?;
        read_reply(&mut stream)?
    };
    assert_eq!(silent.status, 408);
    server.shutdown()?;
    Ok(())
}

#[test]
fn a_full_worker_pool_answers_503() -> Result<(), Box<dyn std::error::Error>> {
    let limits = Limits {
        workers: 1,
        queue: 1,
        read_timeout: Duration::from_secs(3),
        ..test_limits()
    };
    let server = start(limits, RouteTable::new())?;
    let address = server.local_addr();
    let mut busy = TcpStream::connect(address)?;
    busy.write_all(b"GET /health HTTP/1.1\r\n")?;
    thread::sleep(Duration::from_millis(300));
    let mut queued = TcpStream::connect(address)?;
    thread::sleep(Duration::from_millis(300));
    let rejected = get(address, "/health")?;
    assert_eq!(rejected.status, 503);
    assert_eq!(error_code(&rejected)?, "server_busy");
    busy.write_all(b"Host: a\r\n\r\n")?;
    assert_eq!(read_reply(&mut busy)?.status, 200);
    queued.write_all(b"GET /health HTTP/1.1\r\nHost: a\r\n\r\n")?;
    assert_eq!(read_reply(&mut queued)?.status, 200);
    server.shutdown()?;
    Ok(())
}

#[test]
fn a_panicking_handler_or_an_invalid_header_answers_500() -> Result<(), Box<dyn std::error::Error>>
{
    let limits = Limits {
        workers: 1,
        ..test_limits()
    };
    let server = start(limits, filled_routes()?)?;
    let address = server.local_addr();
    let panicked = get(address, "/fetch?panic=1")?;
    assert_eq!(panicked.status, 500);
    assert_eq!(error_code(&panicked)?, "internal_error");
    let injected = get(address, "/fetch?url=x")?;
    assert_eq!(injected.status, 500);
    assert_eq!(error_code(&injected)?, "invalid_response");
    assert!(injected.header("Injected").is_none());
    assert_eq!(get(address, "/health")?.status, 200);
    server.shutdown()?;
    Ok(())
}

#[test]
fn zero_limits_are_refused_and_shutdown_releases_the_port() -> Result<(), Box<dyn std::error::Error>>
{
    let address: SocketAddr = "127.0.0.1:0".parse()?;
    for limits in [
        Limits {
            workers: 0,
            ..test_limits()
        },
        Limits {
            queue: 0,
            ..test_limits()
        },
        Limits {
            read_timeout: Duration::ZERO,
            ..test_limits()
        },
    ] {
        let refused = Server::bind(address, limits, RouteTable::new()).err();
        assert_eq!(
            refused.map(|error| error.kind()),
            Some(std::io::ErrorKind::InvalidInput)
        );
    }
    let server = start(test_limits(), RouteTable::new())?;
    let bound = server.local_addr();
    server.shutdown()?;
    let rebound = Server::bind(bound, test_limits(), RouteTable::new())?;
    assert_eq!(rebound.local_addr()?, bound);
    Ok(())
}
