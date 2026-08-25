//! Checkpoint certificates and root-bound inclusion proof types.

use crate::batch::BatchHeader;

/// The exact canonical fields carried by one Paxeer guarantor attestation.
pub const GUARANTOR_ATTESTATION_FIELDS: [&str; 17] = [
    "protocol_version",
    "network_id",
    "paxeer_chain_id",
    "paxeer_settlement_contract",
    "epoch",
    "checkpoint_id",
    "checkpoint_hash",
    "guarantor_id",
    "batch_number",
    "data_availability_root",
    "replayed",
    "da_possessed",
    "availability_class_mask",
    "attested_at_ms",
    "signer",
    "signature",
    "signature_v",
];

/// One canonical Merkle path decoded from core bytes.
#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MerkleProof {
    leaf_index: u32,
    leaf_count: u32,
    siblings: Vec<[u8; 32]>,
}

/// Activity inclusion tied to the activity root it claims to establish.
#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivityInclusionProof {
    expected_activity_root: [u8; 32],
    proof: MerkleProof,
}

/// State inclusion tied to the resulting state root it claims to establish.
#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StateInclusionProof {
    expected_state_root: [u8; 32],
    proof: MerkleProof,
}

/// Availability-chunk inclusion tied to its committed availability root.
#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AvailabilityChunkInclusionProof {
    expected_data_availability_root: [u8; 32],
    proof: MerkleProof,
}

/// One replay-and-possession attestation over a checkpoint.
#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuarantorAttestation {
    protocol_version: u16,
    network_id: u32,
    paxeer_chain_id: u64,
    paxeer_settlement_contract: [u8; 20],
    epoch: u64,
    checkpoint_id: [u8; 32],
    checkpoint_hash: [u8; 32],
    guarantor_id: [u8; 32],
    batch_number: u64,
    data_availability_root: [u8; 32],
    replayed: bool,
    da_possessed: bool,
    availability_class_mask: u8,
    attested_at_ms: u64,
    signer: [u8; 20],
    signature: [u8; 64],
    signature_v: u8,
}

/// The checkpoint body hashed before guarantor attestation.
#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckpointBody {
    header: BatchHeader,
    validity_proof: Box<[u8]>,
}

/// A threshold certificate decoded from the core-produced representation.
#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckpointCertificate {
    checkpoint: CheckpointBody,
    attestations: Vec<GuarantorAttestation>,
    threshold: usize,
    bonded_economic_guarantee: bool,
    validity_proof_present: bool,
}

/// A bounded settlement reference attached only after checkpointing.
#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaxeerSettlementReference(Box<[u8]>);
