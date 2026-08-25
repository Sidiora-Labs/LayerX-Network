//! Construction of proof-only offline verification exports.

use layerx_proof::checkpoint::SettlementDomain;
use layerx_proof::export::{verify, ExportVerificationError, OfflineExport, VerificationReport};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuiltExport {
    pub artifact: OfflineExport,
    pub local_verification: VerificationReport,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExportBuildError {
    Empty,
    Verification(ExportVerificationError),
}

/// Builds an export only from proof-verifiable core evidence and labelled local views.
///
/// # Errors
///
/// Refuses an artifact holding no receipts, inclusions or checkpoints, and returns the first
/// verification failure raised by `layerx-proof` over the artifact.
pub fn build(
    artifact: OfflineExport,
    expected_settlement_domain: SettlementDomain,
) -> Result<BuiltExport, ExportBuildError> {
    if artifact.receipts.is_empty()
        && artifact.inclusions.is_empty()
        && artifact.checkpoints.is_empty()
    {
        return Err(ExportBuildError::Empty);
    }
    let local_verification = verify(&artifact, expected_settlement_domain)
        .map_err(ExportBuildError::Verification)?;
    Ok(BuiltExport {
        artifact,
        local_verification,
    })
}
