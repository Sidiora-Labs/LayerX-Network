use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TracingFormat {
    Default,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TracingSettings {
    /// If disabled, tracing is not initialized for neither
    /// stdout, nor jaeger (enabled by default).
    pub enabled: bool,
    pub format: TracingFormat,
}

impl Default for TracingSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            format: TracingFormat::Default,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct JaegerSettings {
    pub enabled: bool,
    pub agent_endpoint: String,
}

impl Default for JaegerSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            agent_endpoint: "127.0.0.1:6831".to_string(),
        }
    }
}
