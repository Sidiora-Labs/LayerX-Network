use sig_provider_proto::blockscout::sig_provider::v1::{
    health_check_response, health_server::Health, HealthCheckRequest, HealthCheckResponse,
};

#[derive(Default)]
pub struct HealthService {}

#[async_trait::async_trait]
impl Health for HealthService {
    async fn check(
        &self,
        _request: tonic::Request<HealthCheckRequest>,
    ) -> Result<tonic::Response<HealthCheckResponse>, tonic::Status> {
        Ok(tonic::Response::new(HealthCheckResponse {
            status: health_check_response::ServingStatus::Serving as i32,
        }))
    }
}
