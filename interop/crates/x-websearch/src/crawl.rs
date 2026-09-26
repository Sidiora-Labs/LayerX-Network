use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::canonical;
use crate::config::{CrawlConfig, MAX_DEPTH, MAX_PAGES_PER_CYCLE, MAX_POLITENESS_DELAY_MS};
use crate::extract::{self, ExtractError};
use crate::fetch::{FetchError, Fetcher, HttpClient, Url};
use crate::index::{IndexError, WebIndex, MAX_TITLE_BYTES};

/// The most links taken from one page.
pub const MAX_LINKS_PER_PAGE: usize = 1_000;

const HTML: &str = "text/html";

/// How far one crawl cycle may go: pages per cycle, pages per host, the
/// maximum link depth from a seed, and the delay between two requests to
/// the same host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CrawlBudget {
    pub pages_per_cycle: u32,
    pub pages_per_host: u32,
    pub max_depth: u32,
    pub politeness_delay: Duration,
}

impl CrawlBudget {
    #[must_use]
    pub const fn from_config(config: &CrawlConfig) -> Self {
        Self {
            pages_per_cycle: config.pages_per_cycle,
            pages_per_host: config.pages_per_host,
            max_depth: config.max_depth,
            politeness_delay: config.politeness_delay(),
        }
    }

    fn valid(&self) -> bool {
        (1..=MAX_PAGES_PER_CYCLE).contains(&self.pages_per_cycle)
            && (1..=self.pages_per_cycle).contains(&self.pages_per_host)
            && self.max_depth <= MAX_DEPTH
            && !self.politeness_delay.is_zero()
            && self.politeness_delay <= Duration::from_millis(MAX_POLITENESS_DELAY_MS)
    }
}

impl From<&CrawlConfig> for CrawlBudget {
    fn from(config: &CrawlConfig) -> Self {
        Self::from_config(config)
    }
}

#[derive(Debug)]
pub enum CrawlError {
    InvalidBudget,
    Fetch(FetchError),
    Index(IndexError),
}

impl std::fmt::Display for CrawlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidBudget => f.write_str("invalid_crawl_budget"),
            Self::Fetch(error) => error.fmt(f),
            Self::Index(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for CrawlError {}

impl From<IndexError> for CrawlError {
    fn from(error: IndexError) -> Self {
        Self::Index(error)
    }
}

/// What a visit to one page came to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PageOutcome {
    /// The page was indexed and `links` new URLs were queued from it.
    Indexed { links: usize },
    /// The page was indexed but its links could not be read.
    IndexedWithoutLinks(FetchError),
    /// The fetch path refused the page; nothing was indexed.
    Refused(FetchError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CrawledPage {
    pub url: String,
    pub depth: u32,
    pub outcome: PageOutcome,
}

/// The pages one cycle visited, in visiting order, and how many queued URLs
/// the budget left for a later cycle.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CrawlReport {
    pub pages: Vec<CrawledPage>,
    pub deferred: usize,
}

impl CrawlReport {
    /// The visited URLs in visiting order.
    #[must_use]
    pub fn visited(&self) -> Vec<&str> {
        self.pages.iter().map(|page| page.url.as_str()).collect()
    }

    /// The pages that were indexed.
    pub fn indexed(&self) -> impl Iterator<Item = &CrawledPage> {
        self.pages.iter().filter(|page| {
            matches!(
                page.outcome,
                PageOutcome::Indexed { .. } | PageOutcome::IndexedWithoutLinks(_)
            )
        })
    }

    /// The refusal a visited URL met, if any.
    #[must_use]
    pub fn refusal(&self, url: &str) -> Option<FetchError> {
        self.pages
            .iter()
            .find(|page| page.url == url)
            .and_then(|page| match page.outcome {
                PageOutcome::Refused(error) => Some(error),
                _ => None,
            })
    }
}

/// Walks the seed list breadth first through the fetch path, within the
/// budget, and writes each page into the index.
pub struct Crawler {
    budget: CrawlBudget,
    fetcher: Arc<Fetcher>,
    client: HttpClient,
    index: Arc<WebIndex>,
    last_request: HashMap<String, Instant>,
}

struct Frontier {
    queue: VecDeque<(String, u32)>,
    seen: HashSet<String>,
}

impl Frontier {
    fn push(&mut self, url: String, depth: u32) -> bool {
        if self.seen.insert(url.clone()) {
            self.queue.push_back((url, depth));
            true
        } else {
            false
        }
    }
}

fn normalise(url: &str) -> String {
    Url::parse(url).map_or_else(|_| url.to_owned(), |parsed| parsed.to_string())
}

fn host_of(url: &str) -> String {
    Url::parse(url)
        .map(|parsed| parsed.host)
        .unwrap_or_default()
}

/// The title of an HTML page: the first line of its canonical text, which
/// is the `title` element when the page has one.
#[must_use]
pub fn title_of(text: &str) -> String {
    let line = text.lines().next().unwrap_or_default().trim();
    let mut end = line.len().min(MAX_TITLE_BYTES);
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    line[..end].to_owned()
}

fn tag_end(html: &str, from: usize) -> usize {
    let mut quote = None;
    for (offset, byte) in html.as_bytes()[from..].iter().enumerate() {
        match (quote, byte) {
            (None, b'"' | b'\'') => quote = Some(*byte),
            (Some(open), _) if open == *byte => quote = None,
            (None, b'>') => return from + offset,
            _ => {}
        }
    }
    html.len()
}

fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let bytes = tag.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        while index < bytes.len() && (bytes[index].is_ascii_whitespace() || bytes[index] == b'/') {
            index += 1;
        }
        let start = index;
        while index < bytes.len()
            && !bytes[index].is_ascii_whitespace()
            && !matches!(bytes[index], b'=' | b'/')
        {
            index += 1;
        }
        let key = &tag[start..index];
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        let mut value = "";
        if bytes.get(index) == Some(&b'=') {
            index += 1;
            while index < bytes.len() && bytes[index].is_ascii_whitespace() {
                index += 1;
            }
            match bytes.get(index) {
                Some(&quote @ (b'"' | b'\'')) => {
                    let open = index + 1;
                    let close = tag[open..]
                        .find(char::from(quote))
                        .map_or(tag.len(), |at| open + at);
                    value = &tag[open..close];
                    index = close + 1;
                }
                Some(_) => {
                    let open = index;
                    while index < bytes.len() && !bytes[index].is_ascii_whitespace() {
                        index += 1;
                    }
                    value = &tag[open..index];
                }
                None => {}
            }
        }
        if key.eq_ignore_ascii_case(name) {
            return Some(value);
        }
        if start == index {
            index += 1;
        }
    }
    None
}

/// The `href` values of the `a` elements of an HTML document, in document
/// order, with character references decoded.
#[must_use]
pub fn links(html: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(offset) = lower[from..].find("<a") {
        let start = from + offset + 2;
        if !lower
            .as_bytes()
            .get(start)
            .is_some_and(u8::is_ascii_whitespace)
        {
            from = start;
            continue;
        }
        let end = tag_end(html, start);
        if let Some(href) = attribute(&html[start..end], "href") {
            let href = extract::html_text(href).trim().to_owned();
            if !href.is_empty() {
                found.push(href);
            }
        }
        if found.len() >= MAX_LINKS_PER_PAGE {
            break;
        }
        from = end;
    }
    found
}

impl Crawler {
    /// # Errors
    /// Refuses a zero or out-of-range budget and a client that cannot be
    /// built under the fetcher's limits.
    pub fn new(
        budget: CrawlBudget,
        fetcher: Arc<Fetcher>,
        index: Arc<WebIndex>,
    ) -> Result<Self, CrawlError> {
        if !budget.valid() {
            return Err(CrawlError::InvalidBudget);
        }
        let client =
            HttpClient::new(fetcher.limits().connect_timeout()).map_err(CrawlError::Fetch)?;
        Ok(Self {
            budget,
            fetcher,
            client,
            index,
            last_request: HashMap::new(),
        })
    }

    #[must_use]
    pub const fn budget(&self) -> &CrawlBudget {
        &self.budget
    }

    /// Crawls the seeds once: breadth first, each URL at most once per
    /// cycle, no more than the pages per cycle and per host, links followed
    /// only from pages above the maximum depth, and at least the politeness
    /// delay between two requests to one host. Every page goes through the
    /// fetch path and its refusals, robots.txt for `x-websearch` included.
    /// Indexed pages are committed before the cycle returns.
    ///
    /// # Errors
    /// Returns the error writing or committing the index.
    pub fn run_cycle(&mut self, seeds: &[String]) -> Result<CrawlReport, CrawlError> {
        let mut frontier = Frontier {
            queue: VecDeque::new(),
            seen: HashSet::new(),
        };
        for seed in seeds {
            frontier.push(normalise(seed), 0);
        }
        let mut per_host: HashMap<String, u32> = HashMap::new();
        let mut report = CrawlReport::default();
        let mut visited = 0_u32;
        while let Some((url, depth)) = frontier.queue.pop_front() {
            if visited >= self.budget.pages_per_cycle {
                report.deferred += 1 + frontier.queue.len();
                break;
            }
            let host = host_of(&url);
            let count = per_host.entry(host.clone()).or_insert(0);
            if *count >= self.budget.pages_per_host {
                report.deferred += 1;
                continue;
            }
            *count += 1;
            visited += 1;
            let follow = depth < self.budget.max_depth && visited < self.budget.pages_per_cycle;
            let outcome = self.visit(&url, &host, depth, follow, &mut frontier)?;
            report.pages.push(CrawledPage {
                url,
                depth,
                outcome,
            });
        }
        self.index.commit()?;
        Ok(report)
    }

    fn wait_for(&self, host: &str) {
        if let Some(last) = self.last_request.get(host) {
            let elapsed = last.elapsed();
            if elapsed < self.budget.politeness_delay {
                std::thread::sleep(self.budget.politeness_delay - elapsed);
            }
        }
    }

    fn visit(
        &mut self,
        url: &str,
        host: &str,
        depth: u32,
        follow: bool,
        frontier: &mut Frontier,
    ) -> Result<PageOutcome, CrawlError> {
        self.wait_for(host);
        let fetched = self.fetcher.fetch(url);
        self.last_request.insert(host.to_owned(), Instant::now());
        let page = match fetched {
            Ok(page) => page,
            Err(error) => return Ok(PageOutcome::Refused(error)),
        };
        let title = if page.media_type == HTML {
            title_of(&page.text)
        } else {
            String::new()
        };
        match self.index.put(&page.final_url, &title, &page.text) {
            Ok(()) => {}
            Err(IndexError::InvalidUrl) => return Ok(PageOutcome::Refused(FetchError::InvalidUrl)),
            Err(error) => return Err(error.into()),
        }
        frontier.seen.insert(page.final_url.clone());
        if !follow || page.media_type != HTML {
            return Ok(PageOutcome::Indexed { links: 0 });
        }
        match self.page_links(&page.final_url) {
            Ok(found) => {
                let links = found
                    .into_iter()
                    .filter(|link| frontier.push(link.clone(), depth + 1))
                    .count();
                Ok(PageOutcome::Indexed { links })
            }
            Err(error) => Ok(PageOutcome::IndexedWithoutLinks(error)),
        }
    }

    /// Reads the links of a page the fetch path has just admitted: the same
    /// destination check and limits, the page's final URL, no redirects.
    fn page_links(&mut self, final_url: &str) -> Result<Vec<String>, FetchError> {
        let url = Url::parse(final_url)?;
        let address = self.fetcher.destination(&url)?;
        let limits = *self.fetcher.limits();
        let body_limit = usize::try_from(limits.max_body_bytes).unwrap_or(usize::MAX);
        self.wait_for(&url.host);
        let response = self.client.get(
            &url,
            address,
            Instant::now() + limits.total_timeout(),
            body_limit,
            &[("Accept", HTML)],
        );
        self.last_request.insert(url.host.clone(), Instant::now());
        let response = response?;
        if !(200..300).contains(&response.status) {
            return Err(FetchError::Status(response.status));
        }
        let content_type = response
            .header("content-type")
            .ok_or(FetchError::Extract(ExtractError::MissingMediaType))?;
        let (media_type, charset) =
            extract::parse_content_type(content_type).map_err(FetchError::Extract)?;
        if media_type != HTML {
            return Err(FetchError::Extract(ExtractError::UnsupportedMediaType));
        }
        let html =
            canonical::decode(&response.body, charset.as_deref()).map_err(FetchError::Canonical)?;
        Ok(links(&html)
            .iter()
            .filter_map(|href| url.join(href).ok())
            .map(|link| link.to_string())
            .collect())
    }
}
