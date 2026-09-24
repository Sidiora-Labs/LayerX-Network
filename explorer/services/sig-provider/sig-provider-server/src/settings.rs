use blockscout_service_launcher::{
    launcher::{ConfigSettings, MetricsSettings, ServerSettings},
    tracing::{JaegerSettings, TracingSettings},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    #[serde(default)]
    pub server: ServerSettings,
    #[serde(default)]
    pub metrics: MetricsSettings,
    #[serde(default)]
    pub tracing: TracingSettings,
    #[serde(default)]
    pub jaeger: JaegerSettings,

    #[serde(default)]
    pub sources: SourcesSettings,
}

impl ConfigSettings for Settings {
    const SERVICE_NAME: &'static str = "SIG_PROVIDER";
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct SourcesSettings {
    pub fourbyte: url::Url,
    pub sigeth: url::Url,
    pub eth_bytecode_db: EthBytecodeDbSettings,
}

impl Default for SourcesSettings {
    fn default() -> Self {
        Self {
            fourbyte: url::Url::parse("https://www.4byte.directory/").unwrap(),
            sigeth: url::Url::parse("https://sig.eth.samczsun.com/").unwrap(),
            eth_bytecode_db: Default::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct EthBytecodeDbSettings {
    pub enabled: bool,
    pub url: url::Url,
}

impl Default for EthBytecodeDbSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            url: url::Url::parse("https://eth-bytecode-db.services.blockscout.com/").unwrap(),
        }
    }
}
