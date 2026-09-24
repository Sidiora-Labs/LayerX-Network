use actix_web::{
    body::MessageBody,
    dev::{ServiceRequest, ServiceResponse},
    http::StatusCode,
    Error, HttpMessage, ResponseError,
};
use once_cell::sync::Lazy;
use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, RwLock},
    time::Instant,
};
use tracing::{Id, Span};
use tracing_actix_web::root_span;

static REQUEST_TIMINGS: Lazy<Mutex<HashMap<Id, Instant>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

static SKIP_HTTP_TRACE_PATHS: Lazy<RwLock<HashSet<String>>> =
    Lazy::new(|| RwLock::new(HashSet::new()));

#[derive(Default)]
pub struct CompactRootSpanBuilder;

impl CompactRootSpanBuilder {
    pub fn init_skip_http_trace_paths<S: Into<String>>(
        skip_http_trace_paths: impl IntoIterator<Item = S>,
    ) {
        *SKIP_HTTP_TRACE_PATHS.write().unwrap() = skip_http_trace_paths
            .into_iter()
            .map(|path| path.into())
            .collect();
    }
}

impl tracing_actix_web::RootSpanBuilder for CompactRootSpanBuilder {
    fn on_request_start(request: &ServiceRequest) -> Span {
        if SKIP_HTTP_TRACE_PATHS
            .read()
            .unwrap()
            .contains(request.path())
        {
            request
                .extensions_mut()
                .insert(tracing_actix_web::SkipHttpTrace);
        }
        let span = root_span!(
            request,
            duration = tracing::field::Empty,
            unit = tracing::field::Empty
        );
        // Will be none if tracing subscriber is not initialized
        if let Some(span_id) = span.id() {
            REQUEST_TIMINGS
                .lock()
                .unwrap()
                .insert(span_id, Instant::now());
        }
        span
    }

    fn on_request_end<B: MessageBody>(span: Span, outcome: &Result<ServiceResponse<B>, Error>) {
        // Will be none if tracing subscriber is not initialized
        if let Some(span_id) = span.id() {
            let start = REQUEST_TIMINGS.lock().unwrap().remove(&span_id);
            if let Some(start) = start {
                let duration = Instant::now() - start;
                span.record("duration", duration.as_micros());
                span.record("unit", "microsecond");
            }
        }

        match &outcome {
            Ok(response) => {
                if let Some(error) = response.response().error() {
                    // use the status code already constructed for the outgoing HTTP response
                    handle_error(span, response.status(), error.as_response_error());
                } else {
                    let code: i32 = response.response().status().as_u16().into();
                    span.record("http.status_code", code);
                }
            }
            Err(error) => {
                let response_error = error.as_response_error();
                handle_error(span, response_error.status_code(), response_error);
            }
        };
    }
}

fn handle_error(span: Span, status_code: StatusCode, response_error: &dyn ResponseError) {
    // pre-formatting errors is a workaround for https://github.com/tokio-rs/tracing/issues/1565
    let display = format!("{response_error}");
    let debug = format!("{response_error:?}");
    span.record("exception.message", tracing::field::display(display));
    span.record("exception.details", tracing::field::display(debug));
    let code: i32 = status_code.as_u16().into();

    span.record("http.status_code", code);
}
