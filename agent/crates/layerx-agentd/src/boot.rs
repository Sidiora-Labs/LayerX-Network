//! Startup and reconnection boundary-handshake gate.

use std::collections::BTreeSet;

use layerx_client::lni::handshake::{
    perform, Handshake, HandshakeConfig, HandshakeError, NodeRole,
};
use layerx_client::lni::report::{capability_report, CapabilityReport};
use layerx_client::lni::schema::{Capability, Version};
use layerx_client::lni::transport::FrameTransport;

use crate::config::StartupConfig;
use crate::protocol_evidence::{EvidenceAuthority, ProtocolEvidenceVerifier, VerifierPolicyError};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HandshakeStatus {
    pub generation: u64,
    pub interface_version: Version,
    pub protocol_version: u16,
    pub network_id: u32,
    pub node_role: NodeRole,
    pub chain_head_sequence: u64,
    pub latest_sealed_batch: u64,
    pub latest_finalised_checkpoint: [u8; 32],
    pub authorised_sequencer_key: [u8; 32],
    pub available_capabilities: BTreeSet<Capability>,
    pub missing_capabilities: BTreeSet<Capability>,
    pub unknown_advertised: Vec<String>,
    pub writes_ready: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Status {
    AwaitingHandshake,
    Ready(HandshakeStatus),
    Refused(HandshakeError),
    EvidenceRefused(VerifierPolicyError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriteGateError {
    NotReady,
    MissingCapability(Capability),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GateError {
    Handshake(HandshakeError),
    Evidence(VerifierPolicyError),
}

impl From<HandshakeError> for GateError {
    fn from(value: HandshakeError) -> Self {
        Self::Handshake(value)
    }
}

#[derive(Clone, Debug)]
pub struct Gate {
    config: HandshakeConfig,
    last_accepted: Option<Handshake>,
    status: Status,
    generation: u64,
    evidence: EvidenceAuthority,
}

impl Gate {
    /// Loads the trusted sequencer registry before opening any write gate.
    ///
    /// # Errors
    ///
    /// Refuses an unavailable or malformed configured authority source.
    pub fn new(config: &StartupConfig) -> Result<Self, GateError> {
        let verifier = ProtocolEvidenceVerifier::load(config).map_err(GateError::Evidence)?;
        Ok(Self {
            config: HandshakeConfig {
                built_interface_version: Version::V1_0,
                expected_protocol_version: config.expected_protocol_version,
                expected_network_id: config.network_id,
            },
            last_accepted: None,
            status: Status::AwaitingHandshake,
            generation: 0,
            evidence: EvidenceAuthority::new(verifier),
        })
    }

    #[must_use]
    pub const fn status(&self) -> &Status {
        &self.status
    }

    #[must_use]
    pub fn capability_report(&self) -> Option<CapabilityReport> {
        self.last_accepted
            .as_ref()
            .filter(|_| matches!(self.status, Status::Ready(_)))
            .map(|accepted| capability_report(accepted.capabilities()))
    }

    pub fn disconnected(&mut self) {
        self.status = Status::AwaitingHandshake;
    }

    /// Executes a write only after a successful current handshake that exposed submission.
    ///
    /// # Errors
    ///
    /// Returns `NotReady` before startup or after disconnect/refusal, and names the missing
    /// submission capability after a compatible handshake with a reduced intersection.
    pub fn guard_write<T>(&self, operation: impl FnOnce() -> T) -> Result<T, WriteGateError> {
        match &self.status {
            Status::Ready(status) if status.writes_ready => Ok(operation()),
            Status::Ready(_) => Err(WriteGateError::MissingCapability(Capability::Submit)),
            Status::AwaitingHandshake | Status::Refused(_) | Status::EvidenceRefused(_) => {
                Err(WriteGateError::NotReady)
            }
        }
    }

    /// Returns daemon-owned evidence authority only while the current handshake is write-ready.
    ///
    /// # Errors
    ///
    /// Refuses startup, disconnect, handshake refusal, authority refusal, or a node
    /// lacking submission capability.
    pub fn evidence_authority(&self) -> Result<&EvidenceAuthority, WriteGateError> {
        match &self.status {
            Status::Ready(status) if status.writes_ready => Ok(&self.evidence),
            Status::Ready(_) => Err(WriteGateError::MissingCapability(Capability::Submit)),
            Status::AwaitingHandshake | Status::Refused(_) | Status::EvidenceRefused(_) => {
                Err(WriteGateError::NotReady)
            }
        }
    }
}

/// Performs and applies the mandatory startup or reconnection boundary handshake.
///
/// # Errors
///
/// Refuses transport, interface, protocol, network, or malformed-node failures and leaves
/// writes not-ready. Every call performs a new exchange and recomputes the intersection.
pub fn handshake_gate<'a, T: FrameTransport>(
    gate: &'a mut Gate,
    transport: &mut T,
) -> Result<&'a HandshakeStatus, GateError> {
    let accepted = match perform(transport, &gate.config, gate.last_accepted.as_ref()) {
        Ok(accepted) => accepted,
        Err(error) => {
            gate.status = Status::Refused(error);
            return Err(GateError::Handshake(error));
        }
    };
    apply_handshake(gate, accepted)
}

fn apply_handshake(gate: &mut Gate, accepted: Handshake) -> Result<&HandshakeStatus, GateError> {
    let node = accepted.node();
    if node.protocol_version != gate.config.expected_protocol_version {
        gate.status = Status::EvidenceRefused(VerifierPolicyError::ProtocolVersion);
        return Err(GateError::Evidence(VerifierPolicyError::ProtocolVersion));
    }
    if node.network_id != gate.config.expected_network_id {
        gate.status = Status::EvidenceRefused(VerifierPolicyError::Network);
        return Err(GateError::Evidence(VerifierPolicyError::Network));
    }
    if !gate
        .evidence
        .verifier()
        .accepts_handshake_key(node.latest_sealed_batch, node.authorised_sequencer_key)
    {
        gate.status = Status::EvidenceRefused(VerifierPolicyError::HandshakeKey);
        return Err(GateError::Evidence(VerifierPolicyError::HandshakeKey));
    }
    gate.generation = gate.generation.saturating_add(1);
    let capabilities = accepted.capabilities();
    let status = HandshakeStatus {
        generation: gate.generation,
        interface_version: node.interface_version,
        protocol_version: node.protocol_version,
        network_id: node.network_id,
        node_role: node.role,
        chain_head_sequence: node.chain_head_sequence,
        latest_sealed_batch: node.latest_sealed_batch,
        latest_finalised_checkpoint: node.latest_finalised_checkpoint,
        authorised_sequencer_key: node.authorised_sequencer_key,
        available_capabilities: capabilities.available().clone(),
        missing_capabilities: capabilities.unavailable().clone(),
        unknown_advertised: capabilities.unknown_advertised().to_vec(),
        writes_ready: capabilities.contains(Capability::Submit),
    };
    gate.last_accepted = Some(accepted);
    gate.status = Status::Ready(status);
    match &gate.status {
        Status::Ready(status) => Ok(status),
        Status::AwaitingHandshake | Status::Refused(_) | Status::EvidenceRefused(_) => {
            unreachable!()
        }
    }
}
