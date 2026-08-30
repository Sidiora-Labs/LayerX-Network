use std::path::Path;
use std::process::Command;

/// Runs and audits the external-process boundary suite against core-linked `layerxd`.
///
/// # Errors
///
/// Fails on any message, lifecycle mode, pagination, negotiation or capability gap.
pub fn agent_qualify_boundary_gate(
    repository: &Path,
    node: &Path,
    harness: &Path,
) -> Result<String, String> {
    if !node.is_file() || !harness.is_file() {
        return Err(
            "boundary qualification requires built node and harness executables".to_owned(),
        );
    }
    if !repository
        .join("agent/tests/boundary/node/layerxd_lni.c")
        .is_file()
    {
        return Err("core-linked layerxd source is missing".to_owned());
    }
    let suite_source = std::fs::read_to_string(repository.join("agent/tests/boundary/cases.rs"))
        .map_err(|error| format!("could not audit boundary suite source: {error}"))?;
    for required in [
        "covered != (1_u16..=27).collect()",
        "incompatible request",
        "page_two",
        "unreachable-node case",
        "restart handshake",
        "behind-node handshake",
        "degraded-node handshake",
    ] {
        if !suite_source.contains(required) {
            return Err(format!("boundary suite no longer covers {required}"));
        }
    }
    let output = Command::new(harness)
        .arg(node)
        .arg(repository)
        .output()
        .map_err(|error| format!("could not start boundary harness: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "boundary harness failed: status={} stderr={}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|_| "boundary harness emitted non-UTF-8 output".to_owned())?;
    for required in [
        "real node, 27 messages, restart, behind, unreachable, degraded",
        "boundary_capabilities version=1",
        "capability=historical_proofs exposed=false absent_behavior=historical_verification_unavailable",
    ] {
        if !stdout.contains(required) {
            return Err(format!("boundary result omitted {required}"));
        }
    }
    let capabilities = stdout
        .lines()
        .filter(|line| line.starts_with("capability="))
        .collect::<Vec<_>>();
    if capabilities.len() != 12 {
        return Err(format!(
            "capability report has {} rows instead of 12",
            capabilities.len()
        ));
    }
    if capabilities
        .iter()
        .filter(|line| line.contains("exposed=false"))
        .count()
        != 1
        || capabilities
            .iter()
            .filter(|line| line.contains("exposed=true"))
            .count()
            != 11
    {
        return Err(
            "capability report did not preserve eleven capabilities and one explicit gap"
                .to_owned(),
        );
    }
    Ok(format!(
        "agent_qualify_boundary_gate passed external_node=true messages=27 modes=restart,behind,unreachable,degraded\n{}",
        capabilities.join("\n")
    ))
}
