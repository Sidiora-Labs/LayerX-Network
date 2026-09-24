use blockscout_service_launcher::launcher::ConfigSettings;
use sig_provider_server::{sig_provider, Settings};

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let settings = Settings::build().expect("failed to read config");
    sig_provider(settings).await
}
