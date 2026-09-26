use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::sync::Arc;

use serde::Serialize;
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, Occur, Query, TermQuery};
use tantivy::schema::{IndexRecordOption, Value as _};
use tantivy::snippet::SnippetGenerator;
use tantivy::tokenizer::TokenStream as _;
use tantivy::{Score, TantivyDocument, Term};

use crate::canonical::{self, CanonicalError, ContentKind};
use crate::content::{ContentStore, PaymentHook};
use crate::index::{IndexError, WebIndex};
use crate::server::{QueryError, Request, Response, Route, RouteError, RouteTable};

/// The most results a search returns.
pub const MAX_RESULTS: usize = 10;

/// The longest query accepted, in bytes.
pub const MAX_QUERY_BYTES: usize = 512;

/// The most distinct terms a query is searched with.
pub const MAX_QUERY_TERMS: usize = 32;

/// The longest snippet, in characters.
pub const SNIPPET_CHARS: usize = 150;

/// The media type of the search text in the canonical bytes.
pub const SEARCH_MEDIA_TYPE: &str = "application/json";

#[derive(Debug)]
pub enum SearchError {
    EmptyQuery,
    QueryTooLong,
    TooManyTerms,
    Index(IndexError),
}

impl SearchError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::EmptyQuery => "empty_query",
            Self::QueryTooLong => "query_too_long",
            Self::TooManyTerms => "too_many_terms",
            Self::Index(error) => error.code(),
        }
    }

    #[must_use]
    pub const fn status(&self) -> u16 {
        match self {
            Self::EmptyQuery | Self::QueryTooLong | Self::TooManyTerms => 400,
            Self::Index(_) => 500,
        }
    }
}

impl std::fmt::Display for SearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Index(error) => error.fmt(f),
            _ => f.write_str(self.code()),
        }
    }
}

impl std::error::Error for SearchError {}

impl From<IndexError> for SearchError {
    fn from(error: IndexError) -> Self {
        Self::Index(error)
    }
}

impl From<tantivy::TantivyError> for SearchError {
    fn from(error: tantivy::TantivyError) -> Self {
        Self::Index(IndexError::Tantivy(error))
    }
}

/// One search result as it appears in the canonical bytes: url, title and
/// snippet, in that order.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SearchResult {
    pub url: String,
    pub title: String,
    pub snippet: String,
}

/// A result with the score it was ranked by. The score orders the results
/// and never enters the canonical bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct ScoredResult {
    pub score: Score,
    pub result: SearchResult,
}

/// The distinct terms of a query as the body field tokenises them.
///
/// # Errors
/// Refuses an over-long query, a query with no terms and one with more than
/// [`MAX_QUERY_TERMS`] distinct terms.
pub fn query_terms(index: &WebIndex, query: &str) -> Result<Vec<String>, SearchError> {
    if query.len() > MAX_QUERY_BYTES {
        return Err(SearchError::QueryTooLong);
    }
    let mut analyzer = index.index().tokenizer_for_field(index.fields().body)?;
    let mut stream = analyzer.token_stream(query);
    let mut terms = BTreeSet::new();
    while stream.advance() {
        terms.insert(stream.token().text.clone());
    }
    if terms.is_empty() {
        return Err(SearchError::EmptyQuery);
    }
    if terms.len() > MAX_QUERY_TERMS {
        return Err(SearchError::TooManyTerms);
    }
    Ok(terms.into_iter().collect())
}

fn build_query(index: &WebIndex, terms: &[String]) -> BooleanQuery {
    let fields = index.fields();
    let clauses: Vec<(Occur, Box<dyn Query>)> = terms
        .iter()
        .flat_map(|term| [fields.title, fields.body].map(|field| (field, term)))
        .map(|(field, term)| {
            let query: Box<dyn Query> = Box::new(TermQuery::new(
                Term::from_field_text(field, term),
                IndexRecordOption::WithFreqs,
            ));
            (Occur::Should, query)
        })
        .collect();
    BooleanQuery::new(clauses)
}

fn stored_text(document: &TantivyDocument, field: tantivy::schema::Field) -> String {
    document
        .get_first(field)
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_owned()
}

fn snippet_text(generator: &SnippetGenerator, body: &str) -> String {
    let snippet = generator.snippet(body);
    let fragment = if snippet.fragment().is_empty() {
        let end = body
            .char_indices()
            .nth(SNIPPET_CHARS)
            .map_or(body.len(), |(offset, _)| offset);
        &body[..end]
    } else {
        snippet.fragment()
    };
    fragment.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Searches the index: at most [`MAX_RESULTS`] results of url, title and
/// snippet, sorted by score descending then url ascending. Every document
/// tied with the last admitted score is considered before the url order
/// decides, so the result does not depend on the order documents were
/// indexed in.
///
/// # Errors
/// Returns the query refusals and the error reading the index.
pub fn search(index: &WebIndex, query: &str) -> Result<Vec<ScoredResult>, SearchError> {
    let terms = query_terms(index, query)?;
    let query = build_query(index, &terms);
    let searcher = index.searcher();
    let mut limit = MAX_RESULTS;
    let hits = loop {
        let hits = searcher.search(&query, &TopDocs::with_limit(limit).order_by_score())?;
        let complete = hits.len() < limit
            || hits[MAX_RESULTS - 1].0.total_cmp(&hits[limit - 1].0) == Ordering::Greater;
        if complete {
            break hits;
        }
        limit = limit.saturating_mul(2);
    };
    let fields = index.fields();
    let mut ranked: Vec<(Score, String, TantivyDocument)> = Vec::with_capacity(hits.len());
    for (score, address) in hits {
        let document = searcher.doc::<TantivyDocument>(address)?;
        ranked.push((score, stored_text(&document, fields.url), document));
    }
    ranked.sort_by(|left, right| {
        right
            .0
            .total_cmp(&left.0)
            .then_with(|| left.1.cmp(&right.1))
    });
    ranked.truncate(MAX_RESULTS);
    let mut generator = SnippetGenerator::create(&searcher, &query, fields.body)?;
    generator.set_max_num_chars(SNIPPET_CHARS);
    Ok(ranked
        .into_iter()
        .map(|(score, url, document)| ScoredResult {
            score,
            result: SearchResult {
                url,
                title: stored_text(&document, fields.title),
                snippet: snippet_text(&generator, &stored_text(&document, fields.body)),
            },
        })
        .collect())
}

/// The search text: the compact JSON array of the results with keys url,
/// title and snippet in that order and no score.
///
/// # Errors
/// Returns the error serialising the results.
pub fn search_text(results: &[SearchResult]) -> Result<String, serde_json::Error> {
    serde_json::to_string(results)
}

/// The search canonical bytes: kind 2, the query as the payload, the media
/// type `application/json` and the compact JSON array as the text.
///
/// # Errors
/// Refuses a query longer than a uint32 length can carry and results that
/// do not serialise.
pub fn search_canonical_bytes(
    query: &str,
    results: &[SearchResult],
) -> Result<Vec<u8>, CanonicalError> {
    let text = search_text(results).map_err(|_| CanonicalError::Malformed)?;
    canonical::canonical_bytes(
        ContentKind::Search,
        query.as_bytes(),
        SEARCH_MEDIA_TYPE,
        &text,
    )
}

/// The `GET /search?q=` resource: searches the index, writes the canonical
/// bytes to the content store and answers with the results and their digest.
#[must_use]
pub fn search_route(index: &WebIndex, store: &ContentStore, request: &Request) -> Response {
    let query = match request.query_param("q") {
        Ok(Some(query)) if !query.is_empty() => query,
        Ok(_) => return Response::error(400, "missing_query"),
        Err(QueryError::Duplicate) => return Response::error(400, "duplicate_query"),
        Err(QueryError::Malformed) => return Response::error(400, "malformed_query"),
    };
    let results: Vec<SearchResult> = match search(index, &query) {
        Ok(results) => results.into_iter().map(|scored| scored.result).collect(),
        Err(error) => return Response::error(error.status(), error.code()),
    };
    let canonical = match search_canonical_bytes(&query, &results) {
        Ok(canonical) => canonical,
        Err(error) => return Response::error(500, error.code()),
    };
    let Ok(digest) = store.put(&canonical) else {
        return Response::error(500, "content_store_error");
    };
    Response::json(
        200,
        serde_json::json!({
            "query": query,
            "media_type": SEARCH_MEDIA_TYPE,
            "digest": canonical::digest_hex(&digest),
            "results": results,
        })
        .to_string()
        .into_bytes(),
    )
}

/// Registers `GET /search` behind the payment hook.
///
/// # Errors
/// Refuses a route that already has a handler.
pub fn register(
    routes: &mut RouteTable,
    hook: &Arc<dyn PaymentHook>,
    index: &Arc<WebIndex>,
    store: &Arc<ContentStore>,
) -> Result<(), RouteError> {
    let (hook, index, store) = (Arc::clone(hook), Arc::clone(index), Arc::clone(store));
    routes.set(Route::Search, move |request: &Request| {
        hook.serve(request, &|request: &Request| {
            search_route(&index, &store, request)
        })
    })
}
