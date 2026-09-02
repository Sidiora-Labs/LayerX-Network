//! Loader for the cross-language checkpoint identity and freshness vectors.

use std::fs;
use std::path::{Path, PathBuf};

use crate::json::{self, JsonError, JsonValue};
use crate::vectors::CorpusError;

/// Schema identifier of every checkpoint vector document.
pub const CHECKPOINT_VECTOR_SCHEMA: &str = "layerx/checkpoint-vector/1";

/// Repository-relative directory holding the checkpoint vectors.
pub const CHECKPOINT_VECTOR_DIRECTORY: &str = "tests/vectors/checkpoint";

/// Every published checkpoint vector case, in publication order.
pub const CHECKPOINT_VECTOR_CASES: [&str; 5] = [
    "fresh",
    "too_early",
    "too_late",
    "boundary_low",
    "boundary_high",
];

/// Exact width of the signed guarantor attestation message.
pub const ATTESTATION_MESSAGE_BYTES: usize = 189;

/// Exact freshness rejection a vector expects from every verifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckpointRejection {
    /// The attestation precedes the checkpoint header timestamp.
    NotYetValid,
    /// The attestation follows the header timestamp plus the maximum delay.
    Expired,
}

/// Verifier outcome a vector expects.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckpointOutcome {
    /// Every verifier must accept the certificate.
    Accept,
    /// Every verifier must reject the certificate for the named reason.
    Reject(CheckpointRejection),
}

/// Canonical header fields alongside their exact encoding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckpointVectorHeader {
    pub protocol_version: u16,
    pub network_id: u32,
    pub epoch: u64,
    pub batch_number: u64,
    pub first_sequence: u64,
    pub last_sequence: u64,
    pub previous_state_root: [u8; 32],
    pub resulting_state_root: [u8; 32],
    pub activity_merkle_root: [u8; 32],
    pub receipt_merkle_root: [u8; 32],
    pub event_merkle_root: [u8; 32],
    pub data_availability_root: [u8; 32],
    pub oracle_root: [u8; 32],
    pub timestamp_ms: u64,
    pub sequencer_id: [u8; 32],
    pub bytes: Vec<u8>,
}

/// One signed attestation with its exact message and digest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckpointVectorAttestation {
    pub guarantor_id: [u8; 32],
    pub replayed: bool,
    pub data_possessed: bool,
    pub availability_class_mask: u8,
    pub attested_at_ms: u64,
    pub signer: [u8; 20],
    pub signature: [u8; 64],
    pub signature_v: u8,
    pub message: [u8; ATTESTATION_MESSAGE_BYTES],
    pub digest: [u8; 32],
}

/// One complete checkpoint vector.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckpointVector {
    pub case_name: String,
    pub settlement_domain: String,
    pub outcome: CheckpointOutcome,
    pub header: CheckpointVectorHeader,
    pub validity_proof: Vec<u8>,
    pub threshold: usize,
    pub expected_digest: [u8; 32],
    pub attestations: Vec<CheckpointVectorAttestation>,
}

fn format_error(path: &Path, detail: String) -> CorpusError {
    CorpusError::Format {
        path: path.to_path_buf(),
        detail,
    }
}

fn json_error(path: &Path) -> impl Fn(JsonError) -> CorpusError + '_ {
    move |error| format_error(path, error.to_string())
}

fn header(document: &JsonValue, path: &Path) -> Result<CheckpointVectorHeader, CorpusError> {
    let json = json_error(path);
    let protocol_version =
        u16::try_from(document.u64_at("header.protocol_version").map_err(&json)?)
            .map_err(|_| format_error(path, "header.protocol_version exceeds u16".to_owned()))?;
    let network_id = u32::try_from(document.u64_at("header.network_id").map_err(&json)?)
        .map_err(|_| format_error(path, "header.network_id exceeds u32".to_owned()))?;
    Ok(CheckpointVectorHeader {
        protocol_version,
        network_id,
        epoch: document.u64_at("header.epoch").map_err(&json)?,
        batch_number: document.u64_at("header.batch_number").map_err(&json)?,
        first_sequence: document.u64_at("header.first_sequence").map_err(&json)?,
        last_sequence: document.u64_at("header.last_sequence").map_err(&json)?,
        previous_state_root: document
            .hex_array_at("header.previous_state_root")
            .map_err(&json)?,
        resulting_state_root: document
            .hex_array_at("header.resulting_state_root")
            .map_err(&json)?,
        activity_merkle_root: document
            .hex_array_at("header.activity_merkle_root")
            .map_err(&json)?,
        receipt_merkle_root: document
            .hex_array_at("header.receipt_merkle_root")
            .map_err(&json)?,
        event_merkle_root: document
            .hex_array_at("header.event_merkle_root")
            .map_err(&json)?,
        data_availability_root: document
            .hex_array_at("header.data_availability_root")
            .map_err(&json)?,
        oracle_root: document.hex_array_at("header.oracle_root").map_err(&json)?,
        timestamp_ms: document.u64_at("header.timestamp_ms").map_err(&json)?,
        sequencer_id: document
            .hex_array_at("header.sequencer_id")
            .map_err(&json)?,
        bytes: document.hex_at("header.bytes").map_err(&json)?,
    })
}

fn attestation(
    document: &JsonValue,
    index: usize,
    path: &Path,
) -> Result<CheckpointVectorAttestation, CorpusError> {
    let json = json_error(path);
    let prefix = format!("attestations.{index}");
    let mask = document
        .u64_at(&format!("{prefix}.availability_class_mask"))
        .map_err(&json)?;
    let signature_v = document
        .u64_at(&format!("{prefix}.signature_v"))
        .map_err(&json)?;
    let availability_class_mask = u8::try_from(mask)
        .map_err(|_| format_error(path, format!("{prefix}.availability_class_mask exceeds u8")))?;
    let signature_v = u8::try_from(signature_v)
        .map_err(|_| format_error(path, format!("{prefix}.signature_v exceeds u8")))?;
    Ok(CheckpointVectorAttestation {
        guarantor_id: document
            .hex_array_at(&format!("{prefix}.guarantor_id"))
            .map_err(&json)?,
        replayed: document
            .bool_at(&format!("{prefix}.replayed"))
            .map_err(&json)?,
        data_possessed: document
            .bool_at(&format!("{prefix}.data_possessed"))
            .map_err(&json)?,
        availability_class_mask,
        attested_at_ms: document
            .u64_at(&format!("{prefix}.attested_at_ms"))
            .map_err(&json)?,
        signer: document
            .hex_array_at(&format!("{prefix}.signer"))
            .map_err(&json)?,
        signature: document
            .hex_array_at(&format!("{prefix}.signature"))
            .map_err(&json)?,
        signature_v,
        message: document
            .hex_array_at(&format!("{prefix}.message"))
            .map_err(&json)?,
        digest: document
            .hex_array_at(&format!("{prefix}.digest"))
            .map_err(&json)?,
    })
}

fn outcome(document: &JsonValue, path: &Path) -> Result<CheckpointOutcome, CorpusError> {
    let json = json_error(path);
    let expected = document.str_at("expected_outcome").map_err(&json)?;
    let rejection = document.str_at("expected_rejection").map_err(&json)?;
    match (expected, rejection) {
        ("accept", "none") => Ok(CheckpointOutcome::Accept),
        ("reject", "not_yet_valid") => {
            Ok(CheckpointOutcome::Reject(CheckpointRejection::NotYetValid))
        }
        ("reject", "expired") => Ok(CheckpointOutcome::Reject(CheckpointRejection::Expired)),
        _ => Err(format_error(
            path,
            format!("unsupported outcome {expected} with rejection {rejection}"),
        )),
    }
}

/// Loads and validates one checkpoint vector document.
///
/// # Errors
///
/// Returns an I/O or format error naming the file. A vector whose case name
/// differs from the expected case is rejected.
pub fn load_checkpoint_vector(
    path: &Path,
    expected_case: &str,
) -> Result<CheckpointVector, CorpusError> {
    let text = fs::read_to_string(path).map_err(|error| CorpusError::Io {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;
    let document = json::parse(&text).map_err(json_error(path))?;
    let json = json_error(path);
    let schema = document.str_at("schema").map_err(&json)?;
    if schema != CHECKPOINT_VECTOR_SCHEMA {
        return Err(format_error(path, format!("unsupported schema {schema}")));
    }
    let case_name = document.str_at("case").map_err(&json)?;
    if case_name != expected_case {
        return Err(format_error(
            path,
            format!("case {case_name} differs from {expected_case}"),
        ));
    }
    let settlement_domain = document.str_at("settlement_domain").map_err(&json)?;
    let outcome = outcome(&document, path)?;
    let header = header(&document, path)?;
    let validity_proof = document
        .hex_at("certificate.validity_proof")
        .map_err(&json)?;
    let threshold = usize::try_from(document.u64_at("certificate.threshold").map_err(&json)?)
        .map_err(|_| format_error(path, "certificate.threshold exceeds usize".to_owned()))?;
    let expected_digest = document.hex_array_at("expected_digest").map_err(&json)?;
    let count = document.array_at("attestations").map_err(&json)?.len();
    if count == 0 {
        return Err(format_error(
            path,
            "vector carries no attestation".to_owned(),
        ));
    }
    let mut attestations = Vec::with_capacity(count);
    for index in 0..count {
        attestations.push(attestation(&document, index, path)?);
    }
    Ok(CheckpointVector {
        case_name: case_name.to_owned(),
        settlement_domain: settlement_domain.to_owned(),
        outcome,
        header,
        validity_proof,
        threshold,
        expected_digest,
        attestations,
    })
}

/// Returns the path of one published checkpoint vector case.
#[must_use]
pub fn checkpoint_vector_path(repository_root: &Path, case_name: &str) -> PathBuf {
    repository_root
        .join(CHECKPOINT_VECTOR_DIRECTORY)
        .join(format!("{case_name}.json"))
}

/// Loads every published checkpoint vector case in publication order.
///
/// # Errors
///
/// Returns the first I/O or format error. No case is skipped.
pub fn load_checkpoint_vectors(
    repository_root: &Path,
) -> Result<Vec<CheckpointVector>, CorpusError> {
    CHECKPOINT_VECTOR_CASES
        .iter()
        .map(|case_name| {
            load_checkpoint_vector(
                &checkpoint_vector_path(repository_root, case_name),
                case_name,
            )
        })
        .collect()
}
