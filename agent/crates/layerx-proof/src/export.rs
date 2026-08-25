//! Self-contained proof objects for offline third-party verification.

use std::collections::BTreeSet;

use layerx_types::verify::VerificationLevel;

use crate::checkpoint::{
    verify_certificate, Certificate, CheckpointError, GuarantorKey, SettlementDomain,
};
use crate::inclusion::{verify_activity, verify_state, InclusionError, SequencerAuthorization};
use crate::merkle::Proof;
use crate::receipt::{verify_outcome, AuthorizedBatch, ReceiptCheck};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReceiptFact {
    pub statement: String,
    pub canonical_receipt_bytes: Vec<u8>,
    pub authorised_batch: AuthorizedBatch,
    pub expected_receipt_digest: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InclusionKind {
    Activity,
    State,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InclusionFact {
    pub statement: String,
    pub kind: InclusionKind,
    pub canonical_leaf_bytes: Vec<u8>,
    pub proof: Proof,
    pub named_root: [u8; 32],
    pub canonical_header_bytes: Vec<u8>,
    pub header_signature: [u8; 64],
    pub sequencer_authorization: SequencerAuthorization,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckpointFact {
    pub statement: String,
    pub certificate: Certificate,
    pub bonded_set: Vec<GuarantorKey>,
    pub registered_checkpoint_id: [u8; 32],
    pub registered_settlement_reference: Option<Vec<u8>>,
    pub availability_obtained: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DerivedAggregate {
    pub label: String,
    pub rendered_value: String,
    pub contributing_receipt_digests: Vec<[u8; 32]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OfflineExport {
    pub receipts: Vec<ReceiptFact>,
    pub inclusions: Vec<InclusionFact>,
    pub checkpoints: Vec<CheckpointFact>,
    pub derived_aggregates: Vec<DerivedAggregate>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationReport {
    pub verified_receipts: usize,
    pub verified_inclusions: usize,
    pub verified_checkpoints: usize,
    pub receipt_digests: Vec<[u8; 32]>,
    pub achieved_levels: Vec<VerificationLevel>,
    pub derived_aggregates_are_protocol_facts: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExportVerificationError {
    EmptyStatement,
    Receipt {
        index: usize,
        check: ReceiptCheck,
    },
    ReceiptDigest {
        index: usize,
    },
    Inclusion {
        index: usize,
        error: InclusionError,
    },
    InclusionRoot {
        index: usize,
    },
    CheckpointUnavailable {
        index: usize,
    },
    Checkpoint {
        index: usize,
        error: CheckpointError,
    },
    UnknownAggregateContributor {
        aggregate: usize,
        digest: [u8; 32],
    },
    AggregateWithoutContributors {
        aggregate: usize,
    },
}

/// Re-runs every proof using only the export object and `layerx-proof`.
///
/// # Errors
///
/// Returns the first malformed statement, failed receipt, inclusion,
/// checkpoint, or aggregate-consistency check.
pub fn verify(
    export: &OfflineExport,
    expected_settlement_domain: SettlementDomain,
) -> Result<VerificationReport, ExportVerificationError> {
    let mut receipt_digests = Vec::with_capacity(export.receipts.len());
    let mut achieved_levels = Vec::new();
    for (index, fact) in export.receipts.iter().enumerate() {
        require_statement(&fact.statement)?;
        let verified = verify_outcome(&fact.canonical_receipt_bytes, &fact.authorised_batch)
            .map_err(|failure| ExportVerificationError::Receipt {
                index,
                check: failure.check,
            })?;
        let digest = verified.evidence().receipt_digest().unwrap_or([0; 32]);
        if digest != fact.expected_receipt_digest {
            return Err(ExportVerificationError::ReceiptDigest { index });
        }
        receipt_digests.push(digest);
        achieved_levels.push(verified.level());
    }
    for (index, fact) in export.inclusions.iter().enumerate() {
        require_statement(&fact.statement)?;
        let evidence = match fact.kind {
            InclusionKind::Activity => verify_activity(
                &fact.canonical_leaf_bytes,
                &fact.proof,
                &fact.canonical_header_bytes,
                &fact.header_signature,
                &fact.sequencer_authorization,
            ),
            InclusionKind::State => verify_state(
                &fact.canonical_leaf_bytes,
                &fact.proof,
                &fact.named_root,
                &fact.canonical_header_bytes,
                &fact.header_signature,
                &fact.sequencer_authorization,
            ),
        }
        .map_err(|error| ExportVerificationError::Inclusion { index, error })?;
        let actual_root = match fact.kind {
            InclusionKind::Activity => evidence.header().header().activity_merkle_root(),
            InclusionKind::State => evidence.header().header().resulting_state_root(),
        };
        if actual_root != fact.named_root {
            return Err(ExportVerificationError::InclusionRoot { index });
        }
        achieved_levels.push(evidence.level());
    }
    for (index, fact) in export.checkpoints.iter().enumerate() {
        require_statement(&fact.statement)?;
        if !fact.availability_obtained {
            return Err(ExportVerificationError::CheckpointUnavailable { index });
        }
        let report = verify_certificate(
            &fact.certificate,
            &fact.bonded_set,
            &fact.registered_checkpoint_id,
            expected_settlement_domain,
            fact.registered_settlement_reference.as_deref(),
        )
        .map_err(|error| ExportVerificationError::Checkpoint { index, error })?;
        achieved_levels.push(report.level());
    }
    let known: BTreeSet<_> = receipt_digests.iter().copied().collect();
    for (aggregate, view) in export.derived_aggregates.iter().enumerate() {
        if view.contributing_receipt_digests.is_empty() {
            return Err(ExportVerificationError::AggregateWithoutContributors { aggregate });
        }
        for digest in &view.contributing_receipt_digests {
            if !known.contains(digest) {
                return Err(ExportVerificationError::UnknownAggregateContributor {
                    aggregate,
                    digest: *digest,
                });
            }
        }
    }
    Ok(VerificationReport {
        verified_receipts: export.receipts.len(),
        verified_inclusions: export.inclusions.len(),
        verified_checkpoints: export.checkpoints.len(),
        receipt_digests,
        achieved_levels,
        derived_aggregates_are_protocol_facts: false,
    })
}

fn require_statement(statement: &str) -> Result<(), ExportVerificationError> {
    if statement.is_empty() {
        Err(ExportVerificationError::EmptyStatement)
    } else {
        Ok(())
    }
}
