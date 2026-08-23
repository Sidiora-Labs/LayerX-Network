//! Public program-registry projection built only from receipt-verified reads.

use layerx_programs::{
    ProgramId, ProgramLifecycle, ReadFreshness, SourceStatus, UpgradePolicy, VerifiedRegistryRead,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExplorerProgramVersion {
    pub number: u32,
    pub code_hash: [u8; 32],
    pub abi_version: u16,
    pub source: SourceStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExplorerProgram {
    pub identifier: [u8; 32],
    pub upgrade_policy: UpgradePolicy,
    pub lifecycle: ProgramLifecycle,
    pub versions: Vec<ExplorerProgramVersion>,
    pub observed_sequence: u64,
    pub observed_at: u64,
    pub receipt_digest: [u8; 32],
}

impl From<VerifiedRegistryRead> for ExplorerProgram {
    fn from(read: VerifiedRegistryRead) -> Self {
        Self {
            identifier: read.entry.program.bytes(),
            upgrade_policy: read.entry.upgrade_policy,
            lifecycle: read.entry.lifecycle,
            versions: read
                .entry
                .versions
                .into_iter()
                .map(|version| ExplorerProgramVersion {
                    number: version.number,
                    code_hash: version.code_hash,
                    abi_version: version.abi_version,
                    source: version.source,
                })
                .collect(),
            observed_sequence: read.freshness.observed_sequence,
            observed_at: read.freshness.observed_at,
            receipt_digest: read.receipt_digest,
        }
    }
}

/// The closed taxonomy of typed program-call failures the explorer surfaces.
/// It mirrors the agent-layer failure taxonomy so the explorer names a refusal
/// exactly as the caller received it, never as an opaque error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExplorerCallFailureClass {
    UnknownProgram,
    Reentrancy,
    DepthExceeded,
    FanoutExceeded,
    GuestRefused,
    Authority,
    Resource,
    Response,
    Fault,
}

/// The typed outcome of one program call as it appears in the public index.
/// A completed call carries the callee's non-negative code and a digest of its
/// response; a failed call carries its typed class and the receipt's own result
/// code. The explorer never renders a body it did not receive under a receipt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExplorerCallOutcome {
    Completed {
        code: i32,
        response_len: usize,
        response_digest: [u8; 32],
    },
    Failed {
        class: ExplorerCallFailureClass,
        result_code: i32,
    },
}

/// One receipt-verified program call ready for public projection. The evidence
/// fields are supplied only after the backing receipt has been verified and its
/// freshness observed, so this type cannot be built from an unverified read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedProgramCall {
    pub program: ProgramId,
    pub caller: [u8; 32],
    pub calldata_digest: [u8; 32],
    pub declared_fuel: u64,
    pub declared_fee_limit: u128,
    pub requested_capabilities: Vec<u8>,
    pub outcome: ExplorerCallOutcome,
    pub result_code: i32,
    pub receipt_digest: [u8; 32],
    pub freshness: ReadFreshness,
}

/// Public program-call projection carrying its freshness statement. Every field
/// is derived from receipt-verified evidence; the observed sequence and time are
/// retained so a reader can judge how current the row is.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExplorerProgramCall {
    pub program: [u8; 32],
    pub caller: [u8; 32],
    pub calldata_digest: [u8; 32],
    pub declared_fuel: u64,
    pub declared_fee_limit: u128,
    pub requested_capabilities: Vec<u8>,
    pub outcome: ExplorerCallOutcome,
    pub result_code: i32,
    pub observed_sequence: u64,
    pub observed_at: u64,
    pub receipt_digest: [u8; 32],
}

/// Refusal to project a program call that is not receipt-verified or is not
/// internally consistent with its own outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExplorerProgramError {
    /// The call carried no receipt digest or no observed freshness.
    UnverifiedCall,
    /// The typed outcome disagreed with the receipt's own result code.
    OutcomeResultMismatch,
}

impl ExplorerProgramCall {
    /// Projects one receipt-verified program call into the public index.
    ///
    /// # Errors
    ///
    /// Returns [`ExplorerProgramError::UnverifiedCall`] when the receipt digest
    /// is absent or freshness was never observed, and
    /// [`ExplorerProgramError::OutcomeResultMismatch`] when the typed outcome
    /// contradicts the receipt's own result code, so a completed call is never
    /// shown against a failed receipt nor a failure against a successful one.
    pub fn from_verified(read: VerifiedProgramCall) -> Result<Self, ExplorerProgramError> {
        if read.receipt_digest == [0; 32]
            || read.freshness.observed_sequence == 0
            || read.freshness.observed_at == 0
        {
            return Err(ExplorerProgramError::UnverifiedCall);
        }
        match read.outcome {
            ExplorerCallOutcome::Completed { code, .. } => {
                if code < 0 || code != read.result_code {
                    return Err(ExplorerProgramError::OutcomeResultMismatch);
                }
            }
            ExplorerCallOutcome::Failed { result_code, .. } => {
                if result_code >= 0 || result_code != read.result_code {
                    return Err(ExplorerProgramError::OutcomeResultMismatch);
                }
            }
        }
        Ok(Self {
            program: read.program.bytes(),
            caller: read.caller,
            calldata_digest: read.calldata_digest,
            declared_fuel: read.declared_fuel,
            declared_fee_limit: read.declared_fee_limit,
            requested_capabilities: read.requested_capabilities,
            outcome: read.outcome,
            result_code: read.result_code,
            observed_sequence: read.freshness.observed_sequence,
            observed_at: read.freshness.observed_at,
            receipt_digest: read.receipt_digest,
        })
    }
}

#[cfg(test)]
mod program_call_tests {
    use super::{
        ExplorerCallFailureClass, ExplorerCallOutcome, ExplorerProgramCall, ExplorerProgramError,
        ProgramId, ReadFreshness, VerifiedProgramCall,
    };

    fn program() -> ProgramId {
        match ProgramId::new([0x11; 32]) {
            Ok(program) => program,
            Err(_) => panic!("nonzero program identifier rejected"),
        }
    }

    fn verified(outcome: ExplorerCallOutcome, result_code: i32) -> VerifiedProgramCall {
        VerifiedProgramCall {
            program: program(),
            caller: [0x22; 32],
            calldata_digest: [0x33; 32],
            declared_fuel: 1000,
            declared_fee_limit: 250,
            requested_capabilities: vec![1, 3],
            outcome,
            result_code,
            receipt_digest: [0x44; 32],
            freshness: ReadFreshness {
                observed_sequence: 7,
                observed_at: 1_700,
            },
        }
    }

    #[test]
    fn completed_call_projects_with_freshness() {
        let read = verified(
            ExplorerCallOutcome::Completed {
                code: 0,
                response_len: 3,
                response_digest: [0x55; 32],
            },
            0,
        );
        let Ok(projected) = ExplorerProgramCall::from_verified(read) else {
            panic!("verified completed call rejected");
        };
        assert_eq!(projected.observed_sequence, 7);
        assert_eq!(projected.observed_at, 1_700);
        assert_eq!(projected.result_code, 0);
        assert_eq!(projected.requested_capabilities, vec![1, 3]);
    }

    #[test]
    fn failed_call_surfaces_typed_class() {
        let read = verified(
            ExplorerCallOutcome::Failed {
                class: ExplorerCallFailureClass::GuestRefused,
                result_code: -736,
            },
            -736,
        );
        let Ok(projected) = ExplorerProgramCall::from_verified(read) else {
            panic!("verified failed call rejected");
        };
        assert_eq!(
            projected.outcome,
            ExplorerCallOutcome::Failed {
                class: ExplorerCallFailureClass::GuestRefused,
                result_code: -736,
            }
        );
    }

    #[test]
    fn unverified_call_is_refused() {
        let mut read = verified(
            ExplorerCallOutcome::Completed {
                code: 0,
                response_len: 0,
                response_digest: [0; 32],
            },
            0,
        );
        read.receipt_digest = [0; 32];
        assert_eq!(
            ExplorerProgramCall::from_verified(read),
            Err(ExplorerProgramError::UnverifiedCall)
        );
    }

    #[test]
    fn outcome_disagreeing_with_receipt_is_refused() {
        let read = verified(
            ExplorerCallOutcome::Completed {
                code: 0,
                response_len: 0,
                response_digest: [0; 32],
            },
            -1,
        );
        assert_eq!(
            ExplorerProgramCall::from_verified(read),
            Err(ExplorerProgramError::OutcomeResultMismatch)
        );
    }
}
