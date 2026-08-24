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
use crate::accounts::{derive_program_account, MAX_PROGRAM_ACCOUNT_SEED_BYTES};
use crate::calls::CallGraph;
use crate::crypto::{hash_bytes, HashAlgorithm};
use crate::storage::{PrincipalId, ProgramId};

const SET_DOMAIN_V1: &[u8] = b"LayerX/programs/402LXP/transfer-set/v1\0";
const SET_DOMAIN_V2: &[u8] = b"LayerX/programs/402LXP/transfer-set/v2\0";
const PROGRAM_AUTHORITY_DOMAIN: &[u8] = b"LayerX/programs/402LXP/program-authority/v1\0";
const MERKLE_LEAF_DOMAIN: &[u8] = b"LXP/v1/merkle-leaf\0";
const MERKLE_INTERNAL_DOMAIN: &[u8] = b"LXP/v1/merkle-internal\0";
const SOURCE_PRINCIPAL: u8 = 1;
const SOURCE_PROGRAM: u8 = 2;
const MAX_TRANSFER_LEGS: usize = 256;

/// Exact, owner-frame authority for one program-account debit.
///
/// The constructor is crate-private and recomputes the derived source account,
/// so a guest cannot fabricate this token by naming an account. The token binds
/// the owner program, exact seed, host-assigned staging frame and every monetary
/// field of the leg. It grants no balance access; it is consumed only by the
/// existing atomic kernel transfer-set boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramAuthority {
    owner_program: ProgramId,
    seed: Vec<u8>,
    source_account: [u8; 32],
    staging_frame: CallFrameId,
    asset: [u8; 32],
    to: [u8; 32],
    amount: u128,
}

impl ProgramAuthority {
    pub(crate) fn issue(
        owner_program: ProgramId,
        seed: &[u8],
        source_account: [u8; 32],
        staging_frame: CallFrameId,
        asset: [u8; 32],
        to: [u8; 32],
        amount: u128,
    ) -> Result<Self, TransferLawError> {
        if asset == [0; 32]
            || to == [0; 32]
            || amount == 0
            || seed.len() > MAX_PROGRAM_ACCOUNT_SEED_BYTES
        {
            return Err(TransferLawError::InvalidProgramAuthority);
        }
        let derived = derive_program_account(owner_program, seed)
            .map_err(|_| TransferLawError::InvalidProgramAuthority)?;
        if !derived.matches(&source_account) {
            return Err(TransferLawError::InvalidProgramAuthority);
        }
        Ok(Self {
            owner_program,
            seed: seed.to_vec(),
            source_account,
            staging_frame,
            asset,
            to,
            amount,
        })
    }

    #[must_use]
    pub const fn owner_program(&self) -> ProgramId {
        self.owner_program
    }

    #[must_use]
    pub fn seed(&self) -> &[u8] {
        &self.seed
    }

    #[must_use]
    pub const fn source_account(&self) -> [u8; 32] {
        self.source_account
    }

    #[must_use]
    pub const fn staging_frame(&self) -> CallFrameId {
        self.staging_frame
    }

    #[must_use]
    pub const fn asset(&self) -> [u8; 32] {
        self.asset
    }

    #[must_use]
    pub const fn to(&self) -> [u8; 32] {
        self.to
    }

    #[must_use]
    pub const fn amount(&self) -> u128 {
        self.amount
    }

    fn canonical_encoding(&self) -> Result<Vec<u8>, TransferLawError> {
        let seed_length = u16::try_from(self.seed.len())
            .map_err(|_| TransferLawError::InvalidProgramAuthority)?;
        let mut encoded = Vec::with_capacity(
            PROGRAM_AUTHORITY_DOMAIN
                .len()
                .saturating_add(32 + 2 + self.seed.len() + 32 + 9 + 32 + 32 + 16),
        );
        encoded.extend_from_slice(PROGRAM_AUTHORITY_DOMAIN);
        encoded.extend_from_slice(&self.owner_program.bytes());
        encoded.extend_from_slice(&seed_length.to_be_bytes());
        encoded.extend_from_slice(&self.seed);
        encoded.extend_from_slice(&self.source_account);
        let (path, depth) = self.staging_frame.canonical_bytes();
        encoded.extend_from_slice(&path);
        encoded.push(depth);
        encoded.extend_from_slice(&self.asset);
        encoded.extend_from_slice(&self.to);
        encoded.extend_from_slice(&self.amount.to_be_bytes());
        Ok(encoded)
    }
}

/// Explicit debit source for one 402LXP transfer request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransferSource {
    /// The protocol-authenticated principal that invoked the activity.
    Principal(PrincipalId),
    /// An exact account derived by the program that staged this leg.
    Program(ProgramAuthority),
}

impl TransferSource {
    #[must_use]
    pub const fn account(&self) -> [u8; 32] {
        match self {
            Self::Principal(principal) => principal.bytes(),
            Self::Program(authority) => authority.source_account(),
        }
    }
}

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
        let candidate_v2 = self.root_capabilities.has_program_spend()
            || effects
                .calls
                .iter()
                .any(|call| call.capabilities.has_program_spend())
            || effects
                .transfers
                .iter()
                .any(|transfer| matches!(&transfer.source, TransferSource::Program(_)));
        let set_domain = if candidate_v2 {
            SET_DOMAIN_V2
        } else {
            SET_DOMAIN_V1
        };
        let mut total = 0u128;
        let mut canonical = Vec::with_capacity(
            set_domain.len()
                + 112
                + effects.calls.len().saturating_mul(166)
                + effects.transfers.len().saturating_mul(320),
        );
        canonical.extend_from_slice(set_domain);
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
        let mut principal_frame_totals = BTreeMap::new();
        let mut principal_graph_totals = BTreeMap::new();
        let mut program_frame_totals = BTreeMap::new();
        let mut program_graph_totals = BTreeMap::new();
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
            match &transfer.source {
                TransferSource::Principal(source) => {
                    if *source != self.principal {
                        return Err(TransferLawError::UnverifiedAuthority);
                    }
                    let frame_key = (transfer.frame, transfer.asset, transfer.to);
                    let frame_amount = principal_frame_totals
                        .get(&frame_key)
                        .copied()
                        .unwrap_or(0_u128)
                        .checked_add(transfer.amount)
                        .ok_or(TransferLawError::AmountOverflow)?;
                    if !capabilities.permits_transfer(transfer.asset, transfer.to, frame_amount) {
                        return Err(TransferLawError::CapabilityEscalation);
                    }
                    principal_frame_totals.insert(frame_key, frame_amount);
                    let graph_key = (transfer.asset, transfer.to);
                    let graph_amount = principal_graph_totals
                        .get(&graph_key)
                        .copied()
                        .unwrap_or(0_u128)
                        .checked_add(transfer.amount)
                        .ok_or(TransferLawError::AmountOverflow)?;
                    if !self.root_capabilities.permits_transfer(
                        transfer.asset,
                        transfer.to,
                        graph_amount,
                    ) {
                        return Err(TransferLawError::CapabilityEscalation);
                    }
                    principal_graph_totals.insert(graph_key, graph_amount);
                }
                TransferSource::Program(authority) => {
                    if authority.owner_program != *program
                        || authority.owner_program != transfer.program
                        || authority.staging_frame != transfer.frame
                        || authority.asset != transfer.asset
                        || authority.to != transfer.to
                        || authority.amount != transfer.amount
                        || derive_program_account(authority.owner_program, &authority.seed)
                            .map_err(|_| TransferLawError::InvalidProgramAuthority)?
                            .bytes()
                            != authority.source_account
                    {
                        return Err(TransferLawError::InvalidProgramAuthority);
                    }
                    let frame_key = (
                        transfer.frame,
                        authority.owner_program,
                        authority.seed.clone(),
                        authority.source_account,
                        transfer.asset,
                        transfer.to,
                    );
                    let frame_amount = program_frame_totals
                        .get(&frame_key)
                        .copied()
                        .unwrap_or(0_u128)
                        .checked_add(transfer.amount)
                        .ok_or(TransferLawError::AmountOverflow)?;
                    if !capabilities.permits_program_spend(
                        crate::abi::capability::ProgramSpendAuthorization {
                            staging_program: transfer.program,
                            owner_program: authority.owner_program,
                            seed: &authority.seed,
                            source_account: authority.source_account,
                            asset: transfer.asset,
                            to: transfer.to,
                            amount: frame_amount,
                        },
                    ) {
                        return Err(TransferLawError::CapabilityEscalation);
                    }
                    program_frame_totals.insert(frame_key, frame_amount);
                    let graph_key = (
                        authority.owner_program,
                        authority.seed.clone(),
                        authority.source_account,
                        transfer.asset,
                        transfer.to,
                    );
                    let graph_amount = program_graph_totals
                        .get(&graph_key)
                        .copied()
                        .unwrap_or(0_u128)
                        .checked_add(transfer.amount)
                        .ok_or(TransferLawError::AmountOverflow)?;
                    if !self.root_capabilities.permits_program_spend(
                        crate::abi::capability::ProgramSpendAuthorization {
                            staging_program: authority.owner_program,
                            owner_program: authority.owner_program,
                            seed: &authority.seed,
                            source_account: authority.source_account,
                            asset: transfer.asset,
                            to: transfer.to,
                            amount: graph_amount,
                        },
                    ) {
                        return Err(TransferLawError::CapabilityEscalation);
                    }
                    program_graph_totals.insert(graph_key, graph_amount);
                }
            }
            let (path, depth) = transfer.frame.canonical_bytes();
            canonical.extend_from_slice(&path);
            canonical.push(depth);
            if candidate_v2 {
                match &transfer.source {
                    TransferSource::Principal(source) => {
                        canonical.push(SOURCE_PRINCIPAL);
                        canonical.extend_from_slice(&source.bytes());
                    }
                    TransferSource::Program(authority) => {
                        canonical.push(SOURCE_PROGRAM);
                        let encoded = authority.canonical_encoding()?;
                        let length = u32::try_from(encoded.len())
                            .map_err(|_| TransferLawError::InvalidProgramAuthority)?;
                        canonical.extend_from_slice(&length.to_be_bytes());
                        canonical.extend_from_slice(&encoded);
                    }
                }
            }
            canonical.extend_from_slice(&transfer.asset);
            canonical.extend_from_slice(&transfer.to);
            canonical.extend_from_slice(&transfer.amount.to_be_bytes());
            canonical.extend_from_slice(&transfer.program.bytes());
        }
        let kernel_canonical = canonical_kernel_legs(&effects.transfers)?;
        let kernel_root = canonical_kernel_root(&kernel_canonical, effects.transfers.len())?;
        Ok(AtomicTransferSet {
            program: self.program,
            principal: self.principal,
            invocation_authority: self.invocation_authority,
            authorization_evidence: canonical,
            kernel_canonical,
            kernel_root,
            total_amount: total,
            legs: effects.transfers.clone(),
            candidate_v2,
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
            if *caller != call.caller
                || !parent_capabilities
                    .contains_narrowed_for_program_edge(call.caller, &call.capabilities)
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
        if evidence.transfer_set_root != transfers.kernel_root
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
    kernel_root: [u8; 32],
    total_amount: u128,
    legs: Vec<TransferRequest>,
    candidate_v2: bool,
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
    pub const fn kernel_root(&self) -> [u8; 32] {
        self.kernel_root
    }
    #[must_use]
    pub const fn total_amount(&self) -> u128 {
        self.total_amount
    }
    #[must_use]
    pub fn legs(&self) -> &[TransferRequest] {
        &self.legs
    }

    #[must_use]
    pub const fn is_candidate_v2(&self) -> bool {
        self.candidate_v2
    }

    /// Strictly decodes and validates a persisted authorisation artifact.
    pub(crate) fn canonical_decode(encoded: &[u8]) -> Result<Self, TransferLawError> {
        let mut cursor = TransferCursor::new(encoded);
        let candidate_v2 = if encoded.starts_with(SET_DOMAIN_V1) {
            false
        } else if encoded.starts_with(SET_DOMAIN_V2) {
            true
        } else {
            return Err(TransferLawError::InvalidTransferSet);
        };
        let set_domain = if candidate_v2 {
            SET_DOMAIN_V2
        } else {
            SET_DOMAIN_V1
        };
        if cursor.take(set_domain.len())? != set_domain {
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
        canonical.extend_from_slice(set_domain);
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
            let grants = if candidate_v2 {
                CapabilitySet::decode_candidate_canonical(grants)
            } else {
                CapabilitySet::decode_canonical(grants)
            }
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
            let source = if candidate_v2 {
                match cursor.take(1)?[0] {
                    SOURCE_PRINCIPAL => {
                        let source = PrincipalId::new(cursor.array()?)
                            .map_err(|_| TransferLawError::InvalidTransferSet)?;
                        if source != principal {
                            return Err(TransferLawError::UnverifiedAuthority);
                        }
                        TransferSource::Principal(source)
                    }
                    SOURCE_PROGRAM => {
                        let authority_length = u32::from_be_bytes(cursor.array()?) as usize;
                        let authority = decode_program_authority(cursor.take(authority_length)?)?;
                        TransferSource::Program(authority)
                    }
                    _ => return Err(TransferLawError::InvalidTransferSet),
                }
            } else {
                TransferSource::Principal(principal)
            };
            let asset = cursor.array()?;
            let to = cursor.array()?;
            let amount = u128::from_be_bytes(cursor.array()?);
            let leg_program = ProgramId::new(cursor.array()?)
                .map_err(|_| TransferLawError::InvalidTransferSet)?;
            if asset == [0; 32] || to == [0; 32] || amount == 0 {
                return Err(TransferLawError::InvalidTransfer);
            }
            if let TransferSource::Program(authority) = &source {
                if authority.owner_program != leg_program
                    || authority.staging_frame != frame
                    || authority.asset != asset
                    || authority.to != to
                    || authority.amount != amount
                {
                    return Err(TransferLawError::InvalidProgramAuthority);
                }
            }
            total = total
                .checked_add(amount)
                .ok_or(TransferLawError::AmountOverflow)?;
            legs.push(TransferRequest {
                program: leg_program,
                principal,
                frame,
                source: source.clone(),
                asset,
                to,
                amount,
            });
            let (path, depth) = frame.canonical_bytes();
            canonical.extend_from_slice(&path);
            canonical.push(depth);
            if candidate_v2 {
                match source {
                    TransferSource::Principal(source) => {
                        canonical.push(SOURCE_PRINCIPAL);
                        canonical.extend_from_slice(&source.bytes());
                    }
                    TransferSource::Program(authority) => {
                        canonical.push(SOURCE_PROGRAM);
                        let encoded = authority.canonical_encoding()?;
                        let length = u32::try_from(encoded.len())
                            .map_err(|_| TransferLawError::InvalidProgramAuthority)?;
                        canonical.extend_from_slice(&length.to_be_bytes());
                        canonical.extend_from_slice(&encoded);
                    }
                }
            }
            canonical.extend_from_slice(&asset);
            canonical.extend_from_slice(&to);
            canonical.extend_from_slice(&amount.to_be_bytes());
            canonical.extend_from_slice(&leg_program.bytes());
        }
        if !cursor.is_empty() || canonical != encoded {
            return Err(TransferLawError::InvalidTransferSet);
        }
        let kernel_canonical = canonical_kernel_legs(&legs)?;
        let kernel_root = canonical_kernel_root(&kernel_canonical, legs.len())?;
        Ok(Self {
            program,
            principal,
            invocation_authority,
            authorization_evidence: canonical,
            kernel_canonical,
            kernel_root,
            total_amount: total,
            legs,
            candidate_v2,
        })
    }
}

fn decode_program_authority(encoded: &[u8]) -> Result<ProgramAuthority, TransferLawError> {
    let mut cursor = TransferCursor::new(encoded);
    if cursor.take(PROGRAM_AUTHORITY_DOMAIN.len())? != PROGRAM_AUTHORITY_DOMAIN {
        return Err(TransferLawError::InvalidProgramAuthority);
    }
    let owner_program =
        ProgramId::new(cursor.array()?).map_err(|_| TransferLawError::InvalidProgramAuthority)?;
    let seed_length = usize::from(u16::from_be_bytes(cursor.array()?));
    if seed_length > MAX_PROGRAM_ACCOUNT_SEED_BYTES {
        return Err(TransferLawError::InvalidProgramAuthority);
    }
    let seed = cursor.take(seed_length)?;
    let source_account = cursor.array()?;
    let staging_frame = frame_from_cursor(&mut cursor)?;
    let asset = cursor.array()?;
    let to = cursor.array()?;
    let amount = u128::from_be_bytes(cursor.array()?);
    if !cursor.is_empty() {
        return Err(TransferLawError::InvalidProgramAuthority);
    }
    let authority = ProgramAuthority::issue(
        owner_program,
        seed,
        source_account,
        staging_frame,
        asset,
        to,
        amount,
    )?;
    if authority.canonical_encoding()? != encoded {
        return Err(TransferLawError::InvalidProgramAuthority);
    }
    Ok(authority)
}

fn canonical_kernel_legs(legs: &[TransferRequest]) -> Result<Vec<u8>, TransferLawError> {
    if legs.is_empty() || legs.len() > MAX_TRANSFER_LEGS {
        return Err(TransferLawError::InvalidTransferSet);
    }
    let mut encoded = Vec::with_capacity(legs.len().saturating_mul(115));
    for leg in legs {
        encoded.push(0);
        encoded.extend_from_slice(&leg.source.account());
        encoded.extend_from_slice(&leg.to);
        encoded.extend_from_slice(&leg.asset);
        encoded.extend_from_slice(&leg.amount.to_be_bytes());
        encoded.extend_from_slice(&1u16.to_be_bytes());
    }
    Ok(encoded)
}

fn canonical_kernel_root(
    encoded_legs: &[u8],
    leg_count: usize,
) -> Result<[u8; 32], TransferLawError> {
    const LEG_BYTES: usize = 115;
    if leg_count == 0
        || leg_count > MAX_TRANSFER_LEGS
        || encoded_legs.len() != leg_count.saturating_mul(LEG_BYTES)
    {
        return Err(TransferLawError::InvalidTransferSet);
    }
    let mut level = Vec::with_capacity(leg_count);
    for encoded in encoded_legs.chunks_exact(LEG_BYTES) {
        level.push(domain_hash(MERKLE_LEAF_DOMAIN, encoded)?);
    }
    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        for pair in level.chunks(2) {
            let right = pair.get(1).unwrap_or(&pair[0]);
            let mut children = [0u8; 64];
            children[..32].copy_from_slice(&pair[0]);
            children[32..].copy_from_slice(right);
            next.push(domain_hash(MERKLE_INTERNAL_DOMAIN, &children)?);
        }
        level = next;
    }
    level.pop().ok_or(TransferLawError::InvalidTransferSet)
}

fn domain_hash(domain: &[u8], bytes: &[u8]) -> Result<[u8; 32], TransferLawError> {
    let mut preimage = Vec::with_capacity(domain.len().saturating_add(bytes.len()));
    preimage.extend_from_slice(domain);
    preimage.extend_from_slice(bytes);
    hash_bytes(HashAlgorithm::Sha256, &preimage).map_err(|_| TransferLawError::InvariantViolation)
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
    InvalidProgramAuthority,
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
            Self::InvalidProgramAuthority => {
                formatter.write_str("program transfer authority is invalid")
            }
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
            source: TransferSource::Principal(principal),
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

    fn program_capability(
        program: ProgramId,
        principal: PrincipalId,
        seed: &[u8],
        source_account: [u8; 32],
        maximum_amount: u128,
    ) -> TransferCapability {
        let grants = CapabilitySet::new([
            Capability::Transfer402 {
                asset: [3; 32],
                to: [4; 32],
                maximum_amount,
            },
            Capability::ProgramSpend {
                owner_program: program,
                seed: seed.to_vec(),
                source_account,
                asset: [3; 32],
                to: [4; 32],
                maximum_amount,
            },
        ])
        .unwrap_or_else(|error| panic!("program capabilities: {error}"));
        let authorization = AuthorizationContext::new(principal, grants);
        TransferCapability::from_root_authorization(program, &authorization, [5; 32])
            .unwrap_or_else(|error| panic!("capability: {error}"))
    }

    fn program_request(
        program: ProgramId,
        principal: PrincipalId,
        seed: &[u8],
        source_account: [u8; 32],
        amount: u128,
    ) -> TransferRequest {
        let authority = ProgramAuthority::issue(
            program,
            seed,
            source_account,
            CallFrameId::root(),
            [3; 32],
            [4; 32],
            amount,
        )
        .unwrap_or_else(|error| panic!("program authority: {error}"));
        TransferRequest {
            program,
            principal,
            frame: CallFrameId::root(),
            source: TransferSource::Program(authority),
            asset: [3; 32],
            to: [4; 32],
            amount,
        }
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
            SET_DOMAIN_V1.len() + 32 + 32 + 32 + 8 + 1 + 4 + b"LayerX/programs/events/v1\0".len();
        let mut inner_malleable = set.canonical().to_vec();
        inner_malleable[event_count_offset + 3] = 0;
        assert_eq!(
            AtomicTransferSet::canonical_decode(&inner_malleable),
            Err(TransferLawError::InvalidTransferSet)
        );
    }

    #[test]
    fn program_authority_recomputes_the_exact_owner_seed_and_source() {
        let owner = program_id(1);
        let source = derive_program_account(owner, b"vault")
            .unwrap_or_else(|error| panic!("source: {error}"))
            .bytes();
        assert!(ProgramAuthority::issue(
            owner,
            b"vault",
            source,
            CallFrameId::root(),
            [3; 32],
            [4; 32],
            7,
        )
        .is_ok());
        assert_eq!(
            ProgramAuthority::issue(
                owner,
                b"other",
                source,
                CallFrameId::root(),
                [3; 32],
                [4; 32],
                7,
            ),
            Err(TransferLawError::InvalidProgramAuthority)
        );
        assert_eq!(
            ProgramAuthority::issue(
                program_id(9),
                b"vault",
                source,
                CallFrameId::root(),
                [3; 32],
                [4; 32],
                7,
            ),
            Err(TransferLawError::InvalidProgramAuthority)
        );
    }

    #[test]
    fn mixed_principal_and_program_sources_share_one_v2_set_and_kernel_root() {
        let owner = program_id(1);
        let principal = principal_id(2);
        let seed = b"vault";
        let source = derive_program_account(owner, seed)
            .unwrap_or_else(|error| panic!("source: {error}"))
            .bytes();
        let effects = AbiEffects {
            transfers: vec![
                request(owner, principal, 5),
                program_request(owner, principal, seed, source, 7),
            ],
            ..AbiEffects::default()
        };
        let set = program_capability(owner, principal, seed, source, 12)
            .authorize(&effects)
            .unwrap_or_else(|error| panic!("mixed set: {error}"));
        assert!(set.is_candidate_v2());
        assert!(set.canonical().starts_with(SET_DOMAIN_V2));
        assert_eq!(set.legs()[0].source.account(), principal.bytes());
        assert_eq!(set.legs()[1].source.account(), source);
        assert_eq!(
            set.kernel_root(),
            canonical_kernel_root(set.kernel_canonical(), set.legs().len())
                .unwrap_or_else(|error| panic!("kernel root: {error}"))
        );
        let decoded = AtomicTransferSet::canonical_decode(set.canonical())
            .unwrap_or_else(|error| panic!("decode: {error}"));
        assert_eq!(decoded, set);
    }

    #[test]
    fn owner_frame_and_cumulative_program_spend_boundaries_are_closed() {
        let owner = program_id(1);
        let principal = principal_id(2);
        let seed = b"pool";
        let source = derive_program_account(owner, seed)
            .unwrap_or_else(|error| panic!("source: {error}"))
            .bytes();
        let capability = program_capability(owner, principal, seed, source, 10);
        let over_limit = AbiEffects {
            transfers: vec![
                program_request(owner, principal, seed, source, 6),
                program_request(owner, principal, seed, source, 6),
            ],
            ..AbiEffects::default()
        };
        assert_eq!(
            capability.authorize(&over_limit),
            Err(TransferLawError::CapabilityEscalation)
        );

        let mut wrong_frame = program_request(owner, principal, seed, source, 7);
        let child = CallFrameId::root()
            .child(1)
            .unwrap_or_else(|error| panic!("frame: {error}"));
        let TransferSource::Program(authority) = &mut wrong_frame.source else {
            panic!("program source")
        };
        authority.staging_frame = child;
        assert_eq!(
            capability.authorize(&AbiEffects {
                transfers: vec![wrong_frame],
                ..AbiEffects::default()
            }),
            Err(TransferLawError::InvalidProgramAuthority)
        );

        let child_program = program_id(7);
        let child_frame = CallFrameId::root()
            .child(1)
            .unwrap_or_else(|error| panic!("child frame: {error}"));
        let forwarded = CapabilitySet::new([Capability::ProgramSpend {
            owner_program: owner,
            seed: seed.to_vec(),
            source_account: source,
            asset: [3; 32],
            to: [4; 32],
            maximum_amount: 10,
        }])
        .unwrap_or_else(|error| panic!("forwarded grant: {error}"));
        let child_authority =
            ProgramAuthority::issue(owner, seed, source, child_frame, [3; 32], [4; 32], 7)
                .unwrap_or_else(|error| panic!("child authority: {error}"));
        assert_eq!(
            capability.authorize(&AbiEffects {
                calls: vec![ProgramCall {
                    caller: owner,
                    callee: child_program,
                    principal,
                    caller_frame: CallFrameId::root(),
                    callee_frame: child_frame,
                    input: Vec::new(),
                    capabilities: forwarded,
                }],
                transfers: vec![TransferRequest {
                    program: child_program,
                    principal,
                    frame: child_frame,
                    source: TransferSource::Program(child_authority),
                    asset: [3; 32],
                    to: [4; 32],
                    amount: 7,
                }],
                ..AbiEffects::default()
            }),
            Err(TransferLawError::InvalidProgramAuthority)
        );
    }
}
