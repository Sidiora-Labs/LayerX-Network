use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use x_websearch::canonical::{
    canonical_bytes, content_digest, digest_hex, CanonicalContent, CanonicalError, ContentKind,
};
use x_websearch::config::{FetchLimits, BODY_LIMIT_BYTES};
use x_websearch::content::ContentStore;
use x_websearch::extract::{extract, html_text, ExtractError, Extracted};
use x_websearch::fetch::{destination_permitted, fetch_route, FetchError, Fetcher, Url};
use x_websearch::robots::Robots;
use x_websearch::{Limits, Request, Route, RouteTable, RunningServer, Server};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const MIB2: usize = 2_097_152;

fn site_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/site")
}

#[derive(Clone)]
enum Reply {
    File(&'static str, &'static str),
    Chunked(&'static str, &'static str),
    UntilClose(&'static str, &'static str),
    Encoded(&'static str, &'static str),
    Untyped(&'static str),
    Redirect(u16, String),
    Status(u16),
    Filled { bytes: usize, declared: bool },
    ChunkedFilled(usize),
    Drip,
    Silent,
}

struct Site {
    address: SocketAddr,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    acceptor: Option<thread::JoinHandle<()>>,
}

impl Site {
    fn url(&self, target: &str) -> String {
        format!("http://{}{target}", self.address)
    }

    fn count(&self, target: &str) -> usize {
        self.requests.lock().map_or(0, |requests| {
            requests.iter().filter(|path| *path == target).count()
        })
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

fn head(status: u16, headers: &[(&str, String)]) -> Vec<u8> {
    let mut text = format!("HTTP/1.1 {status} Fixture\r\nConnection: close\r\n");
    for (name, value) in headers {
        let _ = write!(text, "{name}: {value}\r\n");
    }
    text.push_str("\r\n");
    text.into_bytes()
}

fn read_file(name: &str) -> Vec<u8> {
    std::fs::read(site_dir().join(name)).unwrap_or_default()
}

fn answer(stream: &mut TcpStream, reply: Option<Reply>) {
    match reply {
        Some(Reply::Drip) => {
            let _ = stream.write_all(&head(
                200,
                &[
                    ("Content-Type", "text/plain".to_owned()),
                    ("Content-Length", "1000".to_owned()),
                ],
            ));
            for _ in 0..40 {
                if stream.write_all(b"a").is_err() {
                    return;
                }
                thread::sleep(Duration::from_millis(100));
            }
        }
        Some(Reply::Silent) => {
            thread::sleep(Duration::from_secs(3));
        }
        other => {
            let _ = stream.write_all(&reply_bytes(other));
        }
    }
}

fn reply_bytes(reply: Option<Reply>) -> Vec<u8> {
    match reply {
        None => head(404, &[("Content-Length", "0".to_owned())]),
        Some(Reply::File(name, content_type)) => {
            let body = read_file(name);
            let mut bytes = head(
                200,
                &[
                    ("Content-Type", content_type.to_owned()),
                    ("Content-Length", body.len().to_string()),
                ],
            );
            bytes.extend_from_slice(&body);
            bytes
        }
        Some(Reply::Encoded(name, content_type)) => {
            let body = read_file(name);
            let mut bytes = head(
                200,
                &[
                    ("Content-Type", content_type.to_owned()),
                    ("Content-Encoding", "gzip".to_owned()),
                    ("Content-Length", body.len().to_string()),
                ],
            );
            bytes.extend_from_slice(&body);
            bytes
        }
        Some(Reply::Untyped(name)) => {
            let body = read_file(name);
            let mut bytes = head(200, &[("Content-Length", body.len().to_string())]);
            bytes.extend_from_slice(&body);
            bytes
        }
        Some(Reply::Chunked(name, content_type)) => {
            let body = read_file(name);
            let mut bytes = head(
                200,
                &[
                    ("Content-Type", content_type.to_owned()),
                    ("Transfer-Encoding", "chunked".to_owned()),
                ],
            );
            for chunk in body.chunks(7) {
                bytes.extend_from_slice(format!("{:x};ext=1\r\n", chunk.len()).as_bytes());
                bytes.extend_from_slice(chunk);
                bytes.extend_from_slice(b"\r\n");
            }
            bytes.extend_from_slice(b"0\r\nTrailer: done\r\n\r\n");
            bytes
        }
        Some(Reply::UntilClose(name, content_type)) => {
            let mut bytes = head(200, &[("Content-Type", content_type.to_owned())]);
            bytes.extend_from_slice(&read_file(name));
            bytes
        }
        Some(Reply::Redirect(status, location)) => head(
            status,
            &[("Location", location), ("Content-Length", "0".to_owned())],
        ),
        Some(Reply::Status(status)) => head(status, &[("Content-Length", "0".to_owned())]),
        Some(Reply::Filled { bytes, declared }) => {
            let mut headers = vec![("Content-Type", "text/plain".to_owned())];
            if declared {
                headers.push(("Content-Length", bytes.to_string()));
            }
            let mut response = head(200, &headers);
            response.resize(response.len() + bytes, b'a');
            response
        }
        Some(Reply::ChunkedFilled(bytes)) => {
            let mut response = head(
                200,
                &[
                    ("Content-Type", "text/plain".to_owned()),
                    ("Transfer-Encoding", "chunked".to_owned()),
                ],
            );
            let mut left = bytes;
            while left > 0 {
                let size = left.min(65_536);
                response.extend_from_slice(format!("{size:x}\r\n").as_bytes());
                response.resize(response.len() + size, b'a');
                response.extend_from_slice(b"\r\n");
                left -= size;
            }
            response.extend_from_slice(b"0\r\n\r\n");
            response
        }
        Some(Reply::Drip | Reply::Silent) => Vec::new(),
    }
}

fn request_target(stream: &mut TcpStream) -> Option<String> {
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1_024];
    while !buffer.windows(4).any(|window| window == b"\r\n\r\n") {
        let count = stream.read(&mut chunk).ok()?;
        if count == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..count]);
    }
    let text = String::from_utf8(buffer).ok()?;
    let line = text.lines().next()?;
    assert!(
        text.contains("\r\nUser-Agent: x-websearch/"),
        "the fetch names its user-agent token"
    );
    line.split(' ').nth(1).map(str::to_owned)
}

fn serve_site(routes: Vec<(&str, Reply)>) -> std::io::Result<Site> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let routes: Arc<BTreeMap<String, Reply>> = Arc::new(
        routes
            .into_iter()
            .map(|(path, reply)| (path.to_owned(), reply))
            .collect(),
    );
    let requests = Arc::new(Mutex::new(Vec::new()));
    let stop = Arc::new(AtomicBool::new(false));
    let (log, halt) = (Arc::clone(&requests), Arc::clone(&stop));
    let acceptor = thread::spawn(move || {
        for incoming in listener.incoming() {
            if halt.load(Ordering::SeqCst) {
                return;
            }
            let Ok(mut stream) = incoming else { continue };
            let (routes, log) = (Arc::clone(&routes), Arc::clone(&log));
            thread::spawn(move || {
                let Some(target) = request_target(&mut stream) else {
                    return;
                };
                if let Ok(mut log) = log.lock() {
                    log.push(target.clone());
                }
                answer(&mut stream, routes.get(&target).cloned());
            });
        }
    });
    Ok(Site {
        address,
        requests,
        stop,
        acceptor: Some(acceptor),
    })
}

fn robots() -> (&'static str, Reply) {
    ("/robots.txt", Reply::File("robots.txt", "text/plain"))
}

fn loopback_limits() -> FetchLimits {
    FetchLimits {
        connect_timeout_ms: 3_000,
        total_timeout_ms: 10_000,
        max_body_bytes: BODY_LIMIT_BYTES,
        max_redirects: 3,
        allow_loopback: true,
    }
}

fn fetcher() -> Result<Fetcher, FetchError> {
    Fetcher::new(loopback_limits())
}

#[derive(serde::Deserialize)]
struct Vector {
    path: String,
    content_type: String,
    payload: String,
    media_type: String,
    text: String,
    digest: String,
}

#[derive(serde::Deserialize)]
struct VectorFile {
    kind: u8,
    domain: String,
    vectors: Vec<Vector>,
}

fn vectors() -> Result<VectorFile, Box<dyn std::error::Error>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/content-vectors.json");
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}

fn leak(text: &str) -> &'static str {
    Box::leak(text.to_owned().into_boxed_str())
}

#[test]
fn committed_pages_extract_to_the_committed_text_and_digest() -> TestResult {
    let vectors = vectors()?;
    assert_eq!(vectors.kind, ContentKind::Fetch.byte());
    assert_eq!(vectors.domain, "PAXEERX_WEB_CONTENT_V1");
    let mut routes = vec![robots()];
    for vector in &vectors.vectors {
        let name = leak(vector.path.trim_start_matches('/'));
        routes.push((
            leak(&vector.path),
            Reply::File(name, leak(&vector.content_type)),
        ));
    }
    let site = serve_site(routes)?;
    let fetcher = fetcher()?;
    for vector in &vectors.vectors {
        let url = site.url(&vector.path);
        let page = fetcher.fetch(&url)?;
        assert_eq!(page.text, vector.text, "{}", vector.path);
        assert_eq!(page.media_type, vector.media_type, "{}", vector.path);
        assert_eq!(page.url, url);
        assert_eq!(page.final_url, url);
        assert_eq!(
            page.canonical,
            canonical_bytes(
                ContentKind::Fetch,
                url.as_bytes(),
                &vector.media_type,
                &vector.text
            )?
        );
        assert_eq!(page.digest, content_digest(&page.canonical));
        let parsed = CanonicalContent::parse(&page.canonical)?;
        assert_eq!(parsed.payload, url.as_bytes());
        let pinned = canonical_bytes(
            ContentKind::Fetch,
            vector.payload.as_bytes(),
            &page.media_type,
            &page.text,
        )?;
        assert_eq!(
            digest_hex(&content_digest(&pinned)),
            vector.digest,
            "{}",
            vector.path
        );
    }
    assert_eq!(
        site.count("/robots.txt"),
        1,
        "robots.txt is cached per host"
    );
    Ok(())
}

#[test]
fn two_fetches_of_the_same_page_agree_on_the_digest() -> TestResult {
    let first = serve_site(vec![
        robots(),
        ("/index.html", Reply::File("index.html", "text/html")),
    ])?;
    let second = serve_site(vec![
        robots(),
        (
            "/index.html",
            Reply::Chunked("index.html", "text/html; charset=utf-8"),
        ),
    ])?;
    let third = serve_site(vec![
        robots(),
        ("/index.html", Reply::UntilClose("index.html", "Text/Html")),
    ])?;
    let (one, two, three) = (fetcher()?, fetcher()?, fetcher()?);
    let a = one.fetch(&first.url("/index.html"))?;
    let b = two.fetch(&second.url("/index.html"))?;
    let c = three.fetch(&third.url("/index.html"))?;
    assert_eq!(a.text, b.text);
    assert_eq!(a.text, c.text);
    assert_eq!(
        (a.media_type.as_str(), c.media_type.as_str()),
        ("text/html", "text/html")
    );
    let same = |text: &str| canonical_bytes(ContentKind::Fetch, b"same", "text/html", text);
    assert_eq!(
        content_digest(&same(&a.text)?),
        content_digest(&same(&b.text)?)
    );
    assert_eq!(
        content_digest(&same(&a.text)?),
        content_digest(&same(&c.text)?)
    );
    Ok(())
}

#[test]
fn html_extraction_drops_hidden_elements_and_decodes_references() -> TestResult {
    let html = "<p>A<SCRIPT>x</script >B<style media=\"a>b\">c</STYLE>C</p>\
                <noscript><noscript>n</noscript>still</noscript>D\
                <template><script>\"</template>\"</script>t</template>E\
                <a title=\"x > y\">F</a>&#x41;&#66;&#0;&#xD800;&unknown;G<3";
    assert_eq!(
        html_text(html),
        "\nABC\nDEF\u{41}B\u{fffd}\u{fffd}&unknown;G<3"
    );
    let extracted = extract(
        Some("text/html"),
        b"<h2>T\xc3\xadtulo</h2>\n<p>uno\ndos</p>",
    )?;
    assert_eq!(
        extracted,
        Extracted {
            media_type: "text/html".to_owned(),
            text: "T\u{ed}tulo\n\nuno dos".to_owned(),
        }
    );
    Ok(())
}

#[test]
fn media_types_other_than_html_plain_and_json_are_refused() -> TestResult {
    assert_eq!(
        extract(Some("application/xml"), b"<a/>"),
        Err(ExtractError::UnsupportedMediaType)
    );
    assert_eq!(extract(None, b"x"), Err(ExtractError::MissingMediaType));
    assert_eq!(
        extract(Some("text/plain; charset=shift_jis"), b"x"),
        Err(ExtractError::Canonical(CanonicalError::UnsupportedCharset))
    );
    let site = serve_site(vec![
        ("/robots.txt", Reply::Status(404)),
        ("/feed", Reply::File("feed.xml", "application/xml")),
        ("/image", Reply::File("data.json", "image/png")),
        ("/untyped", Reply::Untyped("plain.txt")),
        (
            "/invalid",
            Reply::File("invalid-utf8.txt", "text/plain; charset=utf-8"),
        ),
        (
            "/sjis",
            Reply::File("plain.txt", "text/plain; charset=shift_jis"),
        ),
        ("/gzip", Reply::Encoded("plain.txt", "text/plain")),
    ])?;
    let fetcher = fetcher()?;
    let refused = [
        (
            "/feed",
            FetchError::Extract(ExtractError::UnsupportedMediaType),
        ),
        (
            "/image",
            FetchError::Extract(ExtractError::UnsupportedMediaType),
        ),
        (
            "/untyped",
            FetchError::Extract(ExtractError::MissingMediaType),
        ),
        (
            "/invalid",
            FetchError::Extract(ExtractError::Canonical(CanonicalError::InvalidEncoding)),
        ),
        (
            "/sjis",
            FetchError::Extract(ExtractError::Canonical(CanonicalError::UnsupportedCharset)),
        ),
        ("/gzip", FetchError::UnsupportedEncoding),
    ];
    for (path, error) in refused {
        assert_eq!(fetcher.fetch(&site.url(path)), Err(error), "{path}");
    }
    assert_eq!(
        FetchError::Extract(ExtractError::UnsupportedMediaType).status(),
        415
    );
    Ok(())
}

#[test]
fn robots_txt_is_honoured_for_the_x_websearch_token() -> TestResult {
    let site = serve_site(vec![
        robots(),
        (
            "/private/secret.html",
            Reply::File("private/secret.html", "text/html"),
        ),
        (
            "/private/open.html",
            Reply::File("private/open.html", "text/html"),
        ),
        ("/feed.xml", Reply::File("feed.xml", "text/plain")),
        ("/index.html", Reply::File("index.html", "text/html")),
    ])?;
    let fetcher = fetcher()?;
    assert_eq!(
        fetcher.fetch(&site.url("/private/secret.html")),
        Err(FetchError::RobotsDisallowed)
    );
    assert_eq!(
        fetcher.fetch(&site.url("/feed.xml")),
        Err(FetchError::RobotsDisallowed)
    );
    assert_eq!(
        fetcher.fetch(&site.url("/private/open.html"))?.text,
        "Open despite the private prefix"
    );
    fetcher.fetch(&site.url("/index.html"))?;
    assert_eq!(site.count("/private/secret.html"), 0);
    assert_eq!(site.count("/feed.xml"), 0);
    assert_eq!(site.count("/robots.txt"), 1);
    Ok(())
}

#[test]
fn robots_rules_match_by_the_longest_pattern() {
    let robots = Robots::parse(
        "User-agent: other\nDisallow: /\n\nUser-agent: X-WebSearch/1.0\nUser-agent: more\n\
         Disallow: /a\nAllow: /a/b\nDisallow: /*.gif$\nDisallow: /q?*secret\nAllow: /tie\n\
         Disallow: /tie\nDisallow:\n# comment\nUser-agent: *\nDisallow: /other",
    );
    assert!(!robots.allows("/a"));
    assert!(!robots.allows("/a/c"));
    assert!(robots.allows("/a/b/c"));
    assert!(!robots.allows("/img/x.gif"));
    assert!(robots.allows("/img/x.gif?size=2"));
    assert!(!robots.allows("/q?id=1&secret=2"));
    assert!(robots.allows("/tie"));
    assert!(robots.allows("/other"));
    assert!(robots.allows("/robots.txt"));
    let star = Robots::parse("User-agent: *\nDisallow: /private\n");
    assert!(!star.allows("/private/x"));
    assert!(star.allows("/public"));
    assert!(Robots::parse("").allows("/anything"));
    assert!(!Robots::disallow_all().allows("/anything"));
    assert!(Robots::allow_all().allows("/anything"));
}

#[test]
fn robots_txt_absence_allows_and_failure_refuses() -> TestResult {
    let missing = serve_site(vec![
        ("/robots.txt", Reply::Status(404)),
        ("/plain.txt", Reply::File("plain.txt", "text/plain")),
    ])?;
    let failing = serve_site(vec![
        ("/robots.txt", Reply::Status(503)),
        ("/plain.txt", Reply::File("plain.txt", "text/plain")),
    ])?;
    let fetcher = fetcher()?;
    fetcher.fetch(&missing.url("/plain.txt"))?;
    assert_eq!(
        fetcher.fetch(&failing.url("/plain.txt")),
        Err(FetchError::RobotsUnavailable)
    );
    assert_eq!(failing.count("/plain.txt"), 0);
    let closed = TcpListener::bind("127.0.0.1:0")?;
    let address = closed.local_addr()?;
    drop(closed);
    assert_eq!(
        fetcher.fetch(&format!("http://{address}/plain.txt")),
        Err(FetchError::RobotsUnavailable)
    );
    Ok(())
}

#[test]
fn forbidden_destinations_are_refused_after_name_resolution() -> TestResult {
    let site = serve_site(vec![
        robots(),
        ("/plain.txt", Reply::File("plain.txt", "text/plain")),
    ])?;
    let strict = Fetcher::new(FetchLimits {
        allow_loopback: false,
        ..loopback_limits()
    })?;
    let port = site.address.port();
    for url in [
        site.url("/plain.txt"),
        format!("http://localhost:{port}/plain.txt"),
        format!("http://[::1]:{port}/plain.txt"),
        format!("http://[::ffff:127.0.0.1]:{port}/plain.txt"),
    ] {
        assert_eq!(
            strict.fetch(&url),
            Err(FetchError::ForbiddenDestination),
            "{url}"
        );
    }
    assert_eq!(site.count("/robots.txt"), 0);
    assert_eq!(site.count("/plain.txt"), 0);

    let loose = fetcher()?;
    for url in [
        "http://10.1.2.3/",
        "http://172.16.0.1/",
        "http://192.168.1.1/",
        "http://169.254.169.254/latest",
        "http://224.0.0.1/",
        "http://0.0.0.0/",
        "http://100.64.0.1/",
        "http://255.255.255.255/",
        "https://[fe80::1]/",
        "https://[fc00::1]/",
        "https://[ff02::1]/",
        "https://[::]/",
        "https://[::ffff:10.0.0.1]/",
    ] {
        assert_eq!(
            loose.fetch(url),
            Err(FetchError::ForbiddenDestination),
            "{url}"
        );
    }
    assert_eq!(
        loose.fetch(&site.url("/plain.txt"))?.media_type,
        "text/plain"
    );

    let public: IpAddr = "93.184.216.34".parse()?;
    let public_v6: IpAddr = "2606:4700::1111".parse()?;
    assert!(destination_permitted(public, false));
    assert!(destination_permitted(public_v6, false));
    assert!(!destination_permitted("127.0.0.1".parse()?, false));
    assert!(destination_permitted("127.0.0.1".parse()?, true));
    assert!(!destination_permitted("10.0.0.1".parse()?, true));
    Ok(())
}

#[test]
fn only_http_and_https_urls_are_accepted() -> TestResult {
    let fetcher = fetcher()?;
    for url in [
        "ftp://paxeer.app/",
        "file:///etc/hosts",
        "javascript:alert(1)",
        "gopher://x/",
    ] {
        assert_eq!(
            fetcher.fetch(url),
            Err(FetchError::UnsupportedScheme),
            "{url}"
        );
    }
    let long = format!("http://paxeer.app/{}", "a".repeat(2_048));
    for url in [
        "paxeer.app/page",
        "http://user@paxeer.app/",
        "http:/paxeer.app",
        "http://",
        "http://paxeer.app:0/",
        "http://paxeer.app:99999/",
        "http://pax eer.app/",
        "http://paxeer.app/\u{e9}",
        "http://[::1/",
        long.as_str(),
    ] {
        assert_eq!(fetcher.fetch(url), Err(FetchError::InvalidUrl), "{url}");
    }
    let url = Url::parse("HTTPS://Paxeer.APP:443?q=1#frag")?;
    assert_eq!(url.to_string(), "https://paxeer.app/?q=1");
    assert_eq!(url.origin(), "https://paxeer.app:443");
    let base = Url::parse("http://paxeer.app:8080/docs/guide/page.html?x=1")?;
    assert_eq!(
        base.join("../api/./index.html")?.to_string(),
        "http://paxeer.app:8080/docs/api/index.html"
    );
    assert_eq!(
        base.join("/root")?.to_string(),
        "http://paxeer.app:8080/root"
    );
    assert_eq!(
        base.join("?y=2")?.to_string(),
        "http://paxeer.app:8080/docs/guide/page.html?y=2"
    );
    assert_eq!(
        base.join("//paxeer.app/other")?.to_string(),
        "http://paxeer.app/other"
    );
    assert_eq!(
        base.join("https://paxeer.app/abs")?.to_string(),
        "https://paxeer.app/abs"
    );
    assert_eq!(
        base.join("ftp://paxeer.app/"),
        Err(FetchError::UnsupportedScheme)
    );
    Ok(())
}

#[test]
fn redirects_are_followed_at_most_three_times_and_each_is_checked() -> TestResult {
    let site = serve_site(vec![
        robots(),
        ("/r1", Reply::Redirect(301, "/r2".to_owned())),
        ("/r2", Reply::Redirect(302, "r3".to_owned())),
        ("/r3", Reply::Redirect(307, "/plain.txt".to_owned())),
        ("/r0", Reply::Redirect(308, "/r1".to_owned())),
        ("/plain.txt", Reply::File("plain.txt", "text/plain")),
        (
            "/to-private",
            Reply::Redirect(303, "/private/secret.html".to_owned()),
        ),
        (
            "/to-internal",
            Reply::Redirect(302, "http://10.0.0.1/".to_owned()),
        ),
        (
            "/to-ftp",
            Reply::Redirect(302, "ftp://paxeer.app/".to_owned()),
        ),
        ("/no-location", Reply::Status(302)),
        ("/missing", Reply::Status(404)),
    ])?;
    let fetcher = fetcher()?;
    let page = fetcher.fetch(&site.url("/r1"))?;
    assert_eq!(page.url, site.url("/r1"));
    assert_eq!(page.final_url, site.url("/plain.txt"));
    assert_eq!(
        CanonicalContent::parse(&page.canonical)?.payload,
        site.url("/r1").as_bytes()
    );
    assert_eq!(
        fetcher.fetch(&site.url("/r0")),
        Err(FetchError::TooManyRedirects)
    );
    let stricter = Fetcher::new(FetchLimits {
        max_redirects: 2,
        ..loopback_limits()
    })?;
    assert_eq!(
        stricter.fetch(&site.url("/r1")),
        Err(FetchError::TooManyRedirects)
    );
    assert_eq!(
        fetcher.fetch(&site.url("/to-private")),
        Err(FetchError::RobotsDisallowed)
    );
    assert_eq!(site.count("/private/secret.html"), 0);
    assert_eq!(
        fetcher.fetch(&site.url("/to-internal")),
        Err(FetchError::ForbiddenDestination)
    );
    assert_eq!(
        fetcher.fetch(&site.url("/to-ftp")),
        Err(FetchError::UnsupportedScheme)
    );
    assert_eq!(
        fetcher.fetch(&site.url("/no-location")),
        Err(FetchError::MalformedResponse)
    );
    assert_eq!(
        fetcher.fetch(&site.url("/missing")),
        Err(FetchError::Status(404))
    );
    Ok(())
}

#[test]
fn bodies_over_two_mebibytes_are_refused() -> TestResult {
    let site = serve_site(vec![
        robots(),
        (
            "/exact",
            Reply::Filled {
                bytes: MIB2,
                declared: true,
            },
        ),
        (
            "/declared",
            Reply::Filled {
                bytes: MIB2 + 1,
                declared: true,
            },
        ),
        (
            "/undeclared",
            Reply::Filled {
                bytes: MIB2 + 1,
                declared: false,
            },
        ),
        ("/chunked", Reply::ChunkedFilled(MIB2 + 1)),
        ("/chunked-exact", Reply::ChunkedFilled(MIB2)),
    ])?;
    let fetcher = fetcher()?;
    assert_eq!(fetcher.fetch(&site.url("/exact"))?.text.len(), MIB2);
    assert_eq!(fetcher.fetch(&site.url("/chunked-exact"))?.text.len(), MIB2);
    for path in ["/declared", "/undeclared", "/chunked"] {
        assert_eq!(
            fetcher.fetch(&site.url(path)),
            Err(FetchError::BodyTooLarge),
            "{path}"
        );
    }
    let small = Fetcher::new(FetchLimits {
        max_body_bytes: 1_024,
        ..loopback_limits()
    })?;
    assert_eq!(
        small.fetch(&site.url("/exact")),
        Err(FetchError::BodyTooLarge)
    );
    Ok(())
}

#[test]
fn the_total_time_limit_stops_slow_and_silent_servers() -> TestResult {
    let site = serve_site(vec![
        robots(),
        ("/drip", Reply::Drip),
        ("/silent", Reply::Silent),
    ])?;
    let quick = Fetcher::new(FetchLimits {
        connect_timeout_ms: 300,
        total_timeout_ms: 600,
        ..loopback_limits()
    })?;
    for path in ["/drip", "/silent"] {
        let started = Instant::now();
        assert_eq!(
            quick.fetch(&site.url(path)),
            Err(FetchError::Timeout),
            "{path}"
        );
        assert!(started.elapsed() < Duration::from_millis(1_500), "{path}");
    }
    assert_eq!(FetchError::Timeout.status(), 504);
    Ok(())
}

#[test]
fn limits_beyond_the_decision_are_refused() {
    let base = loopback_limits();
    for limits in [
        FetchLimits {
            connect_timeout_ms: 3_001,
            ..base
        },
        FetchLimits {
            connect_timeout_ms: 0,
            ..base
        },
        FetchLimits {
            total_timeout_ms: 10_001,
            ..base
        },
        FetchLimits {
            connect_timeout_ms: 2_000,
            total_timeout_ms: 1_000,
            ..base
        },
        FetchLimits {
            max_body_bytes: BODY_LIMIT_BYTES + 1,
            ..base
        },
        FetchLimits {
            max_body_bytes: 0,
            ..base
        },
        FetchLimits {
            max_redirects: 4,
            ..base
        },
    ] {
        assert!(matches!(
            Fetcher::new(limits),
            Err(FetchError::InvalidLimits)
        ));
    }
    assert!(Fetcher::new(base).is_ok());
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> std::io::Result<Self> {
        let path =
            std::env::temp_dir().join(format!("x-websearch-fetch-{}-{name}", std::process::id()));
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

fn sidecar(
    fetcher: Fetcher,
    store: ContentStore,
) -> Result<RunningServer, Box<dyn std::error::Error>> {
    let (fetcher, store) = (Arc::new(fetcher), Arc::new(store));
    let mut routes = RouteTable::new();
    let (fetch_store, content_store) = (Arc::clone(&store), store);
    routes.set(Route::Fetch, move |request: &Request| {
        fetch_route(&fetcher, &fetch_store, request)
    })?;
    routes.set(Route::Content, move |request: &Request| {
        content_store.handle(request)
    })?;
    Ok(Server::bind("127.0.0.1:0".parse()?, Limits::default(), routes)?.spawn()?)
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
    let status = std::str::from_utf8(&bytes[..split])?
        .split(' ')
        .nth(1)
        .ok_or("no status")?
        .parse()?;
    Ok((status, bytes[split + 4..].to_vec()))
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_') {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

#[test]
fn the_fetch_route_answers_and_writes_the_content_store() -> TestResult {
    let site = serve_site(vec![
        robots(),
        ("/index.html", Reply::File("index.html", "text/html")),
    ])?;
    let scratch = Scratch::new("route")?;
    let server = sidecar(fetcher()?, ContentStore::open(&scratch.0, &[])?)?;
    let address = server.local_addr();
    let url = site.url("/index.html");
    let (status, body) = get(address, &format!("/fetch?url={}", encode(&url)))?;
    assert_eq!(status, 200);
    let answer: serde_json::Value = serde_json::from_slice(&body)?;
    let vectors = vectors()?;
    let text = vectors
        .vectors
        .iter()
        .find(|vector| vector.path == "/index.html")
        .map(|vector| vector.text.clone())
        .ok_or("vector")?;
    assert_eq!(answer["text"], text.as_str());
    assert_eq!(answer["media_type"], "text/html");
    assert_eq!(answer["url"], url.as_str());
    assert_eq!(answer["length"], text.len());
    let canonical = canonical_bytes(ContentKind::Fetch, url.as_bytes(), "text/html", &text)?;
    let digest = digest_hex(&content_digest(&canonical));
    assert_eq!(answer["digest"], digest.as_str());
    let (status, stored) = get(address, &format!("/content/{digest}"))?;
    assert_eq!(status, 200);
    assert_eq!(stored, canonical);

    let error = |body: &[u8]| -> Result<String, Box<dyn std::error::Error>> {
        let value: serde_json::Value = serde_json::from_slice(body)?;
        Ok(value["error"].as_str().ok_or("code")?.to_owned())
    };
    let (status, body) = get(address, "/fetch")?;
    assert_eq!((status, error(&body)?.as_str()), (400, "missing_url"));
    let (status, body) = get(address, "/fetch?url=a&url=b")?;
    assert_eq!((status, error(&body)?.as_str()), (400, "duplicate_url"));
    let (status, body) = get(
        address,
        &format!("/fetch?url={}", encode("http://10.0.0.1/")),
    )?;
    assert_eq!(
        (status, error(&body)?.as_str()),
        (403, "forbidden_destination")
    );
    let (status, body) = get(
        address,
        &format!("/fetch?url={}", encode("ftp://paxeer.app/")),
    )?;
    assert_eq!(
        (status, error(&body)?.as_str()),
        (400, "unsupported_scheme")
    );
    server.shutdown()?;
    Ok(())
}
