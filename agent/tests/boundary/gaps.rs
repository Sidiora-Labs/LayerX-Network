use layerx_client::lni::capability_report;
use layerx_client::lni::handshake::Handshake;
use layerx_client::lni::schema::Capability;
use layerx_types::error::LayerError;

pub fn verify_and_render(handshake: &Handshake) -> Result<String, String> {
    let report = capability_report(handshake.capabilities());
    if report.entries().len() != 15 {
        return Err("capability report omitted schema requirements".to_owned());
    }
    if report.daemon_status() != report.cli_output()
        || report.cli_output() != report.qualification_output()
    {
        return Err("daemon, CLI, and qualification capability views diverged".to_owned());
    }
    if report.gaps()
        != [
            "authenticated_durable_submit",
            "historical_proofs",
            "finality_evidence_register",
        ]
    {
        return Err(format!(
            "unexpected real-node capability gaps: {:?}",
            report.gaps()
        ));
    }
    if report.require(Capability::AuthenticatedDurableSubmit)
        != Err(LayerError::UnavailableCapability {
            capability: "authenticated_durable_submit".to_owned(),
        })
    {
        return Err("legacy local submission opened the durable write gate".to_owned());
    }
    if report.require(Capability::HistoricalProofs)
        != Err(LayerError::UnavailableCapability {
            capability: "historical_proofs".to_owned(),
        })
    {
        return Err("missing capability did not fail as unavailable".to_owned());
    }
    if report.require(Capability::FinalityEvidenceRegister)
        != Err(LayerError::UnavailableCapability {
            capability: "finality_evidence_register".to_owned(),
        })
    {
        return Err("unbound finality authority did not fail as unavailable".to_owned());
    }
    if !report
        .qualification_output()
        .contains("capability=authenticated_durable_submit exposed=false absent_behavior=authenticated_durable_submission_unavailable")
    {
        return Err("qualification omitted the durable-write downgrade".to_owned());
    }
    if !report
        .qualification_output()
        .contains("capability=historical_proofs exposed=false absent_behavior=historical_verification_unavailable")
    {
        return Err("qualification omitted the real-node gap".to_owned());
    }
    if !report.qualification_output().contains(
        "capability=finality_evidence_register exposed=false absent_behavior=finality_evidence_registration_unavailable",
    ) {
        return Err("qualification omitted the finality-authority gap".to_owned());
    }
    Ok(report.qualification_output())
}
