//! Code generated from platform/sdk/generators/receipt.kvx. DO NOT EDIT.

pub const PROGRAMS_MODULE_ID: u16 = 9;
pub const PROGRAM_OUTCOME_TAGS: [u32; 3] = [0x50524731, 0x50524732, 0x50524733];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReceiptFailureCode {
    Decode,
    CanonicalEncoding,
    ReceiptShape,
    MissingSignature,
    ProtocolVersion,
    ResultCode,
    Operation,
    ActivityId,
    GlobalSequence,
    ModuleId,
    ModuleVersion,
    Timestamp,
    BatchId,
    Asset,
    PreviousStateRoot,
    ResultingStateRoot,
    DebitBalance,
    CreditBalance,
    ProgramOutcome,
    SequencerSignature,
}

impl ReceiptFailureCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Decode => "decode",
            Self::CanonicalEncoding => "canonical-encoding",
            Self::ReceiptShape => "receipt-shape",
            Self::MissingSignature => "missing-signature",
            Self::ProtocolVersion => "protocol-version",
            Self::ResultCode => "result-code",
            Self::Operation => "operation",
            Self::ActivityId => "activity-id",
            Self::GlobalSequence => "global-sequence",
            Self::ModuleId => "module-id",
            Self::ModuleVersion => "module-version",
            Self::Timestamp => "timestamp",
            Self::BatchId => "batch-id",
            Self::Asset => "asset",
            Self::PreviousStateRoot => "previous-state-root",
            Self::ResultingStateRoot => "resulting-state-root",
            Self::DebitBalance => "debit-balance",
            Self::CreditBalance => "credit-balance",
            Self::ProgramOutcome => "program-outcome",
            Self::SequencerSignature => "sequencer-signature",
        }
    }
}

pub const REQUIRED_NONZERO_CHECKS: &[ReceiptFailureCode] = &[
    ReceiptFailureCode::GlobalSequence,
    ReceiptFailureCode::ModuleId,
    ReceiptFailureCode::ModuleVersion,
    ReceiptFailureCode::Timestamp,
    ReceiptFailureCode::ActivityId,
    ReceiptFailureCode::ResultingStateRoot,
];
