//! Deterministic protocol-owned expiry and terminal reclamation.

use core::fmt::{self, Display};

use layerx_programs::ProtocolDeploymentVerifier;
use layerx_programs_runtime::{
    hash_bytes, HashAlgorithm, KernelTransferPrimitive, Meter, PreparedAuthorizedActivity, Storage,
};
use layerx_proof::merkle::Proof;
use layerx_types::payload::ModuleId;
use layerx_wire::receipt::decode as decode_receipt;

use crate::{settle, Escrow, EscrowRefusal, Lease, LeaseId, LeaseRefusal, LeaseState};

const TERMINAL_DOMAIN: &[u8] = b"LayerX/programs/sandbox/terminal/v1\0";
const SWEEP_STATE_DOMAIN: &[u8] = b"LayerX/programs/sandbox/expiry-queue/v1\0";
const TERMINAL_EVENT: u16 = 0x090a;
const TERMINAL_EVENT_BYTES: usize = 352;
pub const MAX_SWEEP_LEASES_PER_BATCH: u32 = 6;
pub const MAX_EXPIRY_QUEUE_ENTRIES: usize = 4096;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalReceiptEvidence {
    pub canonical_receipt: Vec<u8>,
    pub receipt_proof: Proof,
    pub canonical_header: Vec<u8>,
    pub header_signature: [u8; 64],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthenticatedTerminalRecord {
    canonical: [u8; TERMINAL_EVENT_BYTES],
    receipt_activity_id: [u8; 32],
    lease: LeaseId,
}

impl AuthenticatedTerminalRecord {
    pub fn verify(
        verifier: &ProtocolDeploymentVerifier,
        evidence: TerminalReceiptEvidence,
        now_ms: u64,
        expected_lease: LeaseId,
    ) -> Result<Self, ExpiryRefusal> {
        let head = verifier
            .verify_current_protocol_head(
                &evidence.canonical_receipt,
                &evidence.receipt_proof,
                &evidence.canonical_header,
                &evidence.header_signature,
                now_ms,
            )
            .map_err(|_| ExpiryRefusal::InvalidProtocolReceipt)?;
        let receipt = decode_receipt(&evidence.canonical_receipt)
            .map_err(|_| ExpiryRefusal::InvalidProtocolReceipt)?;
        let protocol = receipt
            .protocol()
            .ok_or(ExpiryRefusal::InvalidProtocolReceipt)?;
        if protocol.activity_id() != head.activity_id() {
            return Err(ExpiryRefusal::InvalidProtocolReceipt);
        }
        let mut chunks: [Option<Vec<u8>>; 2] = [None, None];
        for effect in protocol.effects().iter().filter(|effect| {
            effect.module_id() == ModuleId::Programs as u16 && effect.event_type() == TERMINAL_EVENT
        }) {
            let body = effect.body();
            if body.len() < 8
                || &body[..4] != b"LXDT"
                || body[5] != 2
                || body[6] != 1
                || body[7] != 0
                || usize::from(body[4]) >= chunks.len()
                || chunks[usize::from(body[4])].is_some()
            {
                return Err(ExpiryRefusal::InvalidProtocolReceipt);
            }
            chunks[usize::from(body[4])] = Some(body[8..].to_vec());
        }
        let mut bytes = Vec::with_capacity(TERMINAL_EVENT_BYTES);
        for chunk in chunks {
            bytes.extend_from_slice(&chunk.ok_or(ExpiryRefusal::InvalidProtocolReceipt)?);
        }
        if bytes.len() != TERMINAL_EVENT_BYTES
            || &bytes[..5] != b"LXSD1"
            || bytes[5..37] != expected_lease.bytes()
        {
            return Err(ExpiryRefusal::InvalidProtocolReceipt);
        }
        let mut canonical = [0u8; TERMINAL_EVENT_BYTES];
        canonical.copy_from_slice(&bytes);
        if canonical[37..69] == [0; 32]
            || canonical[117..149] == [0; 32]
            || u64::from_be_bytes(
                canonical[69..77]
                    .try_into()
                    .map_err(|_| ExpiryRefusal::InvalidProtocolReceipt)?,
            ) == 0
            || u64::from_be_bytes(
                canonical[77..85]
                    .try_into()
                    .map_err(|_| ExpiryRefusal::InvalidProtocolReceipt)?,
            ) == 0
        {
            return Err(ExpiryRefusal::InvalidProtocolReceipt);
        }
        Ok(Self {
            canonical,
            receipt_activity_id: head.activity_id(),
            lease: expected_lease,
        })
    }
    #[must_use]
    pub fn canonical_bytes(&self) -> &[u8; TERMINAL_EVENT_BYTES] {
        &self.canonical
    }
    #[must_use]
    pub const fn receipt_activity_id(&self) -> [u8; 32] {
        self.receipt_activity_id
    }
    #[must_use]
    pub fn destroy_activity_id(&self) -> [u8; 32] {
        let mut id = [0u8; 32];
        id.copy_from_slice(&self.canonical[37..69]);
        id
    }
    #[must_use]
    pub const fn lease(&self) -> LeaseId {
        self.lease
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SweepEvidence {
    pub expiry_activity_id: [u8; 32],
    pub expiry_receipt_digest: [u8; 32],
    pub destroy_activity_id: [u8; 32],
    pub destroy_receipt_digest: [u8; 32],
}

impl SweepEvidence {
    fn validate(self) -> Result<Self, ExpiryRefusal> {
        if self.expiry_activity_id == [0; 32]
            || self.expiry_receipt_digest == [0; 32]
            || self.destroy_activity_id == [0; 32]
            || self.destroy_receipt_digest == [0; 32]
            || self.expiry_activity_id == self.destroy_activity_id
            || self.expiry_receipt_digest == self.destroy_receipt_digest
        {
            return Err(ExpiryRefusal::InvalidEvidence);
        }
        Ok(self)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalLeaseRecord {
    lease: LeaseId,
    tenant: [u8; 32],
    host_program: [u8; 32],
    expiry: u64,
    destroyed_at: u64,
    reclaimed_cells: u64,
    reclaimed_bytes: u64,
    metered_cleanup_work: u64,
    refunded: u128,
    refund_transfer_root: [u8; 32],
    expiry_receipt_digest: [u8; 32],
    destroy_receipt_digest: [u8; 32],
    prior_lease_digest: [u8; 32],
    terminal_digest: [u8; 32],
}

impl TerminalLeaseRecord {
    #[must_use]
    pub const fn lease(&self) -> LeaseId {
        self.lease
    }
    #[must_use]
    pub const fn destroyed_at(&self) -> u64 {
        self.destroyed_at
    }
    #[must_use]
    pub const fn refunded(&self) -> u128 {
        self.refunded
    }
    #[must_use]
    pub const fn reclaimed_cells(&self) -> u64 {
        self.reclaimed_cells
    }
    #[must_use]
    pub const fn reclaimed_bytes(&self) -> u64 {
        self.reclaimed_bytes
    }
    #[must_use]
    pub const fn terminal_digest(&self) -> [u8; 32] {
        self.terminal_digest
    }

    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(TERMINAL_DOMAIN.len() + 288);
        bytes.extend_from_slice(TERMINAL_DOMAIN);
        bytes.extend_from_slice(&self.lease.bytes());
        bytes.extend_from_slice(&self.tenant);
        bytes.extend_from_slice(&self.host_program);
        bytes.extend_from_slice(&self.expiry.to_be_bytes());
        bytes.extend_from_slice(&self.destroyed_at.to_be_bytes());
        bytes.extend_from_slice(&self.reclaimed_cells.to_be_bytes());
        bytes.extend_from_slice(&self.reclaimed_bytes.to_be_bytes());
        bytes.extend_from_slice(&self.metered_cleanup_work.to_be_bytes());
        bytes.extend_from_slice(&self.refunded.to_be_bytes());
        bytes.extend_from_slice(&self.refund_transfer_root);
        bytes.extend_from_slice(&self.expiry_receipt_digest);
        bytes.extend_from_slice(&self.destroy_receipt_digest);
        bytes.extend_from_slice(&self.prior_lease_digest);
        bytes
    }

    pub fn verify(&self) -> Result<(), ExpiryRefusal> {
        let digest = hash_bytes(HashAlgorithm::Sha256, &self.canonical_bytes())
            .map_err(|_| ExpiryRefusal::HashRefusal)?;
        if digest != self.terminal_digest
            || self.destroyed_at < self.expiry
            || self.metered_cleanup_work
                != self
                    .reclaimed_cells
                    .checked_add(self.reclaimed_bytes)
                    .ok_or(ExpiryRefusal::AccountingOverflow)?
        {
            return Err(ExpiryRefusal::InvalidTerminalRecord);
        }
        Ok(())
    }
}

pub fn destroy<K: KernelTransferPrimitive>(
    lease: Lease,
    escrow: Escrow,
    storage: &mut Storage,
    meter: &mut Meter,
    prepared_refund: PreparedAuthorizedActivity,
    kernel: &mut K,
    boundary: u64,
    evidence: SweepEvidence,
) -> Result<TerminalLeaseRecord, ExpiryRefusal> {
    let evidence = evidence.validate()?;
    if boundary < lease.expiry() || lease.state() == LeaseState::Destroyed {
        return Err(ExpiryRefusal::NotDue);
    }
    let prior_lease_digest = lease.state_digest().map_err(ExpiryRefusal::Lease)?;
    let mut candidate_lease = lease;
    let mut candidate_escrow = escrow;
    let mut candidate_storage = storage.clone();
    let mut candidate_meter = meter.clone();
    if candidate_lease.state() != LeaseState::Expired {
        candidate_lease
            .expire_by_sweep(
                evidence.expiry_activity_id,
                evidence.expiry_receipt_digest,
                boundary,
            )
            .map_err(ExpiryRefusal::Lease)?;
    }
    let settlement_lease = candidate_lease.clone();
    let (_, reclaimed_cells, reclaimed_bytes) = candidate_lease
        .destroy_by_sweep(
            &mut candidate_storage,
            &mut candidate_meter,
            evidence.destroy_activity_id,
            evidence.destroy_receipt_digest,
            boundary,
        )
        .map_err(ExpiryRefusal::Lease)?;
    let refund = candidate_escrow
        .remaining()
        .map_err(ExpiryRefusal::Escrow)?;
    let outcome = settle(
        &mut candidate_escrow,
        &settlement_lease,
        prepared_refund,
        &mut candidate_storage,
        kernel,
    )
    .map_err(ExpiryRefusal::Escrow)?;
    if outcome.amount() != refund {
        return Err(ExpiryRefusal::RefundMismatch);
    }
    let refund_transfer_root = outcome
        .settlement()
        .map_or([0; 32], |value| value.transfer_set_root());
    if (refund == 0) != (refund_transfer_root == [0; 32]) {
        return Err(ExpiryRefusal::RefundMismatch);
    }
    let metered_cleanup_work = reclaimed_cells
        .checked_add(reclaimed_bytes)
        .ok_or(ExpiryRefusal::AccountingOverflow)?;
    let mut record = TerminalLeaseRecord {
        lease: candidate_lease.id(),
        tenant: candidate_lease.tenant().bytes(),
        host_program: candidate_lease.host_program().bytes(),
        expiry: candidate_lease.expiry(),
        destroyed_at: boundary,
        reclaimed_cells,
        reclaimed_bytes,
        metered_cleanup_work,
        refunded: refund,
        refund_transfer_root,
        expiry_receipt_digest: evidence.expiry_receipt_digest,
        destroy_receipt_digest: evidence.destroy_receipt_digest,
        prior_lease_digest,
        terminal_digest: [0; 32],
    };
    record.terminal_digest = hash_bytes(HashAlgorithm::Sha256, &record.canonical_bytes())
        .map_err(|_| ExpiryRefusal::HashRefusal)?;
    record.verify()?;
    *storage = candidate_storage;
    *meter = candidate_meter;
    Ok(record)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpiryQueue {
    scheduled: Vec<(u64, LeaseId)>,
    terminal: Vec<TerminalLeaseRecord>,
}

impl ExpiryQueue {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            scheduled: Vec::new(),
            terminal: Vec::new(),
        }
    }

    pub fn schedule(&mut self, lease: &Lease) -> Result<(), ExpiryRefusal> {
        let key = (lease.expiry(), lease.id());
        if lease.state() == LeaseState::Destroyed
            || self.terminal.iter().any(|v| v.lease == lease.id())
            || self.scheduled.binary_search(&key).is_ok()
        {
            return Err(ExpiryRefusal::Replay);
        }
        if self.scheduled.len() == MAX_EXPIRY_QUEUE_ENTRIES {
            return Err(ExpiryRefusal::QueueFull);
        }
        let at = self.scheduled.binary_search(&key).unwrap_or_else(|at| at);
        self.scheduled.insert(at, key);
        Ok(())
    }

    pub fn due(&self, boundary: u64, limit: u32) -> Result<SweepPage, ExpiryRefusal> {
        if limit == 0 || limit > MAX_SWEEP_LEASES_PER_BATCH {
            return Err(ExpiryRefusal::InvalidLimit);
        }
        let mut leases = Vec::with_capacity(limit as usize);
        for (expiry, lease) in &self.scheduled {
            if *expiry > boundary {
                break;
            }
            if leases.len() == limit as usize {
                break;
            }
            leases.push(*lease);
        }
        let remaining_due = self
            .scheduled
            .iter()
            .filter(|(expiry, _)| *expiry <= boundary)
            .count()
            .checked_sub(leases.len())
            .ok_or(ExpiryRefusal::AccountingOverflow)?;
        Ok(SweepPage {
            boundary,
            leases,
            remaining_due: u64::try_from(remaining_due)
                .map_err(|_| ExpiryRefusal::AccountingOverflow)?,
        })
    }

    pub fn record_destroyed(&mut self, record: TerminalLeaseRecord) -> Result<(), ExpiryRefusal> {
        record.verify()?;
        let key = (record.expiry, record.lease);
        let at = self
            .scheduled
            .binary_search(&key)
            .map_err(|_| ExpiryRefusal::Replay)?;
        if self.terminal.len() == MAX_EXPIRY_QUEUE_ENTRIES
            || self.terminal.iter().any(|v| v.lease == record.lease)
        {
            return Err(ExpiryRefusal::Replay);
        }
        self.terminal.push(record);
        self.scheduled.remove(at);
        Ok(())
    }

    #[must_use]
    pub fn terminal(&self, lease: LeaseId) -> Option<&TerminalLeaseRecord> {
        self.terminal.iter().find(|record| record.lease == lease)
    }

    pub fn canonical_state(&self) -> Result<Vec<u8>, ExpiryRefusal> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(SWEEP_STATE_DOMAIN);
        bytes.extend_from_slice(
            &u32::try_from(self.scheduled.len())
                .map_err(|_| ExpiryRefusal::AccountingOverflow)?
                .to_be_bytes(),
        );
        for (expiry, lease) in &self.scheduled {
            bytes.extend_from_slice(&expiry.to_be_bytes());
            bytes.extend_from_slice(&lease.bytes());
        }
        bytes.extend_from_slice(
            &u32::try_from(self.terminal.len())
                .map_err(|_| ExpiryRefusal::AccountingOverflow)?
                .to_be_bytes(),
        );
        for record in &self.terminal {
            let record_bytes = record.canonical_bytes();
            bytes.extend_from_slice(&record.lease.bytes());
            bytes.extend_from_slice(
                &u32::try_from(record_bytes.len())
                    .map_err(|_| ExpiryRefusal::AccountingOverflow)?
                    .to_be_bytes(),
            );
            bytes.extend_from_slice(&record_bytes);
            bytes.extend_from_slice(&record.terminal_digest);
        }
        Ok(bytes)
    }

    pub fn canonical_chunks(&self) -> Result<Vec<Vec<u8>>, ExpiryRefusal> {
        const CHUNK: usize = 1000;
        let state = self.canonical_state()?;
        let root =
            hash_bytes(HashAlgorithm::Sha256, &state).map_err(|_| ExpiryRefusal::HashRefusal)?;
        let count = state.len().div_ceil(CHUNK);
        if count > u16::MAX as usize {
            return Err(ExpiryRefusal::AccountingOverflow);
        }
        state
            .chunks(CHUNK)
            .enumerate()
            .map(|(index, body)| {
                let mut chunk = Vec::with_capacity(40 + body.len());
                chunk.extend_from_slice(b"LXSQ1");
                chunk.extend_from_slice(&root);
                chunk.extend_from_slice(&(index as u16).to_be_bytes());
                chunk.extend_from_slice(&(count as u16).to_be_bytes());
                chunk.extend_from_slice(body);
                Ok(chunk)
            })
            .collect()
    }
}

impl Default for ExpiryQueue {
    fn default() -> Self {
        Self::new()
    }
}

pub fn sweep<F>(
    queue: &mut ExpiryQueue,
    boundary: u64,
    limit: u32,
    mut destroy_due: F,
) -> Result<SweepPage, ExpiryRefusal>
where
    F: FnMut(LeaseId, u64) -> Result<TerminalLeaseRecord, ExpiryRefusal>,
{
    let page = queue.due(boundary, limit)?;
    for lease in page.leases.iter().copied() {
        let record = destroy_due(lease, boundary)?;
        if record.lease() != lease || record.destroyed_at() != boundary {
            return Err(ExpiryRefusal::InvalidTerminalRecord);
        }
        queue.record_destroyed(record)?;
    }
    queue.due(boundary, limit)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SweepPage {
    boundary: u64,
    leases: Vec<LeaseId>,
    remaining_due: u64,
}
impl SweepPage {
    #[must_use]
    pub const fn boundary(&self) -> u64 {
        self.boundary
    }
    #[must_use]
    pub fn leases(&self) -> &[LeaseId] {
        &self.leases
    }
    #[must_use]
    pub const fn remaining_due(&self) -> u64 {
        self.remaining_due
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExpiryRefusal {
    NotDue,
    InvalidLimit,
    Replay,
    InvalidEvidence,
    InvalidTerminalRecord,
    InvalidProtocolReceipt,
    AccountingOverflow,
    RefundMismatch,
    HashRefusal,
    QueueFull,
    Lease(LeaseRefusal),
    Escrow(EscrowRefusal),
}
impl Display for ExpiryRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}
impl std::error::Error for ExpiryRefusal {}

#[cfg(test)]
mod source_cases {
    use super::*;
    use crate::LeaseLimits;
    use layerx_programs_runtime::{PrincipalId, ProgramId};

    fn lease(id: u8, expiry: u64) -> Lease {
        Lease::request(
            LeaseId::new([id; 32]).expect("lease"),
            PrincipalId::new([id.wrapping_add(64); 32]).expect("tenant"),
            ProgramId::new([3; 32]).expect("host"),
            [4; 32],
            [5; 32],
            100,
            LeaseLimits {
                cpu_fuel: 10,
                memory_bytes: 10,
                storage_read_bytes: 10,
                storage_write_bytes: 10,
                output_values: 10,
                output_bytes: 10,
                table_elements: 10,
                namespace_bytes: 10,
            },
            1,
            expiry,
        )
        .expect("lease")
    }

    fn terminal(lease: &Lease, boundary: u64) -> TerminalLeaseRecord {
        let mut record = TerminalLeaseRecord {
            lease: lease.id(),
            tenant: lease.tenant().bytes(),
            host_program: lease.host_program().bytes(),
            expiry: lease.expiry(),
            destroyed_at: boundary,
            reclaimed_cells: 0,
            reclaimed_bytes: 0,
            metered_cleanup_work: 0,
            refunded: 100,
            refund_transfer_root: [7; 32],
            expiry_receipt_digest: [8; 32],
            destroy_receipt_digest: [9; 32],
            prior_lease_digest: lease.state_digest().expect("digest"),
            terminal_digest: [0; 32],
        };
        record.terminal_digest =
            hash_bytes(HashAlgorithm::Sha256, &record.canonical_bytes()).expect("terminal digest");
        record
    }

    #[test]
    fn cohort_is_ordered_bounded_and_carried_across_batches() {
        let mut queue = ExpiryQueue::new();
        for id in 1..=14 {
            queue.schedule(&lease(id, 20)).expect("schedule");
        }
        let first = queue.due(19, MAX_SWEEP_LEASES_PER_BATCH).expect("page");
        assert!(first.leases().is_empty());
        let first = queue.due(20, MAX_SWEEP_LEASES_PER_BATCH).expect("page");
        assert_eq!(first.leases().len(), 6);
        assert_eq!(first.remaining_due(), 8);
        assert_eq!(first.leases()[0], LeaseId::new([1; 32]).expect("id"));
        assert_eq!(first.leases()[5], LeaseId::new([6; 32]).expect("id"));
        for id in 1..=6 {
            let value = lease(id, 20);
            queue
                .record_destroyed(terminal(&value, 20))
                .expect("first batch");
        }
        let second = queue
            .due(21, MAX_SWEEP_LEASES_PER_BATCH)
            .expect("second page");
        assert_eq!(second.leases().len(), 6);
        assert_eq!(second.remaining_due(), 2);
        for id in 7..=12 {
            let value = lease(id, 20);
            queue
                .record_destroyed(terminal(&value, 21))
                .expect("second batch");
        }
        let third = queue
            .due(22, MAX_SWEEP_LEASES_PER_BATCH)
            .expect("third page");
        assert_eq!(third.leases().len(), 2);
        assert_eq!(third.remaining_due(), 0);
    }

    #[test]
    fn queue_refuses_duplicate_and_unbounded_sweep_admission() {
        let mut queue = ExpiryQueue::new();
        let lease = lease(1, 20);
        queue.schedule(&lease).expect("schedule");
        assert_eq!(queue.schedule(&lease), Err(ExpiryRefusal::Replay));
        assert_eq!(queue.due(20, 0), Err(ExpiryRefusal::InvalidLimit));
        assert_eq!(
            queue.due(20, MAX_SWEEP_LEASES_PER_BATCH + 1),
            Err(ExpiryRefusal::InvalidLimit)
        );
    }
}
