use std::time::Duration;

pub const LXP_WIRE_PROTOCOL_VERSION: u16 = layerx_wire::limits::PROTOCOL_VERSION;
pub const TESTNET_NETWORK_ID: u32 = 402;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingRelease {
    pub package_semver: String,
    pub wire_protocol_version: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestnetConfig {
    pub package_semver: String,
    pub wire_protocol_version: u16,
    pub network_id: u32,
    pub public_endpoint: String,
    pub gateway_endpoint: String,
    pub faucet_endpoint: String,
    pub status_endpoint: String,
    pub reset_schedule: String,
    pub snapshot_interval: Duration,
}

impl TestnetConfig {
    pub fn validate(&self, pending: &PendingRelease) -> Result<(), &'static str> {
        if self.package_semver != pending.package_semver {
            return Err("testnet package release does not match pending release");
        }
        if self.wire_protocol_version != pending.wire_protocol_version
            || self.wire_protocol_version != LXP_WIRE_PROTOCOL_VERSION
        {
            return Err("testnet LXP wire protocol does not match pending release");
        }
        if self.network_id != TESTNET_NETWORK_ID
            || !canonical_https_origin(&self.public_endpoint)
            || !canonical_https_origin(&self.gateway_endpoint)
            || !canonical_https_origin(&self.faucet_endpoint)
            || !canonical_https_origin(&self.status_endpoint)
            || self.reset_schedule.is_empty()
            || self.snapshot_interval.is_zero()
        {
            return Err("invalid testnet operations configuration");
        }
        Ok(())
    }
}

fn canonical_https_origin(value: &str) -> bool {
    let Some(authority) = value.strip_prefix("https://") else {
        return false;
    };
    !authority.is_empty()
        && !authority.ends_with('/')
        && !authority.bytes().any(|byte| {
            byte.is_ascii_control()
                || byte.is_ascii_whitespace()
                || matches!(byte, b'/' | b'?' | b'#' | b'\\' | b'@')
        })
}

#[must_use]
pub fn platform_testnet() -> TestnetConfig {
    TestnetConfig {
        package_semver: env!("CARGO_PKG_VERSION").to_owned(),
        wire_protocol_version: LXP_WIRE_PROTOCOL_VERSION,
        network_id: TESTNET_NETWORK_ID,
        public_endpoint: "https://testnet.layerx.network".to_owned(),
        gateway_endpoint: "https://api.testnet.layerx.network".to_owned(),
        faucet_endpoint: "https://faucet.testnet.layerx.network".to_owned(),
        status_endpoint: "https://status.layerx.network".to_owned(),
        reset_schedule: "09:00 UTC on the first Tuesday of every month".to_owned(),
        snapshot_interval: Duration::from_secs(15),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_and_wire_versions_are_independent_release_gates() {
        let config = platform_testnet();
        let pending = PendingRelease {
            package_semver: env!("CARGO_PKG_VERSION").to_owned(),
            wire_protocol_version: LXP_WIRE_PROTOCOL_VERSION,
        };
        assert_eq!(config.validate(&pending), Ok(()));
        assert!(config
            .validate(&PendingRelease {
                package_semver: "0.1.1".to_owned(),
                wire_protocol_version: LXP_WIRE_PROTOCOL_VERSION,
            })
            .is_err());
        assert!(config
            .validate(&PendingRelease {
                package_semver: pending.package_semver,
                wire_protocol_version: LXP_WIRE_PROTOCOL_VERSION + 1,
            })
            .is_err());
    }
}
