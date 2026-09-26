use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use x_websearch::config::{CrawlConfig, FetchLimits, BODY_LIMIT_BYTES, MAX_DEPTH};
use x_websearch::crawl::{links, title_of, CrawlBudget, CrawlError, Crawler, PageOutcome};
use x_websearch::fetch::{FetchError, Fetcher};
use x_websearch::index::WebIndex;
use x_websearch::search::search;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn fixture_site() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/crawl-site")
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> std::io::Result<Self> {
        let path =
            std::env::temp_dir().join(format!("x-websearch-crawl-{}-{name}", std::process::id()));
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

/// A loopback server answering GET with the files under a directory and
/// recording each request path with the time it arrived.
struct Site {
    address: SocketAddr,
    requests: Arc<Mutex<Vec<(String, Instant)>>>,
    stop: Arc<AtomicBool>,
    acceptor: Option<thread::JoinHandle<()>>,
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("json") => "application/json",
        _ => "text/plain; charset=utf-8",
    }
}

fn respond(stream: &mut TcpStream, root: &Path, requests: &Mutex<Vec<(String, Instant)>>) {
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
        requests.push((path.clone(), Instant::now()));
    }
    let file = root.join(path.trim_start_matches('/'));
    let found = if path.split('/').any(|segment| segment == "..") {
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
    fn start(root: &Path, ip: &str) -> std::io::Result<Self> {
        let listener = TcpListener::bind(format!("{ip}:0"))?;
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

    fn requests(&self) -> Vec<(String, Instant)> {
        self.requests
            .lock()
            .map(|requests| requests.clone())
            .unwrap_or_default()
    }

    fn count(&self, path: &str) -> usize {
        self.requests()
            .iter()
            .filter(|(requested, _)| requested == path)
            .count()
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

const fn limits(allow_loopback: bool) -> FetchLimits {
    FetchLimits {
        connect_timeout_ms: 3_000,
        total_timeout_ms: 10_000,
        max_body_bytes: BODY_LIMIT_BYTES,
        max_redirects: 3,
        allow_loopback,
    }
}

const fn budget(
    pages_per_cycle: u32,
    pages_per_host: u32,
    max_depth: u32,
    delay_ms: u64,
) -> CrawlBudget {
    CrawlBudget {
        pages_per_cycle,
        pages_per_host,
        max_depth,
        politeness_delay: Duration::from_millis(delay_ms),
    }
}

fn crawler(
    scratch: &Scratch,
    budget: CrawlBudget,
    allow_loopback: bool,
) -> Result<(Crawler, Arc<WebIndex>), Box<dyn std::error::Error>> {
    let index = Arc::new(WebIndex::open(&scratch.0)?);
    let fetcher = Arc::new(Fetcher::new(limits(allow_loopback))?);
    Ok((Crawler::new(budget, fetcher, Arc::clone(&index))?, index))
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[test]
fn a_crawl_walks_the_site_breadth_first_to_the_maximum_depth_under_robots_txt() -> TestResult {
    let scratch = Scratch::new("walk")?;
    let site = Site::start(&fixture_site(), "127.0.0.1")?;
    let (mut crawler, index) = crawler(&scratch, budget(100, 100, 2, 1), true)?;
    let report = crawler.run_cycle(&[site.url("/index.html")])?;

    let expected = [
        ("/index.html", 0),
        ("/guides/tides.html", 1),
        ("/guides/boats.html", 1),
        ("/notes.txt", 1),
        ("/private/ledger.html", 1),
        ("/guides/deep/currents.html", 2),
        ("/guides/deep/knots.html", 2),
    ];
    let visited: Vec<(String, u32)> = report
        .pages
        .iter()
        .map(|page| (page.url.clone(), page.depth))
        .collect();
    let wanted: Vec<(String, u32)> = expected
        .iter()
        .map(|(path, depth)| (site.url(path), *depth))
        .collect();
    assert_eq!(visited, wanted);
    assert_eq!(report.deferred, 0);
    assert_eq!(report.pages[0].outcome, PageOutcome::Indexed { links: 4 });
    assert_eq!(report.pages[1].outcome, PageOutcome::Indexed { links: 1 });
    assert_eq!(report.pages[2].outcome, PageOutcome::Indexed { links: 1 });
    assert_eq!(report.pages[5].outcome, PageOutcome::Indexed { links: 0 });

    assert_eq!(
        report.refusal(&site.url("/private/ledger.html")),
        Some(FetchError::RobotsDisallowed)
    );
    assert_eq!(site.count("/private/ledger.html"), 0);
    assert_eq!(site.count("/guides/deep/abyss.html"), 0);
    assert_eq!(site.count("/robots.txt"), 1);
    assert_eq!(site.count("/index.html"), 2);
    assert_eq!(site.count("/notes.txt"), 1);
    assert_eq!(site.count("/guides/deep/currents.html"), 1);

    assert_eq!(report.indexed().count(), 6);
    assert_eq!(index.num_docs(), 6);
    for (path, _) in expected
        .iter()
        .filter(|(path, _)| !path.starts_with("/private"))
    {
        assert_eq!(index.documents_for(&site.url(path))?, 1, "{path}");
    }
    let almanac = search(&index, "almanac")?;
    assert_eq!(almanac[0].result.url, site.url("/index.html"));
    assert_eq!(almanac[0].result.title, "Harbour Almanac");
    let notes = search(&index, "moorings list")?;
    assert_eq!(notes[0].result.url, site.url("/notes.txt"));
    assert_eq!(notes[0].result.title, "");
    assert!(search(&index, "fees")?.is_empty());
    assert!(search(&index, "soundings")?.is_empty());
    Ok(())
}

#[test]
fn a_maximum_depth_of_zero_visits_only_the_seeds() -> TestResult {
    let scratch = Scratch::new("depth")?;
    let site = Site::start(&fixture_site(), "127.0.0.1")?;
    let (mut crawler, index) = crawler(&scratch, budget(100, 100, 0, 1), true)?;
    let report = crawler.run_cycle(&[site.url("/index.html"), site.url("/guides/boats.html")])?;
    assert_eq!(
        report.visited(),
        [site.url("/index.html"), site.url("/guides/boats.html")]
    );
    assert_eq!(site.count("/index.html"), 1);
    assert_eq!(site.count("/guides/boats.html"), 1);
    assert_eq!(site.count("/guides/tides.html"), 0);
    assert_eq!(index.num_docs(), 2);
    Ok(())
}

#[test]
fn the_pages_per_cycle_budget_stops_the_walk_and_defers_the_rest() -> TestResult {
    let scratch = Scratch::new("cycle")?;
    let site = Site::start(&fixture_site(), "127.0.0.1")?;
    let (mut crawler, index) = crawler(&scratch, budget(3, 3, 2, 1), true)?;
    let report = crawler.run_cycle(&[site.url("/index.html")])?;
    assert_eq!(
        report.visited(),
        [
            site.url("/index.html"),
            site.url("/guides/tides.html"),
            site.url("/guides/boats.html"),
        ]
    );
    assert_eq!(report.deferred, 3);
    assert_eq!(site.count("/guides/boats.html"), 1);
    assert_eq!(site.count("/notes.txt"), 0);
    assert_eq!(index.num_docs(), 3);
    Ok(())
}

#[test]
fn the_pages_per_host_budget_holds_each_host_separately() -> TestResult {
    let scratch = Scratch::new("host")?;
    let first = Site::start(&fixture_site(), "127.0.0.1")?;
    let second = Site::start(&fixture_site(), "127.0.0.2")?;
    let (mut crawler, index) = crawler(&scratch, budget(100, 2, 2, 1), true)?;
    let report = crawler.run_cycle(&[first.url("/index.html"), second.url("/index.html")])?;
    assert_eq!(
        report.visited(),
        [
            first.url("/index.html"),
            second.url("/index.html"),
            first.url("/guides/tides.html"),
            second.url("/guides/tides.html"),
        ]
    );
    assert_eq!(report.deferred, 8);
    for site in [&first, &second] {
        assert_eq!(site.count("/guides/boats.html"), 0);
        assert_eq!(site.count("/notes.txt"), 0);
    }
    assert_eq!(index.num_docs(), 4);
    Ok(())
}

#[test]
fn requests_to_one_host_are_spaced_by_the_politeness_delay() -> TestResult {
    let scratch = Scratch::new("polite")?;
    let site = Site::start(&fixture_site(), "127.0.0.1")?;
    let delay = Duration::from_millis(200);
    let (mut crawler, _) = crawler(&scratch, budget(100, 100, 1, 200), true)?;
    let started = Instant::now();
    let report = crawler.run_cycle(&[site.url("/index.html")])?;
    let elapsed = started.elapsed();
    assert_eq!(report.pages.len(), 5);
    let pages: Vec<(String, Instant)> = site
        .requests()
        .into_iter()
        .filter(|(path, _)| path != "/robots.txt")
        .collect();
    let paths: Vec<&str> = pages.iter().map(|(path, _)| path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "/index.html",
            "/index.html",
            "/guides/tides.html",
            "/guides/boats.html",
            "/notes.txt",
        ]
    );
    for pair in pages.windows(2) {
        let gap = pair[1].1.duration_since(pair[0].1);
        assert!(gap >= delay, "{} after {}: {gap:?}", pair[1].0, pair[0].0);
    }
    assert!(elapsed >= delay * 4);
    Ok(())
}

#[test]
fn a_forbidden_destination_is_refused_through_the_fetch_path() -> TestResult {
    let scratch = Scratch::new("forbidden")?;
    let site = Site::start(&fixture_site(), "127.0.0.1")?;
    let (mut crawler, index) = crawler(&scratch, budget(100, 100, 2, 1), false)?;
    let seed = site.url("/index.html");
    let report = crawler.run_cycle(std::slice::from_ref(&seed))?;
    assert_eq!(report.visited(), [seed.as_str()]);
    assert_eq!(
        report.refusal(&seed),
        Some(FetchError::ForbiddenDestination)
    );
    assert!(site.requests().is_empty());
    assert_eq!(index.num_docs(), 0);

    let report = crawler.run_cycle(&["ftp://files/readme.txt".to_owned()])?;
    assert_eq!(
        report.refusal("ftp://files/readme.txt"),
        Some(FetchError::UnsupportedScheme)
    );
    Ok(())
}

#[test]
fn a_re_crawl_replaces_the_document_of_each_page() -> TestResult {
    let scratch = Scratch::new("recrawl")?;
    let root = scratch.0.join("site");
    copy_tree(&fixture_site(), &root)?;
    let site = Site::start(&root, "127.0.0.1")?;
    let data = scratch.0.join("data");
    std::fs::create_dir_all(&data)?;
    let index = Arc::new(WebIndex::open(&data)?);
    let fetcher = Arc::new(Fetcher::new(limits(true))?);
    let mut crawler = Crawler::new(budget(100, 100, 2, 1), fetcher, Arc::clone(&index))?;
    let seeds = [site.url("/index.html")];
    let tides = site.url("/guides/tides.html");

    crawler.run_cycle(&seeds)?;
    assert_eq!(index.num_docs(), 6);
    let before = search(&index, "springs")?;
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].result.url, tides);

    let page = root.join("guides/tides.html");
    let text = std::fs::read_to_string(&page)?;
    std::fs::write(&page, text.replace("Springs", "Neaps"))?;
    let report = crawler.run_cycle(&seeds)?;
    assert_eq!(report.indexed().count(), 6);
    assert_eq!(index.num_docs(), 6);
    assert_eq!(index.documents_for(&tides)?, 1);
    assert!(search(&index, "springs")?.is_empty());
    let after = search(&index, "neaps")?;
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].result.url, tides);
    assert_eq!(site.count("/guides/tides.html"), 4);
    Ok(())
}

#[test]
fn a_budget_out_of_range_is_refused_and_the_configuration_maps_onto_it() -> TestResult {
    let scratch = Scratch::new("budget")?;
    let index = Arc::new(WebIndex::open(&scratch.0)?);
    let fetcher = Arc::new(Fetcher::new(limits(true))?);
    for refused in [
        budget(0, 0, 1, 1),
        budget(5, 0, 1, 1),
        budget(5, 6, 1, 1),
        budget(5, 5, MAX_DEPTH + 1, 1),
        budget(5, 5, 1, 0),
        budget(5, 5, 1, 3_600_001),
    ] {
        assert!(matches!(
            Crawler::new(refused, Arc::clone(&fetcher), Arc::clone(&index)),
            Err(CrawlError::InvalidBudget)
        ));
    }
    let config = CrawlConfig {
        pages_per_cycle: 500,
        pages_per_host: 50,
        max_depth: 3,
        politeness_delay_ms: 1_500,
    };
    let mapped = CrawlBudget::from(&config);
    assert_eq!(mapped, budget(500, 50, 3, 1_500));
    let crawler = Crawler::new(mapped, fetcher, index)?;
    assert_eq!(crawler.budget(), &mapped);
    Ok(())
}

#[test]
fn links_are_read_from_anchor_elements_in_document_order() -> TestResult {
    let html = std::fs::read_to_string(fixture_site().join("index.html"))?;
    assert_eq!(
        links(&html),
        [
            "guides/tides.html",
            "guides/boats.html",
            "notes.txt",
            "private/ledger.html",
            "mailto:office",
            "#top",
            "guides/tides.html#springs",
        ]
    );
    assert_eq!(
        links("<A HREF=\"list?x=1&amp;y=2\">a</A><abbr href=\"no\">b</abbr><a name=top>c</a><a\nhref = 'next.html' >d</a>"),
        ["list?x=1&y=2", "next.html"]
    );
    assert_eq!(title_of("Tide Tables\n\nSprings follow"), "Tide Tables");
    assert_eq!(title_of(""), "");
    Ok(())
}
