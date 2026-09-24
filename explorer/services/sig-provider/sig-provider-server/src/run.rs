use crate::{health::HealthService, settings::SourcesSettings, Service, Settings};
use blockscout_service_launcher::{launcher, launcher::LaunchSettings, tracing};
use sig_provider::{
    eth_bytecode_db, fourbyte, sigeth, CompleteSignatureSource, SignatureSource, SourceAggregator,
};
use sig_provider_proto::blockscout::sig_provider::v1::{
    abi_service_actix::route_abi_service,
    abi_service_server::{AbiService, AbiServiceServer},
    health_actix::route_health,
    health_server::HealthServer,
    signature_service_actix::route_signature_service,
    signature_service_server::{SignatureService, SignatureServiceServer},
};
use std::sync::Arc;

const SERVICE_NAME: &str = "sig_provider";

#[derive(Clone)]
struct Router<S: SignatureService, A: AbiService> {
    signature: Arc<S>,
    abi: Arc<A>,
    health: Arc<HealthService>,
}

impl<S: SignatureService, A: AbiService> Router<S, A> {
    pub fn grpc_router(&self) -> tonic::transport::server::Router {
        tonic::transport::Server::builder()
            .add_service(HealthServer::from_arc(self.health.clone()))
            .add_service(SignatureServiceServer::from_arc(self.signature.clone()))
            .add_service(AbiServiceServer::from_arc(self.abi.clone()))
    }
}

impl<S: SignatureService, A: AbiService> launcher::HttpRouter for Router<S, A> {
    fn register_routes(&self, service_config: &mut actix_web::web::ServiceConfig) {
        service_config
            .configure(|config| route_health(config, self.health.clone()))
            .configure(|config| route_signature_service(config, self.signature.clone()))
            .configure(|config| route_abi_service(config, self.abi.clone()));
    }
}

pub fn new_service(settings: SourcesSettings) -> Arc<Service> {
    let sources: Vec<Arc<dyn SignatureSource + Send + Sync + 'static>> = vec![
        Arc::new(sigeth::Source::new(settings.sigeth)),
        Arc::new(fourbyte::Source::new(settings.fourbyte)),
    ];
    let complete_sources = {
        let mut sources: Vec<Arc<dyn CompleteSignatureSource + Send + Sync + 'static>> = vec![];
        if settings.eth_bytecode_db.enabled {
            sources.push(Arc::new(eth_bytecode_db::Source::new(
                settings.eth_bytecode_db.url,
            )))
        };
        sources
    };
    let aggregator = Arc::new(SourceAggregator::new(sources, complete_sources));
    Arc::new(Service::new(aggregator))
}

pub async fn sig_provider(settings: Settings) -> Result<(), anyhow::Error> {
    tracing::init_logs(SERVICE_NAME, &settings.tracing, &settings.jaeger)?;

    let health = Arc::new(HealthService::default());
    let service = new_service(settings.sources);

    let router = Router {
        abi: service.clone(),
        signature: service.clone(),
        health,
    };

    let grpc_router = router.grpc_router();
    let http_router = router;

    let launch_settings = LaunchSettings {
        service_name: SERVICE_NAME.to_string(),
        server: settings.server,
        metrics: settings.metrics,
    };

    launcher::launch(&launch_settings, http_router, grpc_router).await
}
