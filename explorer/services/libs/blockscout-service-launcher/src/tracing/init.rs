use super::{JaegerSettings, TracingFormat, TracingSettings};
use opentelemetry::{
    global::{self},
    sdk::{self, propagation::TraceContextPropagator},
    trace::TraceError,
};
use std::marker::Send;
use tracing::Metadata;
use tracing_subscriber::{
    filter::LevelFilter, fmt::format::FmtSpan, layer::SubscriberExt, prelude::*, Layer,
};

pub fn init_logs(
    service_name: &str,
    tracing_settings: &TracingSettings,
    jaeger_settings: &JaegerSettings,
) -> Result<(), anyhow::Error> {
    init_logs_with_filter(
        service_name,
        tracing_settings,
        jaeger_settings,
        tracing_subscriber::filter::filter_fn(move |_| true),
    )
}

pub fn init_logs_with_filter<F: Fn(&Metadata) -> bool + Send + Sync + 'static>(
    service_name: &str,
    tracing_settings: &TracingSettings,
    jaeger_settings: &JaegerSettings,
    filter: tracing_subscriber::filter::FilterFn<F>,
) -> Result<(), anyhow::Error> {
    // If tracing is disabled, there is nothing to initialize
    if !tracing_settings.enabled {
        return Ok(());
    }

    let mut layers: Vec<_> = vec![];

    #[cfg(feature = "actix-request-id")]
    {
        if let TracingFormat::Json = tracing_settings.format {
            layers.push(super::request_id_layer::layer().boxed());
        }
    }

    let stdout_layer: Box<dyn Layer<_> + Sync + Send + 'static> = match tracing_settings.format {
        TracingFormat::Default => tracing_subscriber::fmt::layer()
            .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
            .with_filter(
                tracing_subscriber::EnvFilter::builder()
                    .with_default_directive(LevelFilter::INFO.into())
                    .from_env_lossy(),
            )
            .with_filter(filter)
            .boxed(),
        TracingFormat::Json => tracing_subscriber::fmt::layer()
            .json()
            // .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
            .flatten_event(true)
            .with_current_span(true)
            .with_span_list(false)
            .with_filter(
                tracing_subscriber::EnvFilter::builder()
                    .with_default_directive(LevelFilter::INFO.into())
                    .from_env_lossy(),
            )
            .with_filter(filter)
            .boxed(),
    };
    layers.push(stdout_layer);

    if jaeger_settings.enabled {
        let tracer = init_jaeger_tracer(service_name, &jaeger_settings.agent_endpoint)?;
        // output traces to jaeger with default log level (default is DEBUG)
        let jaeger_layer = tracing_opentelemetry::layer()
            .with_tracer(tracer)
            .with_filter(LevelFilter::DEBUG)
            .boxed();
        layers.push(jaeger_layer)
    }

    let registry = tracing_subscriber::registry().with(layers);
    registry.try_init()?;

    Ok(())
}

pub fn init_jaeger_tracer(
    service_name: &str,
    endpoint: &str,
) -> Result<sdk::trace::Tracer, TraceError> {
    global::set_text_map_propagator(TraceContextPropagator::new());

    opentelemetry_jaeger::new_agent_pipeline()
        .with_service_name(service_name)
        .with_endpoint(endpoint)
        .with_auto_split_batch(true)
        .install_batch(opentelemetry::runtime::Tokio)
}
