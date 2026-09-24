use actix_cors::Cors;
use config::{Config, File};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, str::FromStr};

pub trait ConfigSettings {
    const SERVICE_NAME: &'static str;

    fn build() -> anyhow::Result<Self>
    where
        Self: Deserialize<'static>,
    {
        let config_path_name: &String = &format!("{}__CONFIG", Self::SERVICE_NAME);
        let config_path = std::env::var(config_path_name);

        let mut builder = Config::builder();
        if let Ok(config_path) = config_path {
            builder = builder.add_source(File::with_name(&config_path));
            std::env::remove_var(config_path_name);
        };
        // Use `__` so that it would be possible to address keys with underscores in names (e.g. `access_key`)
        builder = builder
            .add_source(config::Environment::with_prefix(Self::SERVICE_NAME).separator("__"));

        let settings: Self = builder.build()?.try_deserialize()?;

        settings.validate()?;

        Ok(settings)
    }

    fn validate(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

/// HTTP and GRPC server settings.
/// Notice that, by default, HTTP server is enabled, and GRPC is disabled.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct ServerSettings {
    pub http: HttpServerSettings,
    pub grpc: GrpcServerSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HttpServerSettings {
    pub enabled: bool,
    pub addr: SocketAddr,
    pub max_body_size: usize,
    pub cors: CorsSettings,
    pub base_path: Option<BasePath>,
}

impl Default for HttpServerSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            addr: SocketAddr::from_str("0.0.0.0:8050").unwrap(),
            max_body_size: 2 * 1024 * 1024, // 2 Mb - default Actix value
            cors: Default::default(),
            base_path: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CorsSettings {
    pub enabled: bool,
    pub allowed_origin: String,
    pub allowed_methods: String,
    pub allowed_credentials: bool,
    pub max_age: usize,
    pub block_on_origin_mismatch: bool,
    pub send_wildcard: bool,
}

impl Default for CorsSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            allowed_origin: "".to_string(),
            allowed_methods: "PUT, GET, POST, OPTIONS, DELETE, PATCH".to_string(),
            allowed_credentials: true,
            max_age: 3600,
            block_on_origin_mismatch: false,
            send_wildcard: false,
        }
    }
}

impl CorsSettings {
    pub fn build(self) -> Cors {
        if !self.enabled {
            return Cors::default();
        }
        let mut cors = Cors::default()
            .allow_any_header()
            .allowed_methods(split_string(&self.allowed_methods))
            .max_age(Some(self.max_age))
            .block_on_origin_mismatch(self.block_on_origin_mismatch);
        if self.allowed_credentials {
            cors = cors.supports_credentials()
        }
        if self.send_wildcard {
            cors = cors.send_wildcard()
        }
        match self.allowed_origin.as_str() {
            "*" => cors = cors.allow_any_origin(),
            allowed_origin => {
                for origin in split_string(allowed_origin) {
                    cors = cors.allowed_origin(origin)
                }
            }
        };
        cors
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct BasePath(String);

impl TryFrom<String> for BasePath {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if !value.starts_with("/") {
            return Err(format!("Invalid base path '{value}': must start with '/'",));
        };
        if value.ends_with("/") {
            return Err(format!(
                "Invalid base path '{value}': must not end with '/'",
            ));
        };
        Ok(Self(value))
    }
}

impl From<BasePath> for String {
    fn from(value: BasePath) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GrpcServerSettings {
    pub enabled: bool,
    pub addr: SocketAddr,
}

impl Default for GrpcServerSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            addr: SocketAddr::from_str("0.0.0.0:8051").unwrap(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MetricsSettings {
    pub enabled: bool,
    pub addr: SocketAddr,
    pub route: String,
}

impl Default for MetricsSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            addr: SocketAddr::from_str("0.0.0.0:6060").expect("should be valid url"),
            route: "/metrics".to_string(),
        }
    }
}

fn split_string(s: &str) -> Vec<&str> {
    s.split(',').map(|s| s.trim()).collect()
}
