use crate::quote::{Address, SIDIORA};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use std::io::Read;
use std::path::Path;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigError {
    pub field: &'static str,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "configuration refused: {}", self.field)
    }
}
impl std::error::Error for ConfigError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StationConfig {
    pub chain_id: u64,
    pub endpoints: Vec<String>,
    pub paymaster: Address,
    pub token: Address,
    pub decimals: u8,
    pub sid_denom: String,
    pub pax_denom: String,
    pub max_rate_age: u64,
    pub spread_bps: u16,
    pub margin_bps: u16,
    pub per_account_limit: u128,
    pub per_interval_limit: u128,
    pub per_quote_limit: u128,
    pub interval_seconds: u64,
    pub balance_floor: u128,
    pub relayer_key_env: String,
}

fn field<T: DeserializeOwned>(
    map: &mut Map<String, Value>,
    name: &'static str,
) -> Result<T, ConfigError> {
    serde_json::from_value(map.remove(name).ok_or(ConfigError { field: name })?)
        .map_err(|_| ConfigError { field: name })
}

fn address(map: &mut Map<String, Value>, name: &'static str) -> Result<Address, ConfigError> {
    let raw: String = field(map, name)?;
    let raw = raw.strip_prefix("0x").ok_or(ConfigError { field: name })?;
    if raw.len() != 40 || !raw.is_ascii() {
        return Err(ConfigError { field: name });
    }
    let mut result = [0; 20];
    for (index, byte) in result.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&raw[index * 2..index * 2 + 2], 16)
            .map_err(|_| ConfigError { field: name })?;
    }
    Ok(result)
}

impl StationConfig {
    /// # Errors
    /// Returns the field that is missing, malformed or inconsistent.
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        let mut map: Map<String, Value> =
            serde_json::from_str(text).map_err(|_| ConfigError { field: "config" })?;
        let config = Self {
            chain_id: field(&mut map, "chain_id")?,
            endpoints: field(&mut map, "endpoints")?,
            paymaster: address(&mut map, "paymaster")?,
            token: address(&mut map, "token")?,
            decimals: field(&mut map, "decimals")?,
            sid_denom: field(&mut map, "sid_denom")?,
            pax_denom: field(&mut map, "pax_denom")?,
            max_rate_age: field(&mut map, "max_rate_age")?,
            spread_bps: field(&mut map, "spread_bps")?,
            margin_bps: field(&mut map, "margin_bps")?,
            per_account_limit: field(&mut map, "per_account_limit")?,
            per_interval_limit: field(&mut map, "per_interval_limit")?,
            per_quote_limit: field(&mut map, "per_quote_limit")?,
            interval_seconds: field(&mut map, "interval_seconds")?,
            balance_floor: field(&mut map, "balance_floor")?,
            relayer_key_env: field(&mut map, "relayer_key_env")?,
        };
        if !map.is_empty() {
            return Err(ConfigError {
                field: "unknown_field",
            });
        }
        config.validate()?;
        Ok(config)
    }

    /// # Errors
    /// Refuses unreadable, oversized or invalid configuration files.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let file = std::fs::File::open(path).map_err(|_| ConfigError {
            field: "config_path",
        })?;
        let mut text = String::new();
        file.take(1_048_577)
            .read_to_string(&mut text)
            .map_err(|_| ConfigError {
                field: "config_path",
            })?;
        if text.len() > 1_048_576 {
            return Err(ConfigError {
                field: "config_size",
            });
        }
        Self::parse(&text)
    }

    /// # Errors
    /// Names the first field incompatible with the contract or station policy.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let checks = [
            ("chain_id", self.chain_id > 0),
            (
                "endpoints",
                !self.endpoints.is_empty()
                    && self.endpoints.iter().all(|url| {
                        url.strip_prefix("https://").is_some_and(|tail| {
                            !tail.is_empty()
                                && !tail.starts_with('/')
                                && !tail.contains('@')
                                && !tail.contains('#')
                                && !tail.contains('?')
                                && !tail.chars().any(char::is_whitespace)
                        })
                    }),
            ),
            (
                "paymaster",
                self.paymaster != [0; 20] && self.paymaster != SIDIORA,
            ),
            ("token", self.token == SIDIORA),
            ("decimals", self.decimals == 6),
            ("sid_denom", self.sid_denom == "usid"),
            ("pax_denom", self.pax_denom == "uhpx"),
            (
                "max_rate_age",
                self.max_rate_age > 0 && self.max_rate_age <= 300,
            ),
            ("spread_bps", self.spread_bps <= 500),
            ("margin_bps", self.margin_bps <= self.spread_bps),
            ("per_quote_limit", self.per_quote_limit > 0),
            (
                "per_account_limit",
                self.per_account_limit >= self.per_quote_limit,
            ),
            (
                "per_interval_limit",
                self.per_interval_limit >= self.per_account_limit,
            ),
            ("interval_seconds", self.interval_seconds > 0),
            ("balance_floor", self.balance_floor > 0),
            (
                "relayer_key_env",
                !self.relayer_key_env.is_empty()
                    && self.relayer_key_env.bytes().enumerate().all(|(i, b)| {
                        b == b'_' || b.is_ascii_uppercase() || (i > 0 && b.is_ascii_digit())
                    }),
            ),
        ];
        for (field, valid) in checks {
            if !valid {
                return Err(ConfigError { field });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn config() -> StationConfig {
        StationConfig {
            chain_id: 1325,
            endpoints: vec!["https://paxeer.app".into()],
            paymaster: [0x44; 20],
            token: SIDIORA,
            decimals: 6,
            sid_denom: "usid".into(),
            pax_denom: "uhpx".into(),
            max_rate_age: 300,
            spread_bps: 500,
            margin_bps: 100,
            per_account_limit: 4_000_000,
            per_interval_limit: 8_000_000,
            per_quote_limit: 3_000_000,
            interval_seconds: 60,
            balance_floor: 100,
            relayer_key_env: "PAXEER_RELAYER_KEY".into(),
        }
    }

    fn json() -> Value {
        serde_json::json!({"chain_id":1325,"endpoints":["https://paxeer.app"],
            "paymaster":"0x4444444444444444444444444444444444444444",
            "token":"0x21f7b20a555199fa73A238B1a91FD0f549068fEe","decimals":6,
            "sid_denom":"usid","pax_denom":"uhpx","max_rate_age":300,"spread_bps":500,
            "margin_bps":100,"per_account_limit":4_000_000,"per_interval_limit":8_000_000,
            "per_quote_limit":3_000_000,"interval_seconds":60,"balance_floor":100,
            "relayer_key_env":"PAXEER_RELAYER_KEY"})
    }

    #[test]
    fn valid_config_and_every_missing_field() {
        let value = json();
        assert_eq!(StationConfig::parse(&value.to_string()), Ok(config()));
        let Value::Object(fields) = value else {
            panic!("object required")
        };
        for name in fields.keys() {
            let mut incomplete = fields.clone();
            incomplete.remove(name);
            let result = StationConfig::parse(&Value::Object(incomplete).to_string());
            assert_eq!(result.err().map(|e| e.field.to_owned()), Some(name.clone()));
        }
    }

    #[test]
    fn contradictory_fields_and_unknown_secrets_refused_without_echo() {
        for (name, value) in [
            ("chain_id", serde_json::json!(0)),
            ("endpoints", serde_json::json!([])),
            (
                "paymaster",
                serde_json::json!("0x0000000000000000000000000000000000000000"),
            ),
            ("token", serde_json::json!("invalid")),
            ("decimals", serde_json::json!(18)),
            ("sid_denom", serde_json::json!("sid")),
            ("pax_denom", serde_json::json!("pax")),
            ("max_rate_age", serde_json::json!(301)),
            ("spread_bps", serde_json::json!(501)),
            ("margin_bps", serde_json::json!(501)),
            ("per_quote_limit", serde_json::json!(0)),
            ("per_account_limit", serde_json::json!(1)),
            ("per_interval_limit", serde_json::json!(1)),
            ("interval_seconds", serde_json::json!(0)),
            ("balance_floor", serde_json::json!(0)),
            ("relayer_key_env", serde_json::json!("sensitive-value")),
        ] {
            let mut value_map = json();
            value_map[name] = value;
            assert_eq!(
                StationConfig::parse(&value_map.to_string()).err(),
                Some(ConfigError { field: name })
            );
        }
        let mut value = json();
        value["private_key"] = Value::String("sensitive-value".into());
        assert_eq!(
            StationConfig::parse(&value.to_string()).err(),
            Some(ConfigError {
                field: "unknown_field"
            })
        );
    }

    #[test]
    fn load_file_and_refuse_missing_path() -> Result<(), Box<dyn std::error::Error>> {
        let path = std::env::temp_dir().join(format!("gas-station-config-{}", std::process::id()));
        std::fs::write(&path, json().to_string())?;
        let result = StationConfig::load(&path);
        std::fs::remove_file(&path)?;
        assert_eq!(result, Ok(config()));
        assert_eq!(
            StationConfig::load(&path).err(),
            Some(ConfigError {
                field: "config_path"
            })
        );
        Ok(())
    }
}
