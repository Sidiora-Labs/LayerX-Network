//! Closed account-namespace vocabulary.

use crate::limits::{MAX_ACCOUNT_NAME_BYTES, MAX_DID_BYTES};

/// The only account namespaces admitted by the interaction-layer contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountNamespace {
    /// `agent:<did>:main`.
    AgentMain,
    /// `agent:<did>:asset:<lowercase hex64>`.
    AgentAsset,
    /// `agent:<did>:budget:<id>`.
    AgentBudget,
    /// `agent:<did>:escrow:<id>`.
    AgentEscrow,
    /// `agent:<did>:margin:<position>`.
    AgentMargin,
    /// `system:liquidity:<market>`.
    SystemLiquidity,
    /// `system:insurance`.
    SystemInsurance,
    /// `system:fees`.
    SystemFees,
    /// `system:paxeer-reserve`.
    SystemPaxeerReserve,
    /// `system:paxeer-withdrawals`.
    SystemPaxeerWithdrawals,
}

/// Account construction failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountError {
    /// The canonical account name was empty or exceeded 512 bytes.
    Length,
    /// The name did not match one of the closed namespaces.
    UnknownNamespace,
    /// A required DID, identifier, position, or market was empty.
    EmptyComponent,
}

/// A validated canonical account name and its closed namespace.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountId {
    namespace: AccountNamespace,
    canonical: String,
}

impl AccountId {
    /// Parses and validates one canonical account name without normalisation.
    ///
    /// # Errors
    ///
    /// Returns [`AccountError`] when a bound, component, or namespace fails.
    pub fn parse(canonical: &str) -> Result<Self, AccountError> {
        if canonical.is_empty() || canonical.len() > MAX_ACCOUNT_NAME_BYTES {
            return Err(AccountError::Length);
        }
        let namespace = if let Some(agent) = canonical.strip_prefix("agent:") {
            parse_agent(agent)?
        } else if canonical == "system:insurance" {
            AccountNamespace::SystemInsurance
        } else if canonical == "system:fees" {
            AccountNamespace::SystemFees
        } else if canonical == "system:paxeer-reserve" {
            AccountNamespace::SystemPaxeerReserve
        } else if canonical == "system:paxeer-withdrawals" {
            AccountNamespace::SystemPaxeerWithdrawals
        } else if let Some(market) = canonical.strip_prefix("system:liquidity:") {
            if market.is_empty() {
                return Err(AccountError::EmptyComponent);
            }
            AccountNamespace::SystemLiquidity
        } else {
            return Err(AccountError::UnknownNamespace);
        };
        Ok(Self {
            namespace,
            canonical: canonical.to_owned(),
        })
    }

    /// Returns the validated namespace.
    #[must_use]
    pub const fn namespace(&self) -> AccountNamespace {
        self.namespace
    }

    /// Returns the exact canonical name.
    #[must_use]
    pub fn canonical(&self) -> &str {
        &self.canonical
    }
}

fn parse_agent(agent: &str) -> Result<AccountNamespace, AccountError> {
    if let Some(did) = agent.strip_suffix(":main") {
        return if did.is_empty() || did.len() > MAX_DID_BYTES {
            Err(AccountError::EmptyComponent)
        } else {
            Ok(AccountNamespace::AgentMain)
        };
    }
    if let Some((did, asset)) = agent.rsplit_once(":asset:") {
        if asset.len() == 64
            && asset
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return if did.is_empty() || did.len() > MAX_DID_BYTES {
                Err(AccountError::EmptyComponent)
            } else {
                Ok(AccountNamespace::AgentAsset)
            };
        }
    }
    for (marker, namespace) in [
        (":budget:", AccountNamespace::AgentBudget),
        (":escrow:", AccountNamespace::AgentEscrow),
        (":margin:", AccountNamespace::AgentMargin),
    ] {
        if let Some(index) = agent.rfind(marker) {
            let (did, tail) = agent.split_at(index);
            let component = &tail[marker.len()..];
            return if did.is_empty() || did.len() > MAX_DID_BYTES || component.is_empty() {
                Err(AccountError::EmptyComponent)
            } else {
                Ok(namespace)
            };
        }
    }
    Err(AccountError::UnknownNamespace)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_asset_namespace_is_exact() {
        let valid = format!("agent:did:layerx:alice:asset:{}", "ab".repeat(32));
        assert_eq!(
            AccountId::parse(&valid).map(|a| a.namespace()),
            Ok(AccountNamespace::AgentAsset)
        );
        for asset in [
            "ab".repeat(31),
            "ab".repeat(33),
            "AB".repeat(32),
            "gg".repeat(32),
            format!("{}:extra", "ab".repeat(32)),
        ] {
            assert!(AccountId::parse(&format!("agent:did:layerx:alice:asset:{asset}")).is_err());
        }
        assert!(AccountId::parse(&format!("agent::asset:{}", "ab".repeat(32))).is_err());
        assert!(AccountId::parse(&format!(
            "agent:{}:asset:{}",
            "a".repeat(MAX_DID_BYTES + 1),
            "ab".repeat(32)
        ))
        .is_err());
        for did in [
            "did:layerx:budget:alice",
            "did:layerx:escrow:alice",
            "did:layerx:margin:alice",
        ] {
            assert_eq!(
                AccountId::parse(&format!("agent:{did}:asset:{}", "ab".repeat(32)))
                    .map(|a| a.namespace()),
                Ok(AccountNamespace::AgentAsset)
            );
        }
        assert!(AccountId::parse("agent:did:layerx:alice:main").is_ok());
        assert!(AccountId::parse("agent:did:layerx:alice:unknown:abc").is_err());
    }
}
