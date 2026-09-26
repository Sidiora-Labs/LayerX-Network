use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use x_websearch::canonical::{content_digest, digest_hex, CanonicalContent, ContentKind};
use x_websearch::content::ContentStore;
use x_websearch::index::WebIndex;
use x_websearch::payment::{hex as hex_of, system_clock, PaymentGate, PAYMENT_REQUIRED};
use x_websearch::search::register;
use x_websearch::search::{
    search, search_canonical_bytes, search_route, search_text, SearchError, SearchResult,
    MAX_QUERY_BYTES, MAX_RESULTS, SEARCH_MEDIA_TYPE,
};
use x_websearch::server::RouteError;
use x_websearch::{Config, KeyFiles, Limits, Request, Route, RouteTable, Server};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> std::io::Result<Self> {
        let path =
            std::env::temp_dir().join(format!("x-websearch-search-{}-{name}", std::process::id()));
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

struct Document {
    url: String,
    title: String,
    body: String,
}

struct Vector {
    query: String,
    documents: Vec<Document>,
    results: Vec<SearchResult>,
    text: String,
    canonical: Vec<u8>,
    digest: [u8; 32],
}

fn text_of(value: &serde_json::Value, key: &str) -> Result<String, Box<dyn std::error::Error>> {
    Ok(value[key]
        .as_str()
        .ok_or(format!("{key} missing"))?
        .to_owned())
}

fn hex(text: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    if !text.len().is_multiple_of(2) {
        return Err("odd hex length".into());
    }
    (0..text.len())
        .step_by(2)
        .map(|at| Ok(u8::from_str_radix(&text[at..at + 2], 16)?))
        .collect()
}

fn results_of(value: &serde_json::Value) -> Result<Vec<SearchResult>, Box<dyn std::error::Error>> {
    value
        .as_array()
        .ok_or("results")?
        .iter()
        .map(|result| {
            Ok(SearchResult {
                url: text_of(result, "url")?,
                title: text_of(result, "title")?,
                snippet: text_of(result, "snippet")?,
            })
        })
        .collect()
}

fn vector() -> Result<Vector, Box<dyn std::error::Error>> {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/crawl-site/search-vector.json");
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
    let documents = value["documents"]
        .as_array()
        .ok_or("documents")?
        .iter()
        .map(|document| {
            Ok(Document {
                url: text_of(document, "url")?,
                title: text_of(document, "title")?,
                body: text_of(document, "body")?,
            })
        })
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
    let results = results_of(&value["results"])?;
    let digest: [u8; 32] = hex(&text_of(&value, "digest")?)?
        .try_into()
        .map_err(|_| "digest length")?;
    Ok(Vector {
        query: text_of(&value, "query")?,
        documents,
        results,
        text: text_of(&value, "text")?,
        canonical: hex(&text_of(&value, "canonical")?)?,
        digest,
    })
}

fn indexed<'a>(
    scratch: &Scratch,
    documents: impl Iterator<Item = &'a Document>,
) -> Result<WebIndex, Box<dyn std::error::Error>> {
    let index = WebIndex::open(&scratch.0)?;
    for document in documents {
        index.put(&document.url, &document.title, &document.body)?;
    }
    index.commit()?;
    Ok(index)
}

#[test]
fn search_orders_by_score_then_url_caps_at_ten_and_matches_the_committed_vector() -> TestResult {
    let vector = vector()?;
    let scratch = Scratch::new("vector")?;
    let index = indexed(&scratch, vector.documents.iter())?;
    assert_eq!(index.num_docs(), 13);
    let scored = search(&index, &vector.query)?;
    assert_eq!(scored.len(), MAX_RESULTS);
    assert!(scored[0].score > scored[1].score);
    assert!(scored[1].score > scored[2].score);
    for pair in scored.windows(2) {
        assert!(pair[0].score >= pair[1].score);
        if pair[0].score.total_cmp(&pair[1].score).is_eq() {
            assert!(pair[0].result.url < pair[1].result.url);
        }
    }
    assert!(scored[2..]
        .iter()
        .all(|result| result.score.total_cmp(&scored[2].score).is_eq()));
    let results: Vec<SearchResult> = scored.into_iter().map(|scored| scored.result).collect();
    assert_eq!(results, vector.results);
    assert!(results.iter().all(
        |result| !result.url.ends_with("/log/09.html") && !result.url.ends_with("/log/10.html")
    ));

    let text = search_text(&results)?;
    assert_eq!(text, vector.text);
    assert!(!text.contains("score"));
    assert!(text.starts_with("[{\"url\":\"http://127.0.0.1/pilot.html\",\"title\":"));
    let canonical = search_canonical_bytes(&vector.query, &results)?;
    assert_eq!(canonical, vector.canonical);
    assert_eq!(content_digest(&canonical), vector.digest);
    let parsed = CanonicalContent::parse(&canonical)?;
    assert_eq!(parsed.kind, ContentKind::Search);
    assert_eq!(parsed.payload, vector.query.as_bytes());
    assert_eq!(parsed.media_type, SEARCH_MEDIA_TYPE);
    assert_eq!(parsed.text, vector.text);
    Ok(())
}

#[test]
fn the_order_does_not_depend_on_the_order_pages_were_indexed_in() -> TestResult {
    let vector = vector()?;
    let scratch = Scratch::new("reversed")?;
    let index = indexed(&scratch, vector.documents.iter().rev())?;
    let results: Vec<SearchResult> = search(&index, &vector.query)?
        .into_iter()
        .map(|scored| scored.result)
        .collect();
    assert_eq!(results, vector.results);
    assert_eq!(
        content_digest(&search_canonical_bytes(&vector.query, &results)?),
        vector.digest
    );
    Ok(())
}

#[test]
fn fewer_matches_than_the_cap_are_all_returned_and_no_match_is_an_empty_array() -> TestResult {
    let vector = vector()?;
    let scratch = Scratch::new("few")?;
    let index = indexed(&scratch, vector.documents.iter())?;
    let results = search(&index, "mole")?;
    let urls: Vec<&str> = results
        .iter()
        .map(|result| result.result.url.as_str())
        .collect();
    assert_eq!(
        urls,
        [
            "http://127.0.0.1/channel.html",
            "http://127.0.0.1/pilot.html"
        ]
    );
    assert!(results[0].score.total_cmp(&results[1].score).is_eq());
    let weather = search(&index, "FOG tonight")?;
    assert_eq!(weather.len(), 1);
    assert_eq!(weather[0].result.title, "Weather desk");
    assert_eq!(
        weather[0].result.snippet,
        "Fog expected over the bay tonight"
    );
    assert!(search(&index, "lighthouse")?.is_empty());
    assert_eq!(search_text(&[])?, "[]");
    Ok(())
}

#[test]
fn a_query_with_no_terms_or_too_many_bytes_is_refused() -> TestResult {
    let scratch = Scratch::new("refusals")?;
    let index = WebIndex::open(&scratch.0)?;
    assert!(matches!(search(&index, ""), Err(SearchError::EmptyQuery)));
    assert!(matches!(
        search(&index, "?! -- ::"),
        Err(SearchError::EmptyQuery)
    ));
    assert!(matches!(
        search(&index, &"a".repeat(MAX_QUERY_BYTES + 1)),
        Err(SearchError::QueryTooLong)
    ));
    let many: Vec<String> = (0..33).map(|word| format!("w{word}")).collect();
    assert!(matches!(
        search(&index, &many.join(" ")),
        Err(SearchError::TooManyTerms)
    ));
    assert!(search(&index, "harbour")?.is_empty());
    Ok(())
}

fn get(address: SocketAddr, target: &str) -> Result<(u16, Vec<u8>), Box<dyn std::error::Error>> {
    let mut stream = TcpStream::connect(address)?;
    stream.write_all(format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes())?;
    stream.set_read_timeout(Some(Duration::from_secs(15)))?;
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes)?;
    let split = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or("no header terminator")?;
    let head = String::from_utf8(bytes[..split].to_vec())?;
    let status = head.split(' ').nth(1).ok_or("no status")?.parse()?;
    Ok((status, bytes[split + 4..].to_vec()))
}

fn error_code(body: &[u8]) -> Result<String, Box<dyn std::error::Error>> {
    let value: serde_json::Value = serde_json::from_slice(body)?;
    Ok(value["error"].as_str().ok_or("error code")?.to_owned())
}

#[test]
fn the_search_route_answers_with_the_results_and_stores_their_canonical_bytes() -> TestResult {
    let vector = vector()?;
    let scratch = Scratch::new("route")?;
    let index = Arc::new(indexed(&scratch, vector.documents.iter())?);
    let store = Arc::new(ContentStore::open(&scratch.0, &[])?);
    let mut routes = RouteTable::new();
    let (route_index, route_store) = (Arc::clone(&index), Arc::clone(&store));
    routes.set(Route::Search, move |request: &Request| {
        search_route(&route_index, &route_store, request)
    })?;
    let server = Server::bind("127.0.0.1:0".parse()?, Limits::default(), routes)?.spawn()?;
    let address = server.local_addr();

    let (status, body) = get(address, "/search?q=harbour")?;
    assert_eq!(status, 200);
    let value: serde_json::Value = serde_json::from_slice(&body)?;
    assert_eq!(value["query"], "harbour");
    assert_eq!(value["media_type"], SEARCH_MEDIA_TYPE);
    assert_eq!(value["digest"], digest_hex(&vector.digest));
    let results = results_of(&value["results"])?;
    assert_eq!(results, vector.results);
    assert_eq!(store.get(&vector.digest)?, Some(vector.canonical.clone()));

    for (target, code) in [
        ("/search", "missing_query"),
        ("/search?q=", "missing_query"),
        ("/search?q=a&q=b", "duplicate_query"),
        ("/search?q=%zz", "malformed_query"),
        ("/search?q=%21%21", "empty_query"),
    ] {
        let (status, body) = get(address, target)?;
        assert_eq!(
            (status, error_code(&body)?.as_str()),
            (400, code),
            "{target}"
        );
    }
    server.shutdown()?;
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
    std::fs::write(&key, hex_of(&secret))?;
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

fn get_head(
    address: SocketAddr,
    target: &str,
) -> Result<(u16, String), Box<dyn std::error::Error>> {
    let mut stream = TcpStream::connect(address)?;
    stream.write_all(format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes())?;
    stream.set_read_timeout(Some(Duration::from_secs(15)))?;
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes)?;
    let split = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or("no header terminator")?;
    let head = String::from_utf8(bytes[..split].to_vec())?;
    let status = head.split(' ').nth(1).ok_or("no status")?.parse()?;
    Ok((status, head))
}

#[test]
fn the_search_route_is_registered_behind_the_payment_gate() -> TestResult {
    let vector = vector()?;
    let scratch = Scratch::new("gated")?;
    let gate = gate(&scratch.0)?;
    let index = Arc::new(indexed(&scratch, vector.documents.iter())?);
    let store = Arc::new(ContentStore::open(&scratch.0, &[])?);
    let mut routes = RouteTable::new();
    register(&mut routes, &gate, &index, &store)?;
    assert_eq!(
        register(&mut routes, &gate, &index, &store),
        Err(RouteError::AlreadySet)
    );
    let server = Server::bind("127.0.0.1:0".parse()?, Limits::default(), routes)?.spawn()?;

    let (status, head) = get_head(server.local_addr(), "/search?q=harbour")?;
    assert_eq!(status, 402);
    assert_eq!(offered(&head)?, ["SID", "PAX", "USDC", "USDL"]);
    assert_eq!(
        store.get(&vector.digest)?,
        None,
        "an unpaid search releases nothing"
    );
    server.shutdown()?;
    Ok(())
}
