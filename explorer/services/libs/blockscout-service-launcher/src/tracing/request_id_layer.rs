use std::{fmt::Debug, str::FromStr};
use tracing::{field::Field, span::Attributes, Id, Subscriber};
use tracing_subscriber::{
    fmt::{format::JsonFields, FormatFields, FormattedFields},
    layer::Context,
    registry::LookupSpan,
    Layer,
};
use uuid::Uuid;

#[derive(Clone, Copy, Debug)]
struct RequestId(Uuid);

pub fn layer() -> RequestIdStorage {
    RequestIdStorage
}

pub struct RequestIdStorage;

impl<S: Subscriber + for<'lookup> LookupSpan<'lookup>> Layer<S> for RequestIdStorage {
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let span = ctx.span(id).expect("Span not found, this is a bug");

        let request_id = if let Some(parent_span) = span.parent() {
            let request_id = parent_span.extensions().get::<RequestId>().cloned();
            if let Some(request_id) = request_id {
                let mut extensions = span.extensions_mut();
                if extensions
                    .get_mut::<FormattedFields<JsonFields>>()
                    .is_none()
                {
                    let mut fields = FormattedFields::<JsonFields>::new(String::new());
                    if JsonFields::new()
                        .format_fields(fields.as_writer(), attrs)
                        .is_ok()
                    {
                        extensions.insert(fields);
                    } else {
                        eprintln!(
                            "[tracing-subscriber] Unable to format the following event, ignoring: {attrs:?}"
                        );
                    }
                }
                let data = extensions.get_mut::<FormattedFields<JsonFields>>().unwrap();
                match serde_json::from_str::<serde_json::Value>(data) {
                    Ok(serde_json::Value::Object(mut fields))
                        if !fields.contains_key("request_id") =>
                    {
                        fields.insert("request_id".into(), request_id.0.to_string().into());
                        data.fields = serde_json::Value::Object(fields).to_string();
                    }
                    // If the value is not found or has invalid type, just ignore the error
                    // and propagate it further. Default Format<Json> layer will handle those cases.
                    _ => {}
                };
            };
            request_id
        } else if let Some(field) = attrs.fields().field("request_id") {
            struct Visitor {
                request_id_field: Field,
                request_id_value: Option<RequestId>,
            }
            impl tracing::field::Visit for Visitor {
                fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
                    if field == &self.request_id_field {
                        if let Ok(request_id) = Uuid::from_str(&format!("{value:?}")) {
                            self.request_id_value = Some(RequestId(request_id));
                        }
                    }
                }
            }

            let mut visitor = Visitor {
                request_id_field: field,
                request_id_value: None,
            };
            attrs.record(&mut visitor);

            visitor.request_id_value
        } else {
            None
        };

        if let Some(request_id) = request_id {
            span.extensions_mut().insert(request_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io,
        sync::{Arc, Mutex, MutexGuard, TryLockError},
    };

    use pretty_assertions::assert_eq;
    use regex::Regex;
    use tracing::subscriber::with_default;
    use tracing_subscriber::{fmt::format::FmtSpan, layer::SubscriberExt, Layer};

    // https://github.com/tokio-rs/tracing/blob/527b4f66a604e7a6baa6aa7536428e3a303ba3c8/tracing-subscriber/src/fmt/format/json.rs#L522
    struct MockTime;

    impl tracing_subscriber::fmt::time::FormatTime for MockTime {
        fn format_time(
            &self,
            w: &mut tracing_subscriber::fmt::format::Writer<'_>,
        ) -> std::fmt::Result {
            write!(w, "fake time")
        }
    }

    pub(crate) struct MockWriter {
        buf: Arc<Mutex<Vec<u8>>>,
    }

    // https://github.com/tokio-rs/tracing/blob/527b4f66a604e7a6baa6aa7536428e3a303ba3c8/tracing-subscriber/src/fmt/mod.rs#L1249
    impl MockWriter {
        pub(crate) fn new(buf: Arc<Mutex<Vec<u8>>>) -> Self {
            Self { buf }
        }

        pub(crate) fn map_error<Guard>(err: TryLockError<Guard>) -> io::Error {
            match err {
                TryLockError::WouldBlock => io::Error::from(io::ErrorKind::WouldBlock),
                TryLockError::Poisoned(_) => io::Error::from(io::ErrorKind::Other),
            }
        }

        pub(crate) fn buf(&self) -> io::Result<MutexGuard<'_, Vec<u8>>> {
            self.buf.try_lock().map_err(Self::map_error)
        }
    }

    impl io::Write for MockWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.buf()?.write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.buf()?.flush()
        }
    }

    // https://github.com/tokio-rs/tracing/blob/527b4f66a604e7a6baa6aa7536428e3a303ba3c8/tracing-subscriber/src/fmt/mod.rs#L1281
    #[derive(Clone, Default)]
    pub(crate) struct MockMakeWriter {
        buf: Arc<Mutex<Vec<u8>>>,
    }

    impl MockMakeWriter {
        #[allow(unused)]
        pub(crate) fn new(buf: Arc<Mutex<Vec<u8>>>) -> Self {
            Self { buf }
        }

        #[allow(unused)]
        pub(crate) fn buf(&self) -> MutexGuard<'_, Vec<u8>> {
            self.buf.lock().unwrap()
        }

        pub(crate) fn get_string(&self) -> String {
            let mut buf = self.buf.lock().expect("lock shouldn't be poisoned");
            let string = std::str::from_utf8(&buf[..])
                .expect("formatter should not have produced invalid utf-8")
                .to_owned();
            buf.clear();
            string
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for MockMakeWriter {
        type Writer = MockWriter;

        fn make_writer(&'a self) -> Self::Writer {
            MockWriter::new(self.buf.clone())
        }
    }

    // https://github.com/tokio-rs/tracing/blob/527b4f66a604e7a6baa6aa7536428e3a303ba3c8/tracing-subscriber/src/fmt/fmt_subscriber.rs#L1304
    fn sanitize_timings(s: String) -> String {
        let re = Regex::new("time\\.(idle|busy)=([0-9.]+)[mµn]s").unwrap();
        re.replace_all(s.as_str(), "timing").to_string()
    }

    fn parse_json(s: &str) -> serde_json::Value {
        serde_json::from_str::<serde_json::Value>(s)
            .unwrap_or_else(|_| panic!("failed to parse '{s}'"))
    }

    fn parse_captured_logs(logs: String) -> Vec<serde_json::Value> {
        logs.split('\n')
            .filter(|l| !l.is_empty())
            .map(parse_json)
            .collect()
    }

    #[test]
    fn request_id_layer_preserves_fields() {
        let request_id_layer = super::layer().boxed();
        let make_writer = MockMakeWriter::default();
        let json_layer = tracing_subscriber::fmt::layer()
            .json()
            .flatten_event(true)
            .with_current_span(true)
            .with_span_list(true)
            .with_writer(make_writer.clone())
            .with_level(false)
            .with_ansi(false)
            .with_timer(MockTime)
            .with_span_events(FmtSpan::ENTER)
            .boxed();

        let layers = vec![request_id_layer, json_layer];

        let registry = tracing_subscriber::registry().with(layers);

        with_default(registry, || {
            let span0_with_request_id = tracing::info_span!(
                "span0_with_request_id",
                request_id = %"02f09a3f-1624-3b1d-8409-44eff7708208"
            );
            let _e0 = span0_with_request_id.enter();
            let span1 = tracing::info_span!("span1", x = 42);
            let _e = span1.enter();
        });
        let actual = sanitize_timings(make_writer.get_string());
        assert_eq!(
            vec![
                parse_json(
                    r#"{
                    "timestamp":"fake time",
                    "message":"enter",
                    "target":"blockscout_service_launcher::tracing::request_id_layer::tests",
                    "span":{
                        "request_id":"02f09a3f-1624-3b1d-8409-44eff7708208",
                        "name":"span0_with_request_id"
                    },
                    "spans":[
                        {
                            "request_id":"02f09a3f-1624-3b1d-8409-44eff7708208",
                            "name":"span0_with_request_id"
                        }
                    ]
                }"#
                ),
                parse_json(
                    r#"{
                    "timestamp":"fake time",
                    "message":"enter",
                    "target":"blockscout_service_launcher::tracing::request_id_layer::tests",
                    "span":{
                        "request_id":"02f09a3f-1624-3b1d-8409-44eff7708208",
                        "x":42,
                        "name":"span1"
                    },
                    "spans":[
                        {
                            "request_id":"02f09a3f-1624-3b1d-8409-44eff7708208",
                            "name":"span0_with_request_id"
                        },
                        {
                            "request_id":"02f09a3f-1624-3b1d-8409-44eff7708208",
                            "x":42,
                            "name":"span1"
                        }
                    ]
                }"#
                )
            ],
            parse_captured_logs(actual)
        );
    }
}
