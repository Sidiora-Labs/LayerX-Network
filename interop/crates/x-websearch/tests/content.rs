use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use x_websearch::canonical::{canonical_bytes, content_digest, digest_hex, ContentKind};
use x_websearch::content::{self, ContentStore, PEER_HEADER};
use x_websearch::fetch::Fetcher;
use x_websearch::payment::{hex, system_clock, PaymentGate, PAYMENT_REQUIRED};
use x_websearch::server::RouteError;
use x_websearch::{
    Config, KeyFiles, Limits, Request, Response, Route, RouteTable, RunningServer, Server,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> std::io::Result<Self> {
        let path =
            std::env::temp_dir().join(format!("x-websearch-content-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn page(text: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    Ok(canonical_bytes(
        ContentKind::Fetch,
        b"https://paxeer.app/",
        "text/plain",
        text,
    )?)
}

fn serve(routes: RouteTable) -> Result<RunningServer, Box<dyn std::error::Error>> {
    Ok(Server::bind("127.0.0.1:0".parse()?, Limits::default(), routes)?.spawn()?)
}

/// A sidecar serving `GET /content/<digest>` from its own store.
fn sidecar(store: &Arc<ContentStore>) -> Result<RunningServer, Box<dyn std::error::Error>> {
    let store = Arc::clone(store);
    let mut routes = RouteTable::new();
    routes.set(Route::Content, move |request: &Request| {
        store.handle(request)
    })?;
    serve(routes)
}

/// A peer that answers every digest with the same canonical bytes, which
/// hash to a different digest than the one asked for.
fn wrong_peer(bytes: Vec<u8>) -> Result<RunningServer, Box<dyn std::error::Error>> {
    let mut routes = RouteTable::new();
    routes.set(Route::Content, move |_: &Request| {
        Response::new(200, "application/octet-stream", bytes.clone())
    })?;
    serve(routes)
}

fn peer_url(server: &RunningServer) -> String {
    format!("http://{}", server.local_addr())
}

fn get(
    address: SocketAddr,
    target: &str,
    extra: &str,
) -> Result<(u16, String, Vec<u8>), Box<dyn std::error::Error>> {
    let mut stream = TcpStream::connect(address)?;
    stream
        .write_all(format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n{extra}\r\n").as_bytes())?;
    stream.set_read_timeout(Some(Duration::from_secs(15)))?;
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes)?;
    let split = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or("no header terminator")?;
    let head = String::from_utf8(bytes[..split].to_vec())?;
    let status = head.split(' ').nth(1).ok_or("no status")?.parse()?;
    Ok((status, head, bytes[split + 4..].to_vec()))
}

fn error_code(body: &[u8]) -> Result<String, Box<dyn std::error::Error>> {
    let value: serde_json::Value = serde_json::from_slice(body)?;
    Ok(value["error"].as_str().ok_or("error code")?.to_owned())
}

#[test]
fn a_local_hit_returns_exactly_the_stored_bytes() -> TestResult {
    let scratch = Scratch::new("local")?;
    let store = Arc::new(ContentStore::open(&scratch.0, &[])?);
    let bytes = page("stored locally")?;
    let digest = store.put(&bytes)?;
    assert_eq!(digest, content_digest(&bytes));
    assert_eq!(store.put(&bytes)?, digest);
    assert!(store.directory().join(digest_hex(&digest)).is_file());
    let server = sidecar(&store)?;
    let (status, head, body) = get(
        server.local_addr(),
        &format!("/content/{}", digest_hex(&digest)),
        "",
    )?;
    assert_eq!(status, 200);
    assert!(head.contains("Content-Type: application/octet-stream"));
    assert_eq!(body, bytes);
    let (status, _, body) = get(server.local_addr(), "/content/nothex", "")?;
    assert_eq!(
        (status, error_code(&body)?.as_str()),
        (400, "malformed_digest")
    );
    server.shutdown()?;
    Ok(())
}

#[test]
fn a_digest_held_only_by_a_peer_is_retrieved_checked_and_kept() -> TestResult {
    let holder_dir = Scratch::new("holder")?;
    let asker_dir = Scratch::new("asker")?;
    let holder = Arc::new(ContentStore::open(&holder_dir.0, &[])?);
    let bytes = page("held by the peer")?;
    let digest = holder.put(&bytes)?;
    let holder_server = sidecar(&holder)?;
    let liar = wrong_peer(page("a different page")?)?;
    let asker = Arc::new(ContentStore::open(
        &asker_dir.0,
        &[peer_url(&liar), peer_url(&holder_server)],
    )?);
    let asker_server = sidecar(&asker)?;
    let target = format!("/content/{}", digest_hex(&digest));

    let (status, _, body) = get(
        asker_server.local_addr(),
        &target,
        &format!("{PEER_HEADER}: 1\r\n"),
    )?;
    assert_eq!(
        (status, error_code(&body)?.as_str()),
        (404, "content_not_found"),
        "a peer request is answered from the local store only"
    );
    assert_eq!(asker.get(&digest)?, None);

    let (status, _, body) = get(asker_server.local_addr(), &target, "")?;
    assert_eq!(status, 200);
    assert_eq!(body, bytes);
    assert_eq!(asker.get(&digest)?, Some(bytes.clone()));
    let stored: Vec<_> = std::fs::read_dir(asker.directory())?.collect::<Result<_, _>>()?;
    assert_eq!(stored.len(), 1, "only the matching answer is kept");

    holder_server.shutdown()?;
    let (status, _, body) = get(asker_server.local_addr(), &target, "")?;
    assert_eq!(
        (status, body),
        (200, bytes),
        "a retrieved digest is served locally"
    );
    asker_server.shutdown()?;
    liar.shutdown()?;
    Ok(())
}

#[test]
fn a_peer_answer_whose_digest_does_not_match_is_refused() -> TestResult {
    let asker_dir = Scratch::new("mismatch")?;
    let wanted = content_digest(&page("the page asked for")?);
    let liar = wrong_peer(page("a different page")?)?;
    let asker = Arc::new(ContentStore::open(&asker_dir.0, &[peer_url(&liar)])?);
    let server = sidecar(&asker)?;
    let (status, _, body) = get(
        server.local_addr(),
        &format!("/content/{}", digest_hex(&wanted)),
        "",
    )?;
    assert_eq!(
        (status, error_code(&body)?.as_str()),
        (404, "content_not_found")
    );
    assert_eq!(std::fs::read_dir(asker.directory())?.count(), 0);

    let not_canonical = b"not canonical content".to_vec();
    let raw_liar = wrong_peer(not_canonical.clone())?;
    let raw_dir = Scratch::new("raw")?;
    let raw_asker = ContentStore::open(&raw_dir.0, &[peer_url(&raw_liar)])?;
    assert_eq!(raw_asker.retrieve(&content_digest(&not_canonical))?, None);
    server.shutdown()?;
    liar.shutdown()?;
    raw_liar.shutdown()?;
    Ok(())
}

#[test]
fn a_digest_no_peer_holds_is_not_found() -> TestResult {
    let empty_dir = Scratch::new("empty-peer")?;
    let asker_dir = Scratch::new("unheld")?;
    let empty = Arc::new(ContentStore::open(&empty_dir.0, &[])?);
    let empty_server = sidecar(&empty)?;
    let unreachable = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        format!("http://{}", listener.local_addr()?)
    };
    let asker = Arc::new(ContentStore::open(
        &asker_dir.0,
        &[unreachable, peer_url(&empty_server)],
    )?);
    let server = sidecar(&asker)?;
    let digest = content_digest(&page("nobody has this")?);
    let (status, _, body) = get(
        server.local_addr(),
        &format!("/content/{}", digest_hex(&digest)),
        "",
    )?;
    assert_eq!(
        (status, error_code(&body)?.as_str()),
        (404, "content_not_found")
    );
    server.shutdown()?;
    empty_server.shutdown()?;
    Ok(())
}

#[test]
fn the_store_refuses_non_canonical_bytes_and_drops_damaged_files() -> TestResult {
    let scratch = Scratch::new("damaged")?;
    let store = ContentStore::open(&scratch.0, &[])?;
    assert_eq!(
        store.put(b"plain bytes").map_err(|error| error.kind()),
        Err(std::io::ErrorKind::InvalidData)
    );
    let bytes = page("original")?;
    let digest = store.put(&bytes)?;
    let path = store.directory().join(digest_hex(&digest));
    std::fs::write(&path, page("tampered")?)?;
    assert_eq!(store.get(&digest)?, None);
    assert!(!path.exists());
    for peer in ["ftp://paxeer.app", "http://paxeer.app/?q=1", "not a url"] {
        assert!(
            ContentStore::open(&scratch.0, &[peer.to_owned()]).is_err(),
            "{peer}"
        );
    }
    Ok(())
}

/// A payment gate over the committed configuration, its data under `dir`, and
/// a receiver key drawn fresh from the operating system for this test only.
fn gate(dir: &std::path::Path) -> Result<Arc<PaymentGate>, Box<dyn std::error::Error>> {
    use std::io::Read as _;
    use std::os::unix::fs::PermissionsExt as _;
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/config/valid.json");
    let mut config: serde_json::Value = serde_json::from_slice(&std::fs::read(fixture)?)?;
    config["data_dir"] = serde_json::json!(dir.join("data"));
    let config = Config::parse(&config.to_string())?;
    let mut secret = [0_u8; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut secret)?;
    let key = dir.join("receiver.key");
    std::fs::write(&key, hex(&secret))?;
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600))?;
    let files = KeyFiles {
        attestor: None,
        submitter: None,
        receiver: key,
    };
    let receiver = files.load()?.receiver().clone();
    Ok(Arc::new(PaymentGate::new(
        &config,
        &receiver,
        x_websearch::conformance_suite()?,
        system_clock,
    )?))
}

/// The currencies of the offers a `PAYMENT-REQUIRED` header carries.
fn offered(head: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    use base64::Engine as _;
    let header = head
        .split("\r\n")
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case(PAYMENT_REQUIRED)
                .then(|| value.trim().to_owned())
        })
        .ok_or("no PAYMENT-REQUIRED header")?;
    let required: serde_json::Value =
        serde_json::from_slice(&base64::engine::general_purpose::STANDARD.decode(header)?)?;
    Ok(required["accepts"]
        .as_array()
        .ok_or("no accepts")?
        .iter()
        .filter_map(|offer| offer.pointer("/extra/layerx/currency")?.as_str())
        .map(str::to_owned)
        .collect())
}

#[test]
fn content_is_served_unpaid_and_fetch_only_behind_the_payment_gate() -> TestResult {
    let scratch = Scratch::new("gated")?;
    let gate = gate(&scratch.0)?;
    let store = Arc::new(ContentStore::open(&scratch.0, &[])?);
    let fetcher = Arc::new(Fetcher::new(x_websearch::config::FetchLimits {
        connect_timeout_ms: 3_000,
        total_timeout_ms: 10_000,
        max_body_bytes: x_websearch::config::BODY_LIMIT_BYTES,
        max_redirects: 3,
        allow_loopback: true,
    })?);
    let mut routes = RouteTable::new();
    content::register(&mut routes, &gate, &fetcher, &store)?;
    assert_eq!(
        content::register(&mut routes, &gate, &fetcher, &store),
        Err(RouteError::AlreadySet)
    );
    let server = serve(routes)?;
    let bytes = page("served without payment")?;
    let digest = store.put(&bytes)?;

    let (status, head, body) = get(
        server.local_addr(),
        &format!("/content/{}", digest_hex(&digest)),
        "",
    )?;
    assert_eq!((status, body), (200, bytes));
    assert!(!head.to_ascii_uppercase().contains(PAYMENT_REQUIRED));

    let target = format!("/fetch?url=http://{}/plain.txt", server.local_addr());
    let (status, head, _) = get(server.local_addr(), &target, "")?;
    assert_eq!(status, 402);
    assert_eq!(offered(&head)?, ["SID", "PAX", "USDC", "USDL"]);
    assert_eq!(std::fs::read_dir(store.directory())?.count(), 1);
    server.shutdown()?;
    Ok(())
}
