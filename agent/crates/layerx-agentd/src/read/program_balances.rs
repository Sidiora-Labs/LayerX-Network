//! Receipt-bound program custody reads from the production Programs adapter.

use layerx_programs::{ProgramId, ProgramLifecycle, Registry, VerifiedProgramBalanceRead};
use layerx_programs_protocol_adapter::{read_program_state, ProtocolAdapterError};

/// Node-injected production read route. The opaque context is never created by
/// the agent: its owner must be the live Programs context of the node serving
/// the same canonical head and receipts used by other agent reads.
pub struct ProtocolProgramBalanceReader {
    context: core::ptr::NonNull<core::ffi::c_void>,
    registry: Registry,
    staleness_limit: u64,
}

impl ProtocolProgramBalanceReader {
    /// Binds the agent route to a node-owned live Programs read context.
    ///
    /// # Errors
    ///
    /// Refuses a null context or a zero staleness limit as a non-canonical
    /// view.
    ///
    /// # Safety
    ///
    /// `context` must remain the synchronous live node Programs context for
    /// the full lifetime of this reader and must not refer to an independent
    /// agent-local protocol state.
    #[allow(unsafe_code)]
    pub unsafe fn bind(
        context: *mut core::ffi::c_void,
        registry: Registry,
        staleness_limit: u64,
    ) -> Result<Self, ProtocolAdapterError> {
        let context =
            core::ptr::NonNull::new(context).ok_or(ProtocolAdapterError::NonCanonicalView)?;
        if staleness_limit == 0 {
            return Err(ProtocolAdapterError::NonCanonicalView);
        }
        Ok(Self {
            context,
            registry,
            staleness_limit,
        })
    }

    /// Serves one current, receipt-bound program balance read.
    ///
    /// # Errors
    ///
    /// Refuses unavailable, stale, historical or identity-mismatched protocol
    /// evidence and never substitutes a cached or caller-supplied balance.
    ///
    /// # Safety
    ///
    /// The bound context must still be the live node Programs context for the
    /// duration of this synchronous call.
    #[allow(unsafe_code)]
    pub unsafe fn read(
        &mut self,
        program: ProgramId,
        receipt_digest: [u8; 32],
        now: u64,
    ) -> Result<ProgramBalanceRead, ProtocolAdapterError> {
        let read = unsafe {
            program_balances_from_protocol(
                self.context.as_ptr(),
                &mut self.registry,
                program,
                receipt_digest,
                now,
                self.staleness_limit,
            )
        }?;
        if read.program != program.bytes() || now > read.freshness.valid_through {
            return Err(ProtocolAdapterError::NonCanonicalView);
        }
        Ok(read)
    }
}

/// One real derived account rendered for an agent without losing its asset or
/// frozen-state semantics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramValueBalance {
    pub account: [u8; 32],
    pub asset: [u8; 32],
    pub amount: u128,
    pub frozen: bool,
}

/// Current-head evidence attached to the complete program account list.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgramBalanceFreshness {
    pub observed_sequence: u64,
    pub observed_at: u64,
    pub receipt_digest: [u8; 32],
    pub state_root: [u8; 32],
    pub valid_through: u64,
}

/// Agent-facing custody surface. It can only be constructed from the private,
/// proof-verified registry wrapper, never from caller-supplied balances.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramBalanceRead {
    pub program: [u8; 32],
    pub lifecycle: ProgramLifecycle,
    pub accounts: Vec<ProgramValueBalance>,
    pub freshness: ProgramBalanceFreshness,
}

pub(super) fn program_balances(
    read: &VerifiedProgramBalanceRead,
    staleness_limit: u64,
) -> Result<ProgramBalanceRead, ProtocolAdapterError> {
    let valid_through = read
        .freshness()
        .observed_at
        .checked_add(staleness_limit)
        .ok_or(ProtocolAdapterError::NonCanonicalView)?;
    Ok(ProgramBalanceRead {
        program: read.program().bytes(),
        lifecycle: read.lifecycle(),
        accounts: read
            .value_accounts()
            .iter()
            .map(|account| ProgramValueBalance {
                account: account.account_id,
                asset: account.asset_id,
                amount: account.balance,
                frozen: account.frozen,
            })
            .collect(),
        freshness: ProgramBalanceFreshness {
            observed_sequence: read.freshness().observed_sequence,
            observed_at: read.freshness().observed_at,
            receipt_digest: read.receipt_digest(),
            state_root: read.state_root(),
            valid_through,
        },
    })
}

/// Reads the production C Programs/account-state adapter and immediately
/// projects its opaque verified result onto the agent surface.
///
/// # Errors
///
/// Propagates every adapter refusal from the underlying state read and the
/// freshness-window projection unchanged.
///
/// # Safety
///
/// `context` must be the live read-only module context supplied by the node
/// and remain valid for this synchronous call.
#[allow(unsafe_code)]
pub unsafe fn program_balances_from_protocol(
    context: *mut core::ffi::c_void,
    registry: &mut Registry,
    program: ProgramId,
    receipt_digest: [u8; 32],
    now: u64,
    staleness_limit: u64,
) -> Result<ProgramBalanceRead, ProtocolAdapterError> {
    let read = unsafe {
        read_program_state(
            context,
            registry,
            program,
            receipt_digest,
            now,
            staleness_limit,
        )
    }?;
    program_balances(&read.into_balances(), staleness_limit)
}
