//! Receipt-backed detail assembly for one activity entry.

use std::collections::BTreeSet;
use std::fmt::{Display, Formatter, Write as _};

use layerx_agent_api::verify::Level;
use layerx_proof::receipt::{canonical_protocol_facts, VerifiedReceipt};
use sha2::{Digest as _, Sha256};

use super::{ActivityEntry, ActivityKind, ActivityStatus, DepositStage, WithdrawalStage};
use crate::notify::ActivityEntryId;

/// Exact money facts decoded from a locally verified protocol receipt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReceiptActual {
    reference: String,
    asset: [u8; 32],
    amount: u128,
    fee: u128,
}

impl ReceiptActual {
    pub(crate) fn from_verified_journey_bytes(
        bytes: &[u8],
        reference: &str,
    ) -> Result<Self, DetailError> {
        if super::hex(Sha256::digest(bytes)) != reference {
            return Err(DetailError::InvalidReceiptEvidence);
        }
        let facts =
            canonical_protocol_facts(bytes).map_err(|_| DetailError::InvalidReceiptEvidence)?;
        Ok(Self {
            reference: reference.to_owned(),
            asset: facts.asset(),
            amount: facts.amount(),
            fee: facts.fee_charged(),
        })
    }
    /// Constructs actuals only from a receipt that passed the proof verifier.
    ///
    /// # Errors
    ///
    /// Refuses a verified value without its receipt digest or canonical facts.
    pub fn from_verified(receipt: &VerifiedReceipt) -> Result<Self, DetailError> {
        if receipt.evidence().receipt_digest().is_none() {
            return Err(DetailError::InvalidReceiptEvidence);
        }
        let facts = canonical_protocol_facts(receipt.canonical_bytes())
            .map_err(|_| DetailError::InvalidReceiptEvidence)?;
        Ok(Self {
            reference: hex(Sha256::digest(receipt.canonical_bytes())),
            asset: facts.asset(),
            amount: facts.amount(),
            fee: facts.fee_charged(),
        })
    }

    /// Returns the receipt reference shared with the feed entry.
    #[must_use]
    pub fn reference(&self) -> &str {
        &self.reference
    }

    /// Returns the exact protocol asset identifier.
    #[must_use]
    pub const fn asset(&self) -> [u8; 32] {
        self.asset
    }

    /// Returns the exact executed amount in base units.
    #[must_use]
    pub const fn amount(&self) -> u128 {
        self.amount
    }

    /// Returns the exact fee charged in base units.
    #[must_use]
    pub const fn fee(&self) -> u128 {
        self.fee
    }
}

/// Checkpoint reference accepted only at a finalised verification level.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FinalityReference {
    checkpoint_id: [u8; 32],
    level: Level,
}

impl FinalityReference {
    /// Creates a reference carried by the verified agent boundary.
    ///
    /// # Errors
    ///
    /// Refuses zero identifiers and levels below checkpoint finality.
    pub fn new(checkpoint_id: [u8; 32], level: Level) -> Result<Self, DetailError> {
        if checkpoint_id == [0; 32] || level < Level::CheckpointFinalised {
            Err(DetailError::InvalidFinalityEvidence)
        } else {
            Ok(Self {
                checkpoint_id,
                level,
            })
        }
    }

    /// Returns the verified checkpoint identifier.
    #[must_use]
    pub const fn checkpoint_id(self) -> [u8; 32] {
        self.checkpoint_id
    }

    /// Returns the achieved verification level.
    #[must_use]
    pub const fn level(self) -> Level {
        self.level
    }
}

/// Presentation state of one fixed timeline stage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StageState {
    Complete,
    Current,
    Upcoming,
    Failed,
}

/// One stage in the activity's journey-specific timeline.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DetailStage {
    label: &'static str,
    state: StageState,
}

impl DetailStage {
    /// Returns the copy-catalog status label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        self.label
    }

    /// Returns how the stage is presented now.
    #[must_use]
    pub const fn state(self) -> StageState {
        self.state
    }
}

/// Kind of independently reachable evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvidenceKind {
    Receipt,
    Checkpoint,
}

/// One signed-out explorer link shown under Technical details.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceLink {
    kind: EvidenceKind,
    reference: String,
    path: String,
    level: Level,
}

impl EvidenceLink {
    /// Returns the evidence class.
    #[must_use]
    pub const fn kind(&self) -> EvidenceKind {
        self.kind
    }

    /// Returns the exact evidence identifier.
    #[must_use]
    pub fn reference(&self) -> &str {
        &self.reference
    }

    /// Returns the public, authentication-free explorer path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns only the verification level carried by the source entry.
    #[must_use]
    pub const fn level(&self) -> Level {
        self.level
    }
}

/// Complete detail for one principal-scoped feed entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntryDetail {
    entry_id: ActivityEntryId,
    kind: ActivityKind,
    status: ActivityStatus,
    sentence: &'static str,
    stages: Vec<DetailStage>,
    actuals: Vec<ReceiptActual>,
    evidence: Vec<EvidenceLink>,
    refusal_money_left: Option<bool>,
}

impl EntryDetail {
    /// Assembles detail from the feed's current receipt-gated entry and the
    /// canonical receipts that produced its economic facts.
    ///
    /// # Errors
    ///
    /// Refuses missing, duplicate or unrelated receipt material, missing
    /// checkpoint evidence for finalised entries, and unjustified finality.
    pub fn assemble(
        entry: &ActivityEntry,
        actuals: Vec<ReceiptActual>,
        finality: Option<FinalityReference>,
    ) -> Result<Self, DetailError> {
        let expected: BTreeSet<&str> = entry
            .receipts()
            .iter()
            .map(super::ReceiptEvidence::reference)
            .collect();
        let supplied: BTreeSet<&str> = actuals.iter().map(ReceiptActual::reference).collect();
        if supplied.len() != actuals.len() {
            return Err(DetailError::DuplicateReceiptMaterial);
        }
        if expected != supplied {
            return Err(DetailError::ReceiptMaterialMismatch);
        }

        let source_finalised = entry
            .receipts()
            .iter()
            .any(|receipt| receipt.level() >= Level::CheckpointFinalised)
            || entry.status() == ActivityStatus::DoneFinalised;
        if source_finalised != finality.is_some() {
            return Err(if source_finalised {
                DetailError::MissingFinalityEvidence
            } else {
                DetailError::UnjustifiedFinalityEvidence
            });
        }

        let status = if source_finalised && entry.status() == ActivityStatus::Done {
            ActivityStatus::DoneFinalised
        } else {
            entry.status()
        };
        let mut evidence =
            Vec::with_capacity(entry.receipts().len() + usize::from(finality.is_some()));
        for receipt in entry.receipts() {
            evidence.push(EvidenceLink {
                kind: EvidenceKind::Receipt,
                reference: receipt.reference().to_owned(),
                path: format!("/explorer/receipts/{}", receipt.reference()),
                level: receipt.level(),
            });
        }
        if let Some(finality) = finality {
            let reference = hex(finality.checkpoint_id());
            evidence.push(EvidenceLink {
                kind: EvidenceKind::Checkpoint,
                path: format!("/explorer/checkpoints/{reference}"),
                reference,
                level: finality.level(),
            });
        }
        let refusal_money_left = match status {
            ActivityStatus::DidntGoThrough { money_left } => Some(money_left),
            _ => None,
        };
        Ok(Self {
            entry_id: entry.entry_id().clone(),
            kind: entry.kind(),
            status,
            sentence: sentence(entry.kind(), status),
            stages: timeline(entry.kind(), status),
            actuals,
            evidence,
            refusal_money_left,
        })
    }

    /// Returns the stable feed identifier.
    #[must_use]
    pub const fn entry_id(&self) -> &ActivityEntryId {
        &self.entry_id
    }

    /// Returns the activity class.
    #[must_use]
    pub const fn kind(&self) -> ActivityKind {
        self.kind
    }

    /// Returns the receipt-gated presentation status.
    #[must_use]
    pub const fn status(&self) -> ActivityStatus {
        self.status
    }

    /// Returns the one-sentence plain description.
    #[must_use]
    pub const fn sentence(&self) -> &'static str {
        self.sentence
    }

    /// Returns the journey-specific staged timeline.
    #[must_use]
    pub fn stages(&self) -> &[DetailStage] {
        &self.stages
    }

    /// Returns exact amounts and fees decoded from verified receipts.
    #[must_use]
    pub fn actuals(&self) -> &[ReceiptActual] {
        &self.actuals
    }

    /// Returns every public receipt and checkpoint link.
    #[must_use]
    pub fn evidence(&self) -> &[EvidenceLink] {
        &self.evidence
    }

    /// Returns whether a refused activity moved money, when applicable.
    #[must_use]
    pub const fn refusal_money_left(&self) -> Option<bool> {
        self.refusal_money_left
    }
}

fn sentence(kind: ActivityKind, status: ActivityStatus) -> &'static str {
    if let ActivityStatus::DidntGoThrough { money_left } = status {
        return if money_left {
            "This did not finish, and money had already left the account."
        } else {
            "This did not finish, and no money left the account."
        };
    }
    let complete = matches!(
        status,
        ActivityStatus::Done
            | ActivityStatus::DoneFinalised
            | ActivityStatus::Deposit(DepositStage::Done)
            | ActivityStatus::Withdrawal(WithdrawalStage::PaidOut)
    );
    match (kind, complete) {
        (ActivityKind::Deposit, true) => "Money was added to your LayerX balance.",
        (ActivityKind::Deposit, false) => "Your added money is still in progress.",
        (ActivityKind::Withdrawal, true) => "Money was paid out to your wallet.",
        (ActivityKind::Withdrawal, false) => "Your payout is still in progress.",
        (ActivityKind::Movement, true) => "Money moved successfully.",
        (ActivityKind::Movement, false) => "Your money movement is still in progress.",
        (ActivityKind::AgentAction, true) => "Your agent change was completed.",
        (ActivityKind::AgentAction, false) => "Your agent change is still in progress.",
        (ActivityKind::Approval, true) => "The approved activity was completed.",
        (ActivityKind::Approval, false) => "This approval is still awaiting an outcome.",
        (ActivityKind::Security, true) => "Your security change was completed.",
        (ActivityKind::Security, false) => "Your security change is still in progress.",
    }
}

fn timeline(kind: ActivityKind, status: ActivityStatus) -> Vec<DetailStage> {
    let labels: &[&str] = match kind {
        ActivityKind::Deposit => &[
            "Waiting for wallet",
            "Confirming on Paxeer",
            "Crediting",
            "Done",
        ],
        ActivityKind::Withdrawal => &[
            "Processing",
            "Waiting for settlement",
            "Ready to claim",
            "Paid out",
        ],
        ActivityKind::Movement | ActivityKind::AgentAction => {
            &["Getting ready", "Sending", "Processing", "Done"]
        }
        ActivityKind::Approval => &["Waiting for you", "Processing", "Done"],
        ActivityKind::Security => &["Processing", "Done"],
    };
    let failed = matches!(status, ActivityStatus::DidntGoThrough { .. });
    let terminal = matches!(
        status,
        ActivityStatus::Done
            | ActivityStatus::DoneFinalised
            | ActivityStatus::Deposit(DepositStage::Done)
            | ActivityStatus::Withdrawal(WithdrawalStage::PaidOut)
    );
    let current = current_stage(kind, status).min(labels.len().saturating_sub(1));
    labels
        .iter()
        .enumerate()
        .map(|(index, label)| DetailStage {
            label,
            state: if terminal || index < current {
                StageState::Complete
            } else if failed && index == current {
                StageState::Failed
            } else if index == current {
                StageState::Current
            } else {
                StageState::Upcoming
            },
        })
        .collect()
}

const fn current_stage(kind: ActivityKind, status: ActivityStatus) -> usize {
    match status {
        ActivityStatus::Deposit(DepositStage::WaitingForWallet)
        | ActivityStatus::Withdrawal(WithdrawalStage::Processing)
        | ActivityStatus::GettingReady
        | ActivityStatus::WaitingForYou => 0,
        ActivityStatus::Deposit(DepositStage::ConfirmingOnPaxeer)
        | ActivityStatus::Withdrawal(WithdrawalStage::WaitingForSettlement)
        | ActivityStatus::Sending => 1,
        ActivityStatus::Deposit(DepositStage::Crediting)
        | ActivityStatus::Withdrawal(WithdrawalStage::ReadyToClaim) => 2,
        ActivityStatus::Deposit(DepositStage::Done)
        | ActivityStatus::Withdrawal(WithdrawalStage::PaidOut) => 3,
        ActivityStatus::Processing | ActivityStatus::StillChecking => match kind {
            ActivityKind::Approval | ActivityKind::Security => 1,
            _ => 2,
        },
        ActivityStatus::Done | ActivityStatus::DoneFinalised => match kind {
            ActivityKind::Approval => 2,
            ActivityKind::Security => 1,
            _ => 3,
        },
        ActivityStatus::DidntGoThrough { .. } => match kind {
            ActivityKind::Approval => 1,
            ActivityKind::Security => 0,
            _ => 2,
        },
    }
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    let bytes = bytes.as_ref();
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

/// Detail assembly failures. No error returns a partially trusted detail.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DetailError {
    InvalidReceiptEvidence,
    DuplicateReceiptMaterial,
    ReceiptMaterialMismatch,
    InvalidFinalityEvidence,
    MissingFinalityEvidence,
    UnjustifiedFinalityEvidence,
}

impl Display for DetailError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidReceiptEvidence => formatter.write_str("receipt evidence is invalid"),
            Self::DuplicateReceiptMaterial => formatter.write_str("receipt material is duplicated"),
            Self::ReceiptMaterialMismatch => {
                formatter.write_str("receipt material does not match the activity entry")
            }
            Self::InvalidFinalityEvidence => {
                formatter.write_str("checkpoint evidence is not finalised")
            }
            Self::MissingFinalityEvidence => {
                formatter.write_str("finalised activity is missing checkpoint evidence")
            }
            Self::UnjustifiedFinalityEvidence => {
                formatter.write_str("checkpoint evidence would raise the activity level")
            }
        }
    }
}

impl std::error::Error for DetailError {}
