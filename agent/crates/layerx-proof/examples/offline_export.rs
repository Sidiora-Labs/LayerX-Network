//! Minimal offline-verifier entry point.

use layerx_proof::checkpoint::SettlementDomain;
use layerx_proof::export::{verify, OfflineExport};

fn verify_without_network(
    artifact: &OfflineExport,
    expected_settlement_domain: SettlementDomain,
) -> Result<layerx_proof::export::VerificationReport, layerx_proof::export::ExportVerificationError>
{
    verify(artifact, expected_settlement_domain)
}

fn main() {
    let verifier: fn(
        &OfflineExport,
        SettlementDomain,
    ) -> Result<
        layerx_proof::export::VerificationReport,
        layerx_proof::export::ExportVerificationError,
    > = verify_without_network;
    let _ = verifier;
}
