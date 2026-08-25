//! Minimal independent reviewer entry point for a daemon-produced audit export.

use layerx_agentd::audit::{verify_chain_material, AuditExport};
use layerx_proof::export::verify;
use layerx_proof::checkpoint::SettlementDomain;

fn review_without_daemon(
    export: &AuditExport,
    expected_settlement_domain: SettlementDomain,
) -> Result<(), String> {
    verify_chain_material(&export.chain)
        .map_err(|error| format!("audit chain verification failed: {error}"))?;
    for evidence in &export.referenced_evidence {
        verify(&evidence.protocol_facts, expected_settlement_domain).map_err(|error| {
            format!(
                "protocol evidence for receipt {:02x?} failed: {error:?}",
                evidence.receipt_id
            )
        })?;
    }
    Ok(())
}

fn main() {
    let reviewer: fn(&AuditExport, SettlementDomain) -> Result<(), String> = review_without_daemon;
    let _ = reviewer;
}
