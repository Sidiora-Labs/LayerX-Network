use std::fs::File;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::process::ExitStatusExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};
use x_websearch::canonical::digest_hex;
use x_websearch::index::WebIndex;
use x_websearch::keys::{ATTESTOR_KEY_FILE, RECEIVER_KEY_FILE, SUBMITTER_KEY_FILE};
use x_websearch::payment::{hex, PAYER_DID, PAYMENT_REQUIRED};
use x_websearch::search::search;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const WAIT: Duration = Duration::from_secs(20);
const SIGTERM: i32 = 15;
const CURRENCIES: [&str; 4] = ["SID", "PAX", "USDC", "USDL"];

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> std::io::Result<Self> {
        let path =
            std::env::temp_dir().join(format!("x-websearch-binary-{}-{name}", std::process::id()));
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

/// A loopback server answering GET with the files under a directory and 404
/// for anything else, recording each request path.
struct Site {
    address: SocketAddr,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    acceptor: Option<thread::JoinHandle<()>>,
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        _ => "text/plain; charset=utf-8",
    }
}

fn respond(stream: &mut TcpStream, root: &Path, requests: &Mutex<Vec<String>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut head = Vec::new();
    let mut chunk = [0_u8; 1_024];
    while !head.windows(4).any(|window| window == b"\r\n\r\n") && head.len() < 16_384 {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(count) => head.extend_from_slice(&chunk[..count]),
        }
    }
    let text = String::from_utf8_lossy(&head);
    let target = text.split(' ').nth(1).unwrap_or_default();
    let path = target.split('?').next().unwrap_or_default().to_owned();
    if let Ok(mut requests) = requests.lock() {
        requests.push(path.clone());
    }
    let file = root.join(path.trim_start_matches('/'));
    let found = if path.split('/').any(|segment| segment == "..") || !file.is_file() {
        None
    } else {
        std::fs::read(&file).ok()
    };
    let response = match found {
        Some(body) => {
            let mut bytes = format!(
                "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: {}\r\nContent-Length: {}\r\n\r\n",
                content_type(&file),
                body.len()
            )
            .into_bytes();
            bytes.extend_from_slice(&body);
            bytes
        }
        None => {
            b"HTTP/1.1 404 Not Found\r\nConnection: close\r\nContent-Length: 0\r\n\r\n".to_vec()
        }
    };
    let _ = stream.write_all(&response);
}

impl Site {
    fn start(root: &Path) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (thread_requests, thread_stop, root) =
            (Arc::clone(&requests), Arc::clone(&stop), root.to_path_buf());
        let acceptor = thread::spawn(move || {
            for stream in listener.incoming() {
                if thread_stop.load(Ordering::SeqCst) {
                    break;
                }
                if let Ok(mut stream) = stream {
                    respond(&mut stream, &root, &thread_requests);
                }
            }
        });
        Ok(Self {
            address,
            requests,
            stop,
            acceptor: Some(acceptor),
        })
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.address)
    }

    fn count(&self, path: &str) -> usize {
        self.requests
            .lock()
            .map(|requests| requests.iter().filter(|seen| *seen == path).count())
            .unwrap_or_default()
    }
}

impl Drop for Site {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        drop(TcpStream::connect_timeout(
            &self.address,
            Duration::from_secs(1),
        ));
        if let Some(acceptor) = self.acceptor.take() {
            let _ = acceptor.join();
        }
    }
}

fn free_port() -> std::io::Result<u16> {
    Ok(TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

/// The committed configuration pointed at this test's listen port, data
/// directory and loopback site.
fn configure(scratch: &Scratch, site: &Site, edit: impl FnOnce(&mut Value)) -> TestResult<Value> {
    let mut config: Value =
        serde_json::from_slice(&std::fs::read(fixtures().join("binary/config.json"))?)?;
    config["listen"] = json!(format!("127.0.0.1:{}", free_port()?));
    config["data_dir"] = json!(scratch.0.join("data"));
    config["seeds"] = json!([site.url("/index.html")]);
    config["peers"] = json!([site.url("")]);
    config["gateway"]["endpoint"] = json!(site.url("/rpc"));
    edit(&mut config);
    std::fs::write(scratch.0.join("config.json"), config.to_string())?;
    Ok(config)
}

/// A receiver key drawn fresh from the operating system into the scratch
/// directory, readable by its owner only.
fn receiver_key(scratch: &Scratch) -> TestResult<(PathBuf, String)> {
    let mut secret = [0_u8; 32];
    File::open("/dev/urandom")?.read_exact(&mut secret)?;
    let path = scratch.0.join("receiver.key");
    let text = hex(&secret);
    std::fs::write(&path, &text)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    Ok((path, text))
}

/// The real x-websearch executable, its standard error kept in a file.
struct Sidecar {
    child: Child,
    address: SocketAddr,
    log: PathBuf,
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn launch(scratch: &Scratch, receiver: Option<&Path>) -> TestResult<(Child, PathBuf)> {
    let log = scratch.0.join("stderr.log");
    let mut command = Command::new(env!("CARGO_BIN_EXE_x-websearch"));
    command
        .arg("--config")
        .arg(scratch.0.join("config.json"))
        .env_remove(ATTESTOR_KEY_FILE)
        .env_remove(SUBMITTER_KEY_FILE)
        .env_remove(RECEIVER_KEY_FILE)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(File::create(&log)?));
    if let Some(receiver) = receiver {
        command.env(RECEIVER_KEY_FILE, receiver);
    }
    Ok((command.spawn()?, log))
}

fn wait_exit(child: &mut Child) -> TestResult<ExitStatus> {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() > deadline {
            return Err("the sidecar did not exit".into());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

impl Sidecar {
    fn start(scratch: &Scratch, config: &Value, receiver: &Path) -> TestResult<Self> {
        let address: SocketAddr = config["listen"].as_str().ok_or("listen")?.parse()?;
        let (child, log) = launch(scratch, Some(receiver))?;
        let mut sidecar = Self {
            child,
            address,
            log,
        };
        let deadline = Instant::now() + WAIT;
        loop {
            if let Ok((200, _, body)) = sidecar.get("/health", "") {
                assert_eq!(body, br#"{"status":"ok"}"#);
                return Ok(sidecar);
            }
            if let Some(status) = sidecar.child.try_wait()? {
                return Err(
                    format!("the sidecar exited with {status}: {}", sidecar.stderr()?).into(),
                );
            }
            if Instant::now() > deadline {
                return Err("the sidecar never answered /health".into());
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn stderr(&self) -> std::io::Result<String> {
        std::fs::read_to_string(&self.log)
    }

    fn get(&self, target: &str, extra: &str) -> TestResult<(u16, String, Vec<u8>)> {
        let mut stream = TcpStream::connect_timeout(&self.address, Duration::from_secs(2))?;
        stream.write_all(
            format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n{extra}\r\n").as_bytes(),
        )?;
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

    fn wait_for_log(&self, line: &str) -> TestResult<String> {
        let deadline = Instant::now() + WAIT;
        loop {
            let log = self.stderr()?;
            if log.contains(line) {
                return Ok(log);
            }
            if Instant::now() > deadline {
                return Err(format!("no {line:?} in: {log}").into());
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn terminate(&mut self) -> TestResult<ExitStatus> {
        let status = Command::new("kill")
            .arg("-TERM")
            .arg(self.child.id().to_string())
            .status()?;
        assert!(status.success());
        wait_exit(&mut self.child)
    }
}

/// The offers of a 402 answer's `PAYMENT-REQUIRED` header.
fn offers(head: &str) -> TestResult<Vec<Value>> {
    let header = head
        .split("\r\n")
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case(PAYMENT_REQUIRED)
                .then(|| value.trim().to_owned())
        })
        .ok_or("no PAYMENT-REQUIRED header")?;
    let required: Value = serde_json::from_slice(&STANDARD.decode(header)?)?;
    Ok(required["accepts"].as_array().ok_or("no accepts")?.clone())
}

/// Where an offer's account points: `main` for `agent:<did>:main`, and
/// `asset:<id>` for a per-asset account.
fn account_kind(account: &str) -> String {
    if account.ends_with(":main") {
        "main".to_owned()
    } else {
        account
            .split_once(":asset:")
            .map(|(_, asset)| format!("asset:{asset}"))
            .unwrap_or_default()
    }
}

fn assert_offers_every_asset(offers: &[Value], config: &Value, schemes: &[&str]) -> TestResult {
    let mut expected = Vec::new();
    for currency in CURRENCIES {
        let asset_id = config["assets"][currency]["asset_id"]
            .as_str()
            .ok_or("asset id")?;
        let account = if currency == "PAX" {
            "main".to_owned()
        } else {
            format!("asset:{asset_id}")
        };
        for scheme in schemes {
            expected.push((
                (*scheme).to_owned(),
                currency.to_owned(),
                asset_id.to_owned(),
                config["assets"][currency]["price"]
                    .as_str()
                    .ok_or("price")?
                    .to_owned(),
                account.clone(),
            ));
        }
    }
    let found: Vec<(String, String, String, String, String)> = offers
        .iter()
        .map(|offer| {
            let field = |pointer: &str| {
                offer
                    .pointer(pointer)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned()
            };
            (
                field("/scheme"),
                field("/extra/layerx/currency"),
                field("/asset").trim_start_matches("0x").to_owned(),
                field("/amount"),
                account_kind(&field("/extra/layerx/account")),
            )
        })
        .collect();
    assert_eq!(found, expected);
    Ok(())
}

#[test]
fn the_pinned_conformance_suite_is_the_recorded_gateway_exchange() -> TestResult {
    let mut names: Vec<PathBuf> = std::fs::read_dir(fixtures().join("gateway"))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<_, _>>()?;
    names.sort();
    let mut digest = Sha256::new();
    let mut rules = 0_u64;
    for name in names {
        let bytes = std::fs::read(name)?;
        digest.update(&bytes);
        let recording: Value = serde_json::from_slice(&bytes)?;
        if let Some(list) = recording
            .pointer("/endpoints/gateway/*")
            .and_then(Value::as_array)
        {
            rules += u64::try_from(list.len())?;
        }
    }
    let digest: [u8; 32] = digest.finalize().into();
    assert_eq!(digest, x_websearch::CONFORMANCE_SUITE_DIGEST);
    assert_eq!(rules, x_websearch::CONFORMANCE_VECTOR_COUNT);
    let suite = x_websearch::conformance_suite()?;
    assert_eq!(suite.suite().as_str(), x_websearch::CONFORMANCE_SUITE);
    assert_eq!(suite.vector_count(), rules);
    assert_eq!(suite.suite_digest(), digest);
    Ok(())
}

#[test]
fn the_binary_serves_health_free_and_every_paid_route_behind_the_payment_gate() -> TestResult {
    let scratch = Scratch::new("routes")?;
    let site = Site::start(&fixtures().join("binary/site"))?;
    let config = configure(&scratch, &site, |_| {})?;
    let (receiver, secret) = receiver_key(&scratch)?;
    let sidecar = Sidecar::start(&scratch, &config, &receiver)?;

    let (status, head, _) = sidecar.get("/search?q=lighthouse", "")?;
    assert_eq!(status, 402);
    assert_offers_every_asset(&offers(&head)?, &config, &["exact"])?;
    let payer = format!("{PAYER_DID}: did:layerx:buyer\r\n");
    let (status, head, _) = sidecar.get("/search?q=lighthouse", &payer)?;
    assert_eq!(status, 402);
    assert_offers_every_asset(&offers(&head)?, &config, &["metered", "exact"])?;

    let target = format!("/fetch?url={}", site.url("/notes.txt"));
    let (status, head, _) = sidecar.get(&target, "")?;
    assert_eq!(status, 402);
    assert_offers_every_asset(&offers(&head)?, &config, &["exact"])?;
    assert_eq!(site.count("/notes.txt"), 0, "an unpaid fetch dials nothing");

    let unheld = digest_hex(&[0x5a; 32]);
    let (status, head, body) = sidecar.get(&format!("/content/{unheld}"), "")?;
    assert_eq!(status, 404);
    assert!(!head.to_ascii_uppercase().contains(PAYMENT_REQUIRED));
    let body: Value = serde_json::from_slice(&body)?;
    assert_eq!(body["error"], "content_not_found");
    assert_eq!(
        site.count(&format!("/content/{unheld}")),
        1,
        "the peer was asked"
    );

    let (status, _, _) = sidecar.get("/elsewhere", "")?;
    assert_eq!(status, 404);
    assert!(!sidecar.stderr()?.contains(&secret));
    Ok(())
}

#[test]
fn one_crawl_cycle_lands_a_searchable_document_and_the_binary_stops_cleanly() -> TestResult {
    let scratch = Scratch::new("crawl")?;
    let site = Site::start(&fixtures().join("binary/site"))?;
    let config = configure(&scratch, &site, |_| {})?;
    let (receiver, secret) = receiver_key(&scratch)?;
    let mut sidecar = Sidecar::start(&scratch, &config, &receiver)?;

    let log = sidecar.wait_for_log("crawl cycle finished")?;
    assert!(
        log.contains("x-websearch crawl cycle finished: 2 visited, 2 indexed, 0 deferred"),
        "{log}"
    );
    assert_eq!(site.count("/robots.txt"), 1);
    assert_eq!(site.count("/index.html"), 1);
    assert_eq!(site.count("/beacons.html"), 1);
    assert_eq!(site.count("/notes.txt"), 0);

    let status = sidecar.terminate()?;
    assert_eq!(status.signal(), Some(SIGTERM));
    assert!(!status.core_dumped());
    assert!(!sidecar.stderr()?.contains(&secret));
    TcpListener::bind(sidecar.address)?;

    let index = WebIndex::open(&scratch.0.join("data"))?;
    assert_eq!(index.num_docs(), 2);
    let found = search(&index, "lighthouse")?;
    assert_eq!(found[0].result.url, site.url("/index.html"));
    assert_eq!(found[0].result.title, "Lighthouse Register");
    let beacons = search(&index, "beacon flashes")?;
    assert_eq!(beacons[0].result.url, site.url("/beacons.html"));
    Ok(())
}

#[test]
fn the_binary_refuses_to_start_naming_the_configuration_field_or_key() -> TestResult {
    let scratch = Scratch::new("refusals")?;
    let site = Site::start(&fixtures().join("binary/site"))?;
    let (receiver, _) = receiver_key(&scratch)?;

    configure(&scratch, &site, |config| {
        config["kernel_network_id"] = json!(0);
    })?;
    let (mut child, log) = launch(&scratch, Some(&receiver))?;
    assert_eq!(wait_exit(&mut child)?.code(), Some(2));
    assert!(std::fs::read_to_string(&log)?.contains("kernel_network_id"));

    configure(&scratch, &site, |_| {})?;
    let (mut child, log) = launch(&scratch, None)?;
    assert_eq!(wait_exit(&mut child)?.code(), Some(2));
    assert!(std::fs::read_to_string(&log)?.contains(RECEIVER_KEY_FILE));

    std::fs::set_permissions(&receiver, std::fs::Permissions::from_mode(0o644))?;
    let (mut child, log) = launch(&scratch, Some(&receiver))?;
    assert_eq!(wait_exit(&mut child)?.code(), Some(2));
    assert!(std::fs::read_to_string(&log)?.contains(RECEIVER_KEY_FILE));
    assert_eq!(
        site.count("/index.html"),
        0,
        "a refused sidecar crawls nothing"
    );
    Ok(())
}
