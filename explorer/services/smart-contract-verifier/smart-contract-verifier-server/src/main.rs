use blockscout_service_launcher::launcher::ConfigSettings;
use smart_contract_verifier_server::Settings;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let settings = Settings::build().expect("failed to read config");
    smart_contract_verifier_server::run(settings).await
}
