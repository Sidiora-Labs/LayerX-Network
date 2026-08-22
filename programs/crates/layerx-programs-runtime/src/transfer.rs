//! The sole monetary exit from the programs runtime. Guest execution can only
//! produce typed 402LXP requests; this module binds those requests to the
//! invocation authority, submits one atomic set to the kernel transfer
//! primitive, and returns success only with a verified standard receipt.

use core::fmt::{self, Display};
use std::collections::BTreeMap;

use crate::abi::{
    AbiEffects, AuthorizationContext, CallFrameId, CapabilitySet, TransferRequest,
    MAX_EVENT_DATA_BYTES, MAX_EVENT_TOPIC_BYTES,
};
use crate::calls::CallGraph;
use crate::storage::{PrincipalId, ProgramId};

const SET_DOMAIN: &[u8] = b"LayerX/programs/402LXP/transfer-set/v1\0";
const MAX_TRANSFER_LEGS: usize = 256;

/// Authority fixed by the invoking protocol activity. No constructor accepts
/// a balance handle, account store, or mutation callback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransferCapability {
    program: ProgramId,
    principal: PrincipalId,
    invocation_authority: [u8; 32],
    root_frame: CallFrameId,
    root_capabilities: CapabilitySet,
}

impl TransferCapability {
    /// Binds one programs invocation to its protocol-authenticated authority.
    ///
    /// # Errors
    ///
    /// Refuses the reserved zero authority digest.
    pub(crate) fn from_root_authorization(
        program: ProgramId,
        authorization: &AuthorizationContext,
        invocation_authority: [u8; 32],
    ) -> Result<Self, TransferLawError> {
        if invocation_authority == [0; 32] || authorization.frame() != CallFrameId::root() {
            return Err(TransferLawError::UnverifiedAuthority);
        }
        Ok(Self {
            program,
            principal: authorization.principal(),
            invocation_authority,
            root_frame: authorization.frame(),
            root_capabilities: authorization.capabilities().clone(),
        })
    }

    /// Closes successful guest effects into the only form accepted by the
    /// kernel monetary boundary.
    ///
    /// # Errors
    ///
    /// Aborts on an empty or oversized set, invalid leg, authority mismatch,
    /// or arithmetic overflow.
    pub fn authorize(&self, effects: &AbiEffects) -> Result<AtomicTransferSet, TransferLawError> {
        if effects.transfers.is_empty() || effects.transfers.len() > MAX_TRANSFER_LEGS {
            return Err(TransferLawError::InvalidTransferSet);
        }
        let frames = self.authorized_frames(effects)?;
        let mut total = 0u128;
        let mut canonical = Vec::with_capacity(
            SET_DOMAIN.len()
                + 112
                + effects.calls.len().saturating_mul(166)
                + effects.transfers.len().saturating_mul(112),
        );
        canonical.extend_from_slice(SET_DOMAIN);
        canonical.extend_from_slice(&self.program.bytes());
        canonical.extend_from_slice(&self.principal.bytes());
        canonical.extend_from_slice(&self.invocation_authority);
        let (root_path, root_depth) = self.root_frame.canonical_bytes();
        canonical.extend_from_slice(&root_path);
        canonical.push(root_depth);
        let events = effects
            .canonical_program_event_envelope()
            .map_err(|_| TransferLawError::InvalidTransferSet)?;
        let event_length =
            u32::try_from(events.len()).map_err(|_| TransferLawError::InvalidTransferSet)?;
        canonical.extend_from_slice(&event_length.to_be_bytes());
        canonical.extend_from_slice(&events);
        canonical.extend_from_slice(&(effects.calls.len() as u64).to_be_bytes());
        for call in &effects.calls {
            canonical.extend_from_slice(&call.caller.bytes());
            canonical.extend_from_slice(&call.callee.bytes());
            canonical.extend_from_slice(&call.principal.bytes());
            let (caller_path, caller_depth) = call.caller_frame.canonical_bytes();
            let (callee_path, callee_depth) = call.callee_frame.canonical_bytes();
            canonical.extend_from_slice(&caller_path);
            canonical.push(caller_depth);
            canonical.extend_from_slice(&callee_path);
            canonical.push(callee_depth);
            let grants = call.capabilities.canonical_encoding();
            let grant_length =
                u32::try_from(grants.len()).map_err(|_| TransferLawError::InvariantViolation)?;
            canonical.extend_from_slice(&grant_length.to_be_bytes());
            canonical.extend_from_slice(&grants);
        }
        canonical.extend_from_slice(&(effects.transfers.len() as u64).to_be_bytes());
        let mut frame_totals = BTreeMap::new();
        let mut graph_totals = BTreeMap::new();
        for transfer in &effects.transfers {
            let Some((program, capabilities)) = frames.get(&transfer.frame) else {
                return Err(TransferLawError::InvariantViolation);
            };
            if transfer.principal != self.principal || transfer.program != *program {
                return Err(TransferLawError::InvariantViolation);
            }
            if transfer.asset == [0; 32] || transfer.to == [0; 32] || transfer.amount == 0 {
                return Err(TransferLawError::InvalidTransfer);
            }
            total = total
                .checked_add(transfer.amount)
                .ok_or(TransferLawError::AmountOverflow)?;
            let frame_key = (transfer.frame, transfer.asset, transfer.to);
            let frame_amount = frame_totals
                .get(&frame_key)
                .copied()
                .unwrap_or(0_u128)
                .checked_add(transfer.amount)
                .ok_or(TransferLawError::AmountOverflow)?;
            if !capabilities.permits_transfer(transfer.asset, transfer.to, frame_amount) {
                return Err(TransferLawError::CapabilityEscalation);
            }
            frame_totals.insert(frame_key, frame_amount);
            let graph_key = (transfer.asset, transfer.to);
            let graph_amount = graph_totals
                .get(&graph_key)
                .copied()
                .unwrap_or(0_u128)
                .checked_add(transfer.amount)
                .ok_or(TransferLawError::AmountOverflow)?;
            if !self
                .root_capabilities
                .permits_transfer(transfer.asset, transfer.to, graph_amount)
            {
                return Err(TransferLawError::CapabilityEscalation);
            }
            graph_totals.insert(graph_key, graph_amount);
            let (path, depth) = transfer.frame.canonical_bytes();
            canonical.extend_from_slice(&path);
            canonical.push(depth);
            canonical.extend_from_slice(&transfer.asset);
            canonical.extend_from_slice(&transfer.to);
            canonical.extend_from_slice(&transfer.amount.to_be_bytes());
            canonical.extend_from_slice(&transfer.program.bytes());
        }
        let kernel_canonical = canonical_kernel_legs(&effects.transfers)?;
        Ok(AtomicTransferSet {
            program: self.program,
            principal: self.principal,
            invocation_authority: self.invocation_authority,
            authorization_evidence: canonical,
            kernel_canonical,
            total_amount: total,
            legs: effects.transfers.clone(),
        })
    }

    pub(crate) fn authorize_for_graph(
        &self,
        effects: &AbiEffects,
        graph: &CallGraph,
    ) -> Result<AtomicTransferSet, TransferLawError> {
        if graph.principal() != self.principal {
            return Err(TransferLawError::InvariantViolation);
        }
        for call in &effects.calls {
            if !graph.edges().iter().any(|edge| {
                edge.caller() == call.caller
                    && edge.callee() == call.callee
                    && edge.principal() == call.principal
                    && edge.caller_frame() == call.caller_frame
                    && edge.callee_frame() == call.callee_frame
            }) {
                return Err(TransferLawError::InvariantViolation);
            }
        }
        self.authorize(effects)
    }

    fn authorized_frames(
        &self,
        effects: &AbiEffects,
    ) -> Result<BTreeMap<CallFrameId, (ProgramId, CapabilitySet)>, TransferLawError> {
        let mut frames = BTreeMap::new();
        frames.insert(
            self.root_frame,
            (self.program, self.root_capabilities.clone()),
        );
        for call in &effects.calls {
            if call.principal != self.principal || call.caller_frame == call.callee_frame {
                return Err(TransferLawError::InvariantViolation);
            }
            let Some((caller, parent_capabilities)) = frames.get(&call.caller_frame) else {
                return Err(TransferLawError::InvariantViolation);
            };
            if *caller != call.caller || !parent_capabilities.contains_narrowed(&call.capabilities)
            {
                return Err(TransferLawError::CapabilityEscalation);
            }
            if frames
                .insert(call.callee_frame, (call.callee, call.capabilities.clone()))
                .is_some()
            {
                return Err(TransferLawError::InvariantViolation);
            }
        }
        Ok(frames)
    }

    /// Applies all requested monetary effects atomically through the kernel's
    /// existing 402LXP primitive and verifies the exact sealed transfer root.
    ///
    /// # Errors
    ///
    /// Returns a typed law, kernel, or commitment refusal. No partially applied
    /// state or unverified success can be returned through this API.
    pub(crate) fn settle(
        &self,
        effects: &AbiEffects,
        kernel: &mut impl KernelTransferPrimitive,
    ) -> Result<VerifiedProgramSettlement, TransferLawError> {
        let transfers = self.authorize(effects)?;
        self.settle_authorized_set(&transfers, kernel)
    }

    pub(crate) fn settle_authorized_set(
        &self,
        transfers: &AtomicTransferSet,
        kernel: &mut impl KernelTransferPrimitive,
    ) -> Result<VerifiedProgramSettlement, TransferLawError> {
        let evidence = kernel.apply_and_verify_402lxp_set(transfers)?;
        kernel.verify_402lxp_transfer_set_root(&transfers, &evidence)?;
        if evidence.transfer_set_root == [0; 32]
            || evidence.leg_count != transfers.legs.len()
            || evidence.total_amount != transfers.total_amount
        {
            return Err(TransferLawError::ReceiptMismatch);
        }
        Ok(VerifiedProgramSettlement {
            transfer_set_root: evidence.transfer_set_root,
            leg_count: evidence.leg_count,
            total_amount: evidence.total_amount,
        })
    }
}

/// Immutable atomic request passed to the core transfer module.
#[derive(Debug, Eq, PartialEq)]
pub struct AtomicTransferSet {
    program: ProgramId,
    principal: PrincipalId,
    invocation_authority: [u8; 32],
    authorization_evidence: Vec<u8>,
    kernel_canonical: Vec<u8>,
    total_amount: u128,
    legs: Vec<TransferRequest>,
}

impl AtomicTransferSet {
    #[must_use]
    pub const fn program(&self) -> ProgramId {
        self.program
    }
    #[must_use]
    pub const fn principal(&self) -> PrincipalId {
        self.principal
    }
    #[must_use]
    pub const fn invocation_authority(&self) -> [u8; 32] {
        self.invocation_authority
    }
    #[must_use]
    pub fn canonical(&self) -> &[u8] {
        &self.authorization_evidence
    }
    #[must_use]
    pub fn kernel_canonical(&self) -> &[u8] {
        &self.kernel_canonical
    }
    #[must_use]
    pub const fn total_amount(&self) -> u128 {
        self.total_amount
    }
    #[must_use]
    pub fn legs(&self) -> &[TransferRequest] {
        &self.legs
    }

    /// Strictly decodes and validates a persisted authorisation artifact.
    pub(crate) fn canonical_decode(encoded: &[u8]) -> Result<Self, TransferLawError> {
        let mut cursor = TransferCursor::new(encoded);
        if cursor.take(SET_DOMAIN.len())? != SET_DOMAIN {
            return Err(TransferLawError::InvalidTransferSet);
        }
        let program =
            ProgramId::new(cursor.array()?).map_err(|_| TransferLawError::InvalidTransferSet)?;
        let principal =
            PrincipalId::new(cursor.array()?).map_err(|_| TransferLawError::InvalidTransferSet)?;
        let invocation_authority = cursor.array()?;
        if invocation_authority == [0; 32] {
            return Err(TransferLawError::UnverifiedAuthority);
        }
        let root_frame = frame_from_cursor(&mut cursor)?;
        let event_length = u32::from_be_bytes(cursor.array()?) as usize;
        let events = parse_event_envelope(cursor.take(event_length)?)?;
        let mut canonical = Vec::with_capacity(encoded.len());
        canonical.extend_from_slice(SET_DOMAIN);
        canonical.extend_from_slice(&program.bytes());
        canonical.extend_from_slice(&principal.bytes());
        canonical.extend_from_slice(&invocation_authority);
        let (root_path, root_depth) = root_frame.canonical_bytes();
        canonical.extend_from_slice(&root_path);
        canonical.push(root_depth);
        let canonical_event_length =
            u32::try_from(events.len()).map_err(|_| TransferLawError::InvalidTransferSet)?;
        canonical.extend_from_slice(&canonical_event_length.to_be_bytes());
        canonical.extend_from_slice(&events);
        let call_count = usize::try_from(u64::from_be_bytes(cursor.array()?))
            .map_err(|_| TransferLawError::InvalidTransferSet)?;
        if call_count > crate::DEFAULT_MAX_CALL_GRAPH_EDGES as usize {
            return Err(TransferLawError::InvalidTransferSet);
        }
        canonical.extend_from_slice(&(call_count as u64).to_be_bytes());
        for _ in 0..call_count {
            let caller = ProgramId::new(cursor.array()?)
                .map_err(|_| TransferLawError::InvalidTransferSet)?;
            let callee = ProgramId::new(cursor.array()?)
                .map_err(|_| TransferLawError::InvalidTransferSet)?;
            let call_principal = PrincipalId::new(cursor.array()?)
                .map_err(|_| TransferLawError::InvalidTransferSet)?;
            let caller_frame = frame_from_cursor(&mut cursor)?;
            let callee_frame = frame_from_cursor(&mut cursor)?;
            let grant_length = u32::from_be_bytes(cursor.array()?) as usize;
            let grants = cursor.take(grant_length)?;
            let grants = CapabilitySet::decode_canonical(grants)
                .map_err(|_| TransferLawError::InvalidTransferSet)?;
            canonical.extend_from_slice(&caller.bytes());
            canonical.extend_from_slice(&callee.bytes());
            canonical.extend_from_slice(&call_principal.bytes());
            let (caller_path, caller_depth) = caller_frame.canonical_bytes();
            let (callee_path, callee_depth) = callee_frame.canonical_bytes();
            canonical.extend_from_slice(&caller_path);
            canonical.push(caller_depth);
            canonical.extend_from_slice(&callee_path);
            canonical.push(callee_depth);
            let grants = CapabilitySet::new(grants)
                .map_err(|_| TransferLawError::InvalidTransferSet)?
                .canonical_encoding();
            let canonical_grant_length =
                u32::try_from(grants.len()).map_err(|_| TransferLawError::InvalidTransferSet)?;
            canonical.extend_from_slice(&canonical_grant_length.to_be_bytes());
            canonical.extend_from_slice(&grants);
        }
        let leg_count = usize::try_from(u64::from_be_bytes(cursor.array()?))
            .map_err(|_| TransferLawError::InvalidTransferSet)?;
        if leg_count == 0 || leg_count > MAX_TRANSFER_LEGS {
            return Err(TransferLawError::InvalidTransferSet);
        }
        canonical.extend_from_slice(&(leg_count as u64).to_be_bytes());
        let mut total = 0u128;
        let mut legs = Vec::with_capacity(leg_count);
        for _ in 0..leg_count {
            let frame = frame_from_cursor(&mut cursor)?;
            let asset = cursor.array()?;
            let to = cursor.array()?;
            let amount = u128::from_be_bytes(cursor.array()?);
            let leg_program = ProgramId::new(cursor.array()?)
                .map_err(|_| TransferLawError::InvalidTransferSet)?;
            if asset == [0; 32] || to == [0; 32] || amount == 0 {
                return Err(TransferLawError::InvalidTransfer);
            }
            total = total
                .checked_add(amount)
                .ok_or(TransferLawError::AmountOverflow)?;
            legs.push(TransferRequest {
                program: leg_program,
                principal,
                frame,
                asset,
                to,
                amount,
            });
            let (path, depth) = frame.canonical_bytes();
            canonical.extend_from_slice(&path);
            canonical.push(depth);
            canonical.extend_from_slice(&asset);
            canonical.extend_from_slice(&to);
            canonical.extend_from_slice(&amount.to_be_bytes());
            canonical.extend_from_slice(&leg_program.bytes());
        }
        if !cursor.is_empty() || canonical != encoded {
            return Err(TransferLawError::InvalidTransferSet);
        }
        let kernel_canonical = canonical_kernel_legs(&legs)?;
        Ok(Self {
            program,
            principal,
            invocation_authority,
            authorization_evidence: canonical,
            kernel_canonical,
            total_amount: total,
            legs,
        })
    }
}

fn canonical_kernel_legs(legs: &[TransferRequest]) -> Result<Vec<u8>, TransferLawError> {
    if legs.is_empty() || legs.len() > MAX_TRANSFER_LEGS {
        return Err(TransferLawError::InvalidTransferSet);
    }
    let mut encoded = Vec::with_capacity(legs.len().saturating_mul(115));
    for leg in legs {
        encoded.push(0);
        encoded.extend_from_slice(&leg.principal.bytes());
        encoded.extend_from_slice(&leg.to);
        encoded.extend_from_slice(&leg.asset);
        encoded.extend_from_slice(&leg.amount.to_be_bytes());
        encoded.extend_from_slice(&1u16.to_be_bytes());
    }
    Ok(encoded)
}

fn parse_event_envelope(encoded: &[u8]) -> Result<Vec<u8>, TransferLawError> {
    const DOMAIN: &[u8] = b"LayerX/programs/events/v1\0";
    let mut cursor = TransferCursor::new(encoded);
    if cursor.take(DOMAIN.len())? != DOMAIN {
        return Err(TransferLawError::InvalidTransferSet);
    }
    let count = usize::try_from(u32::from_be_bytes(cursor.array()?))
        .map_err(|_| TransferLawError::InvalidTransferSet)?;
    if count > crate::DEFAULT_MAX_CALL_GRAPH_EDGES as usize {
        return Err(TransferLawError::InvalidTransferSet);
    }
    let mut canonical = DOMAIN.to_vec();
    canonical.extend_from_slice(&(count as u32).to_be_bytes());
    for _ in 0..count {
        let program =
            ProgramId::new(cursor.array()?).map_err(|_| TransferLawError::InvalidTransferSet)?;
        let principal =
            PrincipalId::new(cursor.array()?).map_err(|_| TransferLawError::InvalidTransferSet)?;
        let frame = frame_from_cursor(&mut cursor)?;
        let topic_length = u32::from_be_bytes(cursor.array()?) as usize;
        if topic_length > MAX_EVENT_TOPIC_BYTES {
            return Err(TransferLawError::InvalidTransferSet);
        }
        let topic = cursor.take(topic_length)?;
        let data_length = u32::from_be_bytes(cursor.array()?) as usize;
        if data_length > MAX_EVENT_DATA_BYTES {
            return Err(TransferLawError::InvalidTransferSet);
        }
        let data = cursor.take(data_length)?;
        canonical.extend_from_slice(&program.bytes());
        canonical.extend_from_slice(&principal.bytes());
        let (path, depth) = frame.canonical_bytes();
        canonical.extend_from_slice(&path);
        canonical.push(depth);
        canonical.extend_from_slice(&(topic_length as u32).to_be_bytes());
        canonical.extend_from_slice(topic);
        canonical.extend_from_slice(&(data_length as u32).to_be_bytes());
        canonical.extend_from_slice(data);
    }
    if cursor.is_empty() && canonical == encoded {
        Ok(canonical)
    } else {
        Err(TransferLawError::InvalidTransferSet)
    }
}

fn frame_from_cursor(cursor: &mut TransferCursor<'_>) -> Result<CallFrameId, TransferLawError> {
    let path = cursor.array()?;
    let depth = cursor.take(1)?[0];
    CallFrameId::from_canonical(path, depth).map_err(|_| TransferLawError::InvalidTransferSet)
}

struct TransferCursor<'a> {
    remaining: &'a [u8],
}
impl<'a> TransferCursor<'a> {
    const fn new(remaining: &'a [u8]) -> Self {
        Self { remaining }
    }
    fn take(&mut self, length: usize) -> Result<&'a [u8], TransferLawError> {
        let (value, remaining) = self
            .remaining
            .split_at_checked(length)
            .ok_or(TransferLawError::InvalidTransferSet)?;
        self.remaining = remaining;
        Ok(value)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], TransferLawError> {
        self.take(N)?
            .try_into()
            .map_err(|_| TransferLawError::InvalidTransferSet)
    }
    const fn is_empty(&self) -> bool {
        self.remaining.is_empty()
    }
}

/// The existing kernel transfer-set primitive. It owns all balance mutation,
/// conservation enforcement and atomic rollback. The surrounding C activity
/// owns the standard outer receipt, which is built only after this synchronous
/// transfer commitment has released the held program state.
pub trait KernelTransferPrimitive {
    /// Applies the exact set or none of it and returns its nonzero canonical
    /// transfer root before the surrounding activity receipt is constructed.
    ///
    /// # Errors
    ///
    /// Returns a typed core refusal without exposing partial mutation.
    fn apply_and_verify_402lxp_set(
        &mut self,
        transfers: &AtomicTransferSet,
    ) -> Result<KernelTransferEvidence, TransferLawError>;

    /// Cryptographically verifies that the nonzero transfer-set root is the
    /// commitment to this exact canonical request. The C kernel owns the only
    /// production implementation; the runtime never substitutes a ledger.
    fn verify_402lxp_transfer_set_root(
        &self,
        transfers: &AtomicTransferSet,
        evidence: &KernelTransferEvidence,
    ) -> Result<(), TransferLawError>;
}

/// Raw facts returned by the trusted kernel boundary and strictly checked by
/// [`TransferCapability::settle`] before a verified result can exist.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelTransferEvidence {
    pub transfer_set_root: [u8; 32],
    pub leg_count: usize,
    pub total_amount: u128,
}

/// Kernel-verified transfer commitment awaiting binding into the one outer
/// activity receipt. There is no successful constructor that bypasses the
/// canonical verifier and core semantic policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerifiedProgramSettlement {
    transfer_set_root: [u8; 32],
    leg_count: usize,
    total_amount: u128,
}

impl VerifiedProgramSettlement {
    #[must_use]
    pub const fn transfer_set_root(&self) -> [u8; 32] {
        self.transfer_set_root
    }
    #[must_use]
    pub const fn leg_count(&self) -> usize {
        self.leg_count
    }
    #[must_use]
    pub const fn total_amount(&self) -> u128 {
        self.total_amount
    }
}

/// Closed monetary-law refusal taxonomy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferLawError {
    UnverifiedAuthority,
    InvalidTransfer,
    InvalidTransferSet,
    AmountOverflow,
    InvariantViolation,
    CapabilityEscalation,
    KernelRefused,
    ReceiptInvalid,
    ReceiptMismatch,
    StaleStorage,
}

impl Display for TransferLawError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnverifiedAuthority => formatter.write_str("invocation authority is unverified"),
            Self::InvalidTransfer => formatter.write_str("402LXP transfer request is invalid"),
            Self::InvalidTransferSet => formatter.write_str("402LXP transfer set is invalid"),
            Self::AmountOverflow => formatter.write_str("402LXP transfer total overflowed"),
            Self::InvariantViolation => formatter.write_str("INVARIANT 1 monetary bypass detected"),
            Self::CapabilityEscalation => {
                formatter.write_str("child transfer exceeds narrowed call authority")
            }
            Self::KernelRefused => formatter.write_str("kernel transfer primitive refused the set"),
            Self::ReceiptInvalid => formatter.write_str("standard settlement receipt is invalid"),
            Self::ReceiptMismatch => formatter.write_str("receipt does not bind the transfer set"),
            Self::StaleStorage => formatter.write_str("prepared storage snapshot is stale"),
        }
    }
}

impl std::error::Error for TransferLawError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::{Capability, ProgramCall, ProgramEvent};

    fn program_id(byte: u8) -> ProgramId {
        ProgramId::new([byte; 32]).unwrap_or_else(|error| panic!("program: {error}"))
    }

    fn principal_id(byte: u8) -> PrincipalId {
        PrincipalId::new([byte; 32]).unwrap_or_else(|error| panic!("principal: {error}"))
    }

    fn request(program: ProgramId, principal: PrincipalId, amount: u128) -> TransferRequest {
        request_for(program, principal, [3; 32], [4; 32], amount)
    }

    fn request_for(
        program: ProgramId,
        principal: PrincipalId,
        asset: [u8; 32],
        to: [u8; 32],
        amount: u128,
    ) -> TransferRequest {
        TransferRequest {
            program,
            principal,
            frame: if program == program_id(7) {
                CallFrameId::root()
                    .child(1)
                    .unwrap_or_else(|error| panic!("child frame: {error}"))
            } else {
                CallFrameId::root()
            },
            asset,
            to,
            amount,
        }
    }

    fn capability(program: ProgramId, principal: PrincipalId) -> TransferCapability {
        let grants = CapabilitySet::new([
            Capability::Call {
                program: program_id(7),
            },
            Capability::Transfer402 {
                asset: [3; 32],
                to: [4; 32],
                maximum_amount: u128::MAX,
            },
            Capability::Transfer402 {
                asset: [3; 32],
                to: [9; 32],
                maximum_amount: u128::MAX,
            },
        ])
        .unwrap_or_else(|error| panic!("root capabilities: {error}"));
        let authorization = AuthorizationContext::new(principal, grants);
        TransferCapability::from_root_authorization(program, &authorization, [5; 32])
            .unwrap_or_else(|error| panic!("capability: {error}"))
    }

    fn child_call(
        parent_program: ProgramId,
        child_program: ProgramId,
        principal: PrincipalId,
        asset: [u8; 32],
        to: [u8; 32],
        maximum_amount: u128,
    ) -> ProgramCall {
        let capabilities = CapabilitySet::new([Capability::Transfer402 {
            asset,
            to,
            maximum_amount,
        }])
        .unwrap_or_else(|error| panic!("child capabilities: {error}"));
        ProgramCall {
            caller: parent_program,
            callee: child_program,
            principal,
            caller_frame: CallFrameId::root(),
            callee_frame: CallFrameId::root()
                .child(1)
                .unwrap_or_else(|error| panic!("child frame: {error}")),
            input: Vec::new(),
            capabilities,
        }
    }

    #[test]
    fn transfer_set_is_bound_to_invocation_authority_and_exact_order() {
        let program = program_id(1);
        let principal = principal_id(2);
        let capability = capability(program, principal);
        let effects = AbiEffects {
            transfers: vec![
                request(program, principal, 7),
                request(program, principal, 11),
            ],
            ..AbiEffects::default()
        };
        let set = capability
            .authorize(&effects)
            .unwrap_or_else(|error| panic!("set: {error}"));
        assert_eq!(set.program(), program);
        assert_eq!(set.principal(), principal);
        assert_eq!(set.invocation_authority(), [5; 32]);
        assert_eq!(set.total_amount(), 18);
        assert_eq!(set.legs(), effects.transfers);
        assert!(set
            .canonical()
            .starts_with(b"LayerX/programs/402LXP/transfer-set/v1\0"));
    }

    #[test]
    fn empty_invalid_and_overflowing_sets_are_refused_before_core() {
        let program = program_id(1);
        let principal = principal_id(2);
        assert_eq!(
            TransferCapability::from_root_authorization(
                program,
                &AuthorizationContext::new(principal, CapabilitySet::empty()),
                [0; 32],
            ),
            Err(TransferLawError::UnverifiedAuthority)
        );
        let capability = capability(program, principal);
        assert_eq!(
            capability.authorize(&AbiEffects::default()),
            Err(TransferLawError::InvalidTransferSet)
        );
        let overflow = AbiEffects {
            transfers: vec![
                request(program, principal, u128::MAX),
                request(program, principal, 1),
            ],
            ..AbiEffects::default()
        };
        assert_eq!(
            capability.authorize(&overflow),
            Err(TransferLawError::AmountOverflow)
        );
    }

    #[test]
    fn forged_program_or_principal_is_an_invariant_one_violation() {
        let program = program_id(1);
        let principal = principal_id(2);
        let capability = capability(program, principal);
        for transfer in [
            request(program_id(9), principal, 1),
            request(program, principal_id(9), 1),
        ] {
            assert_eq!(
                capability.authorize(&AbiEffects {
                    transfers: vec![transfer],
                    ..AbiEffects::default()
                }),
                Err(TransferLawError::InvariantViolation)
            );
        }
    }

    #[test]
    fn child_transfer_requires_a_reachable_call_graph_edge() {
        let root = program_id(1);
        let child = program_id(7);
        let principal = principal_id(2);
        assert_eq!(
            capability(root, principal).authorize(&AbiEffects {
                transfers: vec![request(child, principal, 1)],
                ..AbiEffects::default()
            }),
            Err(TransferLawError::InvariantViolation)
        );
    }

    #[test]
    fn child_call_cannot_change_the_invoking_principal() {
        let root = program_id(1);
        let child = program_id(7);
        let principal = principal_id(2);
        assert_eq!(
            capability(root, principal).authorize(&AbiEffects {
                calls: vec![child_call(
                    root,
                    child,
                    principal_id(9),
                    [3; 32],
                    [4; 32],
                    10,
                )],
                transfers: vec![request(child, principal, 1)],
                ..AbiEffects::default()
            }),
            Err(TransferLawError::InvariantViolation)
        );
    }

    #[test]
    fn child_transfer_must_fit_its_narrowed_asset_recipient_and_amount() {
        let root = program_id(1);
        let child = program_id(7);
        let principal = principal_id(2);
        let capability = capability(root, principal);
        for transfer in [
            request_for(child, principal, [8; 32], [4; 32], 1),
            request_for(child, principal, [3; 32], [8; 32], 1),
            request_for(child, principal, [3; 32], [4; 32], 11),
        ] {
            assert_eq!(
                capability.authorize(&AbiEffects {
                    calls: vec![child_call(root, child, principal, [3; 32], [4; 32], 10)],
                    transfers: vec![transfer],
                    ..AbiEffects::default()
                }),
                Err(TransferLawError::CapabilityEscalation)
            );
        }
        assert_eq!(
            capability.authorize(&AbiEffects {
                calls: vec![child_call(root, child, principal, [3; 32], [4; 32], 10)],
                transfers: vec![request(child, principal, 6), request(child, principal, 6)],
                ..AbiEffects::default()
            }),
            Err(TransferLawError::CapabilityEscalation)
        );
    }

    #[test]
    fn canonical_transfer_set_commits_child_provenance_and_call_grants() {
        let root = program_id(1);
        let child = program_id(7);
        let principal = principal_id(2);
        let capability = capability(root, principal);
        let effects = |maximum_amount| AbiEffects {
            calls: vec![child_call(
                root,
                child,
                principal,
                [3; 32],
                [4; 32],
                maximum_amount,
            )],
            transfers: vec![request(child, principal, 7)],
            ..AbiEffects::default()
        };
        let narrow = capability
            .authorize(&effects(7))
            .unwrap_or_else(|error| panic!("narrow graph: {error}"));
        let broad = capability
            .authorize(&effects(8))
            .unwrap_or_else(|error| panic!("broad graph: {error}"));
        assert_eq!(narrow.legs()[0].program(), child);
        assert_ne!(narrow.canonical(), broad.canonical());
    }

    #[test]
    fn malformed_zero_and_oversized_transfer_sets_never_reach_the_kernel_boundary() {
        let program = program_id(1);
        let principal = principal_id(2);
        let capability = capability(program, principal);
        for transfer in [
            request_for(program, principal, [0; 32], [4; 32], 1),
            request_for(program, principal, [3; 32], [0; 32], 1),
            request_for(program, principal, [3; 32], [4; 32], 0),
        ] {
            assert_eq!(
                capability.authorize(&AbiEffects {
                    transfers: vec![transfer],
                    ..AbiEffects::default()
                }),
                Err(TransferLawError::InvalidTransfer)
            );
        }
        assert_eq!(
            capability.authorize(&AbiEffects {
                transfers: vec![request(program, principal, 1); 257],
                ..AbiEffects::default()
            }),
            Err(TransferLawError::InvalidTransferSet)
        );
    }

    #[test]
    fn disconnected_and_forged_nested_call_staging_is_rejected_as_invariant_one() {
        let root = program_id(1);
        let child = program_id(7);
        let disconnected = program_id(8);
        let principal = principal_id(2);
        assert_eq!(
            capability(root, principal).authorize(&AbiEffects {
                calls: vec![
                    child_call(root, child, principal, [3; 32], [4; 32], 10),
                    child_call(disconnected, child, principal, [3; 32], [4; 32], 10),
                ],
                transfers: vec![request(child, principal, 1)],
                ..AbiEffects::default()
            }),
            Err(TransferLawError::InvariantViolation)
        );
    }

    #[test]
    fn canonical_set_evidence_binds_leg_order_and_every_destination() {
        let program = program_id(1);
        let principal = principal_id(2);
        let capability = capability(program, principal);
        let effects = |reverse| AbiEffects {
            transfers: if reverse {
                vec![
                    request_for(program, principal, [3; 32], [9; 32], 11),
                    request_for(program, principal, [3; 32], [4; 32], 7),
                ]
            } else {
                vec![
                    request_for(program, principal, [3; 32], [4; 32], 7),
                    request_for(program, principal, [3; 32], [9; 32], 11),
                ]
            },
            ..AbiEffects::default()
        };
        let forward = capability
            .authorize(&effects(false))
            .unwrap_or_else(|error| panic!("forward: {error}"));
        let reversed = capability
            .authorize(&effects(true))
            .unwrap_or_else(|error| panic!("reversed: {error}"));
        assert_eq!(forward.total_amount(), reversed.total_amount());
        assert_ne!(forward.canonical(), reversed.canonical());
    }

    #[test]
    fn canonical_decoder_rejects_inner_and_trailing_event_malleability() {
        let program = program_id(1);
        let principal = principal_id(2);
        let set = capability(program, principal)
            .authorize(&AbiEffects {
                events: vec![ProgramEvent {
                    program,
                    principal,
                    frame: CallFrameId::root(),
                    topic: b"paid".to_vec(),
                    data: vec![9],
                }],
                transfers: vec![request(program, principal, 7)],
                ..AbiEffects::default()
            })
            .unwrap_or_else(|error| panic!("set: {error}"));
        let decoded = AtomicTransferSet::canonical_decode(set.canonical())
            .unwrap_or_else(|error| panic!("decode: {error}"));
        assert_eq!(decoded.canonical(), set.canonical());

        let mut trailing = set.canonical().to_vec();
        trailing.push(0);
        assert_eq!(
            AtomicTransferSet::canonical_decode(&trailing),
            Err(TransferLawError::InvalidTransferSet)
        );

        let event_count_offset =
            SET_DOMAIN.len() + 32 + 32 + 32 + 8 + 1 + 4 + b"LayerX/programs/events/v1\0".len();
        let mut inner_malleable = set.canonical().to_vec();
        inner_malleable[event_count_offset + 3] = 0;
        assert_eq!(
            AtomicTransferSet::canonical_decode(&inner_malleable),
            Err(TransferLawError::InvalidTransferSet)
        );
    }
}
