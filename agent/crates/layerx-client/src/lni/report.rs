//! Stable capability gap records shared by status, CLI, and qualification.

use std::fmt::Write as _;

use layerx_types::error::LayerError;

use super::capabilities::Capabilities;
use super::schema::{lni_schema_v1, Capability};

/// Required node capability and the layer's exact behavior when it is absent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapabilityStatus {
    pub capability: Capability,
    pub exposed: bool,
    pub absent_behavior: &'static str,
}

/// Complete, ordered intersection report for one accepted handshake.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityReport {
    entries: Vec<CapabilityStatus>,
    unknown_advertised: Vec<String>,
}

impl CapabilityReport {
    /// Every required schema capability, including those the node omitted.
    #[must_use]
    pub fn entries(&self) -> &[CapabilityStatus] {
        &self.entries
    }

    /// Node-advertised names this client version does not interpret.
    #[must_use]
    pub fn unknown_advertised(&self) -> &[String] {
        &self.unknown_advertised
    }

    /// Requires an exposed capability. No fallback or reconstruction callback
    /// exists on this API.
    ///
    /// # Errors
    ///
    /// Returns the exact unavailable-capability error for an omitted entry.
    pub fn require(&self, capability: Capability) -> Result<(), LayerError> {
        let exposed = self
            .entries
            .iter()
            .any(|entry| entry.capability == capability && entry.exposed);
        if exposed {
            Ok(())
        } else {
            Err(LayerError::UnavailableCapability {
                capability: capability.name().to_owned(),
            })
        }
    }

    /// Canonical text consumed without transformation by daemon status, the
    /// operator CLI, and qualification output.
    #[must_use]
    pub fn render(&self) -> String {
        let mut output = String::from("boundary_capabilities version=1\n");
        for entry in &self.entries {
            let _ = writeln!(
                output,
                "capability={} exposed={} absent_behavior={}",
                entry.capability.name(),
                entry.exposed,
                entry.absent_behavior
            );
        }
        for unknown in &self.unknown_advertised {
            let _ = writeln!(output, "unknown_advertised={unknown}");
        }
        output
    }

    /// Exact daemon status section.
    #[must_use]
    pub fn daemon_status(&self) -> String {
        self.render()
    }

    /// Exact operator command-line section.
    #[must_use]
    pub fn cli_output(&self) -> String {
        self.render()
    }

    /// Exact capability section embedded into release qualification.
    #[must_use]
    pub fn qualification_output(&self) -> String {
        self.render()
    }

    /// Missing capability names in schema order.
    #[must_use]
    pub fn gaps(&self) -> Vec<&'static str> {
        self.entries
            .iter()
            .filter(|entry| !entry.exposed)
            .map(|entry| entry.capability.name())
            .collect()
    }
}

/// Builds the exhaustive gap report from the negotiated intersection.
#[must_use]
pub fn capability_report(capabilities: &Capabilities) -> CapabilityReport {
    let entries = lni_schema_v1()
        .capabilities
        .iter()
        .copied()
        .map(|capability| CapabilityStatus {
            capability,
            exposed: capabilities.contains(capability),
            absent_behavior: absent_behavior(capability),
        })
        .collect();
    CapabilityReport {
        entries,
        unknown_advertised: capabilities.unknown_advertised().to_vec(),
    }
}

const fn absent_behavior(capability: Capability) -> &'static str {
    match capability {
        Capability::NodeInfo => "startup_refused",
        Capability::Submit => "submission_unavailable",
        Capability::AuthenticatedDurableSubmit => "authenticated_durable_submission_unavailable",
        Capability::ReceiptLookup => "receipt_resolution_unavailable",
        Capability::AccountRead => "verified_account_read_unavailable",
        Capability::HistoryRange => "history_read_unavailable",
        Capability::BatchHeader => "batch_verification_unavailable",
        Capability::Checkpoint => "finality_verification_unavailable",
        Capability::ProofBundle => "proof_read_unavailable",
        Capability::AvailabilityFetch => "availability_read_unavailable",
        Capability::EventSubscribe => "event_stream_unavailable",
        Capability::HistoricalProofs => "historical_verification_unavailable",
        Capability::PreparationState => "preparation_unavailable",
        Capability::FinalityEvidenceRegister => "finality_evidence_registration_unavailable",
        Capability::Simulate => "program_simulation_unavailable",
    }
}
