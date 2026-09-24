use super::{
    metrics::Metrics,
    router::{configure_router, HttpRouter},
    settings::{MetricsSettings, ServerSettings},
    shutdown::{GracefulShutdownHandler, LocalGracefulShutdownHandler},
    span_builder::CompactRootSpanBuilder,
    HttpServerSettings,
};
use actix_web::{middleware::Condition, App, HttpServer};
use actix_web_prom::PrometheusMetrics;
use std::{net::SocketAddr, time::Duration};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing_actix_web::TracingLogger;

pub(crate) const SHUTDOWN_TIMEOUT_SEC: u64 = 10;

pub struct LaunchSettings {
    pub service_name: String,
    pub server: ServerSettings,
    pub metrics: MetricsSettings,
    pub graceful_shutdown: GracefulShutdownHandler,
}

pub async fn launch<R>(
    settings: LaunchSettings,
    http: R,
    grpc: tonic::transport::server::Router,
) -> Result<(), anyhow::Error>
where
    R: HttpRouter + Send + Sync + Clone + 'static,
{
    let metrics = settings
        .metrics
        .enabled
        .then(|| Metrics::new(&settings.service_name, &settings.metrics.route));
    let graceful_shutdown = LocalGracefulShutdownHandler::from(settings.graceful_shutdown);

    let mut futures = JoinSet::new();

    if settings.server.http.enabled {
        let http_server = http_serve(
            http,
            metrics
                .as_ref()
                .map(|metrics| metrics.http_middleware().clone()),
            &settings.server.http,
            graceful_shutdown.clone(),
        );
        graceful_shutdown
            .spawn_and_track(&mut futures, async move {
                http_server.await.map_err(anyhow::Error::msg)
            })
            .await;
    }

    if settings.server.grpc.enabled {
        let grpc_server = grpc_serve(
            grpc,
            settings.server.grpc.addr,
            graceful_shutdown.shutdown_token.clone(),
        );
        graceful_shutdown
            .spawn_and_track(&mut futures, async move {
                grpc_server.await.map_err(anyhow::Error::msg)
            })
            .await;
    }

    if let Some(metrics) = metrics {
        let addr = settings.metrics.addr;
        let graceful_shutdown_cloned = graceful_shutdown.clone();
        graceful_shutdown
            .spawn_and_track(&mut futures, async move {
                metrics.run_server(addr, graceful_shutdown_cloned).await?;
                Ok(())
            })
            .await;
    }
    let shutdown = graceful_shutdown.shutdown_token.clone();
    graceful_shutdown
        .spawn_and_track(&mut futures, async move {
            shutdown.cancelled().await;
            Ok(())
        })
        .await;

    let res = futures.join_next().await.expect("future set is not empty");
    tracing::info!("observed finished future, shutting down launcher and created tasks");
    if graceful_shutdown
        .local_cancel_wait_timeout(Some(Duration::from_secs(SHUTDOWN_TIMEOUT_SEC)))
        .await
        .is_err()
    {
        // timed out; fallback to simple task abort
        tracing::error!(
            "failed to gracefully shutdown with `CancellationToken`, aborting launcher tasks"
        );
        futures.abort_all();
    }
    futures.join_all().await;
    res?
}

pub(crate) async fn stop_actix_server_on_cancel(
    actix_handle: actix_web::dev::ServerHandle,
    shutdown: CancellationToken,
    graceful: bool,
) {
    shutdown.cancelled().await;
    tracing::info!(
        "Shutting down actix server (gracefully: {graceful}).\
        Should finish within {SHUTDOWN_TIMEOUT_SEC} seconds..."
    );
    actix_handle.stop(graceful).await;
}

pub(crate) async fn grpc_cancel_signal(shutdown: CancellationToken) {
    shutdown.cancelled().await;
    tracing::info!("Shutting down grpc server...");
}

fn http_serve<R>(
    http: R,
    metrics: Option<PrometheusMetrics>,
    settings: &HttpServerSettings,
    graceful_shutdown: LocalGracefulShutdownHandler,
) -> actix_web::dev::Server
where
    R: HttpRouter + Send + Sync + Clone + 'static,
{
    let base_path = settings.base_path.clone();
    let addr_debug = if let Some(base_path) = base_path.clone() {
        format!("{}{}", settings.addr, String::from(base_path))
    } else {
        settings.addr.to_string()
    };
    tracing::info!("starting http server on addr {}", addr_debug);

    // Initialize the tracing logger not to print http request and response messages on health endpoint
    CompactRootSpanBuilder::init_skip_http_trace_paths(["/health"]);

    let json_cfg = actix_web::web::JsonConfig::default().limit(settings.max_body_size);
    let cors_settings = settings.cors.clone();
    let cors_enabled = cors_settings.enabled;
    let server = if let Some(metrics) = metrics {
        HttpServer::new(move || {
            let cors = cors_settings.clone().build();
            App::new()
                .wrap(TracingLogger::<CompactRootSpanBuilder>::new())
                .wrap(metrics.clone())
                .wrap(Condition::new(cors_enabled, cors))
                .app_data(json_cfg.clone())
                .configure(configure_router(&http, base_path.clone().map(|p| p.into())))
        })
        .shutdown_timeout(SHUTDOWN_TIMEOUT_SEC)
        .bind(settings.addr)
        .expect("failed to bind server")
        .run()
    } else {
        HttpServer::new(move || {
            let cors = cors_settings.clone().build();
            App::new()
                .wrap(TracingLogger::<CompactRootSpanBuilder>::new())
                .wrap(Condition::new(cors_enabled, cors))
                .app_data(json_cfg.clone())
                .configure(configure_router(&http, base_path.clone().map(|p| p.into())))
        })
        .shutdown_timeout(SHUTDOWN_TIMEOUT_SEC)
        .bind(settings.addr)
        .expect("failed to bind server")
        .run()
    };
    tokio::spawn(
        graceful_shutdown
            .task_trackers
            .track_future(stop_actix_server_on_cancel(
                server.handle(),
                graceful_shutdown.shutdown_token,
                true,
            )),
    );
    server
}

async fn grpc_serve(
    grpc: tonic::transport::server::Router,
    addr: SocketAddr,
    shutdown: CancellationToken,
) -> Result<(), tonic::transport::Error> {
    tracing::info!("starting grpc server on addr {}", addr);
    grpc.serve_with_shutdown(addr, grpc_cancel_signal(shutdown))
        .await
}
