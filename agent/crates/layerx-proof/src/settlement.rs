//! Declared settlement configuration consumed by certificate verification.

use std::sync::OnceLock;

use layerx_types::settlement::{DeclaredCheckpointSettlement, SettlementError};

use crate::checkpoint::{GuarantorKey, SettlementDomain};

/// Exact text of the declared checkpoint settlement document embedded from
/// `contracts/config/checkpoint-settlement.json`.
pub const DECLARED_CHECKPOINT_SETTLEMENT: &str =
    include_str!("../../../../contracts/config/checkpoint-settlement.json");

/// One declared settlement domain resolved into verifier inputs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeclaredDomain {
    name: String,
    network_id: u32,
    settlement: SettlementDomain,
    guarantor_set: Vec<GuarantorKey>,
    certificate_threshold: usize,
}

impl DeclaredDomain {
    /// Returns the declared domain name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the `LayerX` network identifier headers must commit to.
    #[must_use]
    pub const fn network_id(&self) -> u32 {
        self.network_id
    }

    /// Returns the Paxeer chain and contract attestations must commit to.
    #[must_use]
    pub const fn settlement(&self) -> SettlementDomain {
        self.settlement
    }

    /// Returns the bonded guarantor set of the domain.
    #[must_use]
    pub fn guarantor_set(&self) -> &[GuarantorKey] {
        &self.guarantor_set
    }

    /// Returns the declared certificate threshold.
    #[must_use]
    pub const fn certificate_threshold(&self) -> usize {
        self.certificate_threshold
    }
}

/// Returns the parsed declared settlement configuration.
///
/// # Errors
///
/// Returns the validation failure of the embedded document. The result is
/// computed once and shared.
pub fn declared() -> Result<&'static DeclaredCheckpointSettlement, SettlementError> {
    static DECLARED: OnceLock<Result<DeclaredCheckpointSettlement, SettlementError>> =
        OnceLock::new();
    DECLARED
        .get_or_init(|| DeclaredCheckpointSettlement::parse(DECLARED_CHECKPOINT_SETTLEMENT))
        .as_ref()
        .map_err(Clone::clone)
}

/// Returns the declared maximum header-relative attestation delay.
///
/// # Errors
///
/// Returns the validation failure of the embedded document.
pub fn maximum_attestation_delay_ms() -> Result<u64, SettlementError> {
    Ok(declared()?.finality_policy().maximum_attestation_delay_ms())
}

/// Resolves one declared settlement domain into verifier inputs.
///
/// # Errors
///
/// Returns the validation failure of the embedded document or an
/// unknown-domain error.
pub fn declared_domain(name: &str) -> Result<DeclaredDomain, SettlementError> {
    let settlement = declared()?;
    let domain = settlement.domain(name)?;
    Ok(DeclaredDomain {
        name: domain.name().to_owned(),
        network_id: domain.network_id(),
        settlement: SettlementDomain::new(domain.paxeer_chain_id(), domain.settlement_contract()),
        guarantor_set: domain
            .guarantor_set()
            .iter()
            .map(|guarantor| {
                GuarantorKey::new(guarantor.guarantor_id(), guarantor.public_key(), true)
            })
            .collect(),
        certificate_threshold: settlement.finality_policy().certificate_threshold(),
    })
}
