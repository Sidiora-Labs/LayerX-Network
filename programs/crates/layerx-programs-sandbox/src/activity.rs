//! Canonical production submission surface for lease-scoped sandbox work.

use core::fmt::{self, Display};

use layerx_programs_runtime::{hash_bytes, HashAlgorithm};
use layerx_programs::{ProtocolDeploymentVerifier, VerifiedProtocolHead};
use layerx_proof::merkle::{verify_path, Proof};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_wire::activity::{decode_signed, encode_signed};
use layerx_wire::hash::{activity_id, batch_header_digest, payload_hash};
use layerx_wire::receipt::{decode as decode_receipt, decode_batch_header, encode_batch_header};

use crate::{AuthenticatedUsageReceipt, Lease, LeaseId, LeaseState, UsageReceipt,
    MAX_USAGE_STATE_VALUE_BYTES};

pub const PROGRAMS_SANDBOX_ACTIVITY_TYPE: u32 = 0x0009_0009;
const PAYLOAD_VERSION: u8 = 1;
const OP_EXECUTE: u8 = 1;
const OP_FUND: u8 = 2;
const OP_ACTIVATE: u8 = 3;
const FIXED_BYTES: usize = 236;
const CALL_FIXED_BYTES: usize = 106;
const LEASE_STORAGE_CAPABILITIES: &[u8] = &[0, 2, 1, 2];
const PROGRAMS_ACCOUNT_ABI_VERSION: u16 = 2;
const OCCUPANCY_PROTOCOL_VERSION: u16 = 2;
const SANDBOX_PROGRAMS_REGISTRATION_ABI: u16 = 3;
const SANDBOX_USAGE_EVENT: u16 = 0x0909;
const SANDBOX_USAGE_EVENT_KIND: u8 = 3;
const SANDBOX_USAGE_FRAME_BYTES: usize = 52;
const SANDBOX_USAGE_CHUNK_BYTES: usize = 204;
const SANDBOX_USAGE_MAGIC: &[u8; 4] = b"LXUR";
pub const MAX_CANONICAL_CALL_BYTES: usize = 1_056_000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SandboxSubmission {
    activity_type: u32,
    payload: Vec<u8>,
    lease: LeaseId,
    expected_usage_sequence: u64,
    expected_lease_digest: [u8; 32],
    protocol: SandboxProtocolContext,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SandboxProtocolContext { protocol_version: u16, active_programs_abi: u16 }

impl SandboxProtocolContext {
    pub fn new(protocol_version: u16, active_programs_abi: u16)
        -> Result<Self, SandboxActivityRefusal> {
        if protocol_version != OCCUPANCY_PROTOCOL_VERSION
            || active_programs_abi != SANDBOX_PROGRAMS_REGISTRATION_ABI {
            return Err(SandboxActivityRefusal::ProtocolMismatch);
        }
        Ok(Self { protocol_version, active_programs_abi })
    }
}

impl SandboxSubmission {
    #[must_use] pub const fn activity_type(&self) -> u32 { self.activity_type }
    #[must_use] pub fn payload(&self) -> &[u8] { &self.payload }
    #[must_use] pub const fn lease(&self) -> LeaseId { self.lease }
    #[must_use] pub const fn expected_usage_sequence(&self) -> u64 { self.expected_usage_sequence }
    #[must_use] pub const fn expected_lease_digest(&self) -> [u8; 32] { self.expected_lease_digest }

    pub fn decode(payload: &[u8], protocol: SandboxProtocolContext)
        -> Result<Self, SandboxActivityRefusal> {
        if payload.len() < 4 || payload[0] != PAYLOAD_VERSION
            || !matches!(payload[1], OP_EXECUTE | OP_FUND | OP_ACTIVATE)
            || payload[2..4] != [0, 0] {
            return Err(SandboxActivityRefusal::NonCanonical);
        }
        let lease = LeaseId::new(payload[4..36].try_into()
            .map_err(|_| SandboxActivityRefusal::NonCanonical)?)
            .map_err(|_| SandboxActivityRefusal::NonCanonical)?;
        if payload[1] == OP_ACTIVATE {
            if payload.len() != 76 { return Err(SandboxActivityRefusal::NonCanonical); }
            let expected_lease_digest = payload[36..68].try_into()
                .map_err(|_| SandboxActivityRefusal::NonCanonical)?;
            let expected_usage_sequence = u64::from_be_bytes(payload[68..76].try_into()
                .map_err(|_| SandboxActivityRefusal::NonCanonical)?);
            if expected_lease_digest == [0; 32] || expected_usage_sequence == 0 {
                return Err(SandboxActivityRefusal::NonCanonical);
            }
            return Ok(Self { activity_type: PROGRAMS_SANDBOX_ACTIVITY_TYPE,
                payload: payload.to_vec(), lease, expected_usage_sequence, expected_lease_digest,
                protocol });
        }
        if payload[1] == OP_FUND {
            if payload.len() < 256 { return Err(SandboxActivityRefusal::NonCanonical); }
            let lease_length = usize::try_from(u32::from_be_bytes(payload[248..252].try_into()
                .map_err(|_| SandboxActivityRefusal::NonCanonical)?))
                .map_err(|_| SandboxActivityRefusal::LengthLimit)?;
            let lease_end = 252usize.checked_add(lease_length)
                .ok_or(SandboxActivityRefusal::LengthLimit)?;
            let transfer_length_end = lease_end.checked_add(4)
                .ok_or(SandboxActivityRefusal::LengthLimit)?;
            if lease_length == 0 || transfer_length_end > payload.len() {
                return Err(SandboxActivityRefusal::NonCanonical);
            }
            let transfer_length = usize::try_from(u32::from_be_bytes(
                payload[lease_end..transfer_length_end].try_into()
                    .map_err(|_| SandboxActivityRefusal::NonCanonical)?))
                .map_err(|_| SandboxActivityRefusal::LengthLimit)?;
            if transfer_length == 0 || transfer_length_end.checked_add(transfer_length) != Some(payload.len()) {
                return Err(SandboxActivityRefusal::NonCanonical);
            }
            let expected_lease_digest = hash_bytes(HashAlgorithm::Sha256, &payload[252..lease_end])
                .map_err(|_| SandboxActivityRefusal::StateDigest)?;
            return Ok(Self { activity_type: PROGRAMS_SANDBOX_ACTIVITY_TYPE,
                payload: payload.to_vec(), lease, expected_usage_sequence: 0, expected_lease_digest,
                protocol });
        }
        if payload.len() < FIXED_BYTES { return Err(SandboxActivityRefusal::NonCanonical); }
        let expected_usage_sequence = u64::from_be_bytes(payload[36..44].try_into()
            .map_err(|_| SandboxActivityRefusal::NonCanonical)?);
        let expected_lease_digest = payload[44..76].try_into()
            .map_err(|_| SandboxActivityRefusal::NonCanonical)?;
        if payload[76..108] == [0; 32] || payload[108..140] == [0; 32]
            || payload[140..172] == [0; 32] {
            return Err(SandboxActivityRefusal::NonCanonical);
        }
        let call_length = usize::try_from(u32::from_be_bytes(payload[232..236].try_into()
            .map_err(|_| SandboxActivityRefusal::NonCanonical)?))
            .map_err(|_| SandboxActivityRefusal::LengthLimit)?;
        if expected_usage_sequence == 0 || expected_lease_digest == [0; 32]
            || u32::from_be_bytes(payload[172..176].try_into()
                .map_err(|_| SandboxActivityRefusal::NonCanonical)?) == 0
            || call_length == 0 || call_length > MAX_CANONICAL_CALL_BYTES
            || FIXED_BYTES.checked_add(call_length) != Some(payload.len()) {
            return Err(SandboxActivityRefusal::NonCanonical);
        }
        validate_call_structure(&payload[FIXED_BYTES..], protocol)?;
        Ok(Self { activity_type: PROGRAMS_SANDBOX_ACTIVITY_TYPE, payload: payload.to_vec(),
            lease, expected_usage_sequence, expected_lease_digest, protocol })
    }
}

pub fn canonical_fund(
    protocol: SandboxProtocolContext, lease: &Lease, canonical_programs_transfer: &[u8],
) -> Result<SandboxSubmission, SandboxActivityRefusal> {
    if lease.state() != LeaseState::Requested || canonical_programs_transfer.len() < 146 {
        return Err(SandboxActivityRefusal::CallMismatch);
    }
    let leg_count = u16::from_be_bytes(canonical_programs_transfer[32..34].try_into()
        .map_err(|_| SandboxActivityRefusal::NonCanonical)?);
    if canonical_programs_transfer[..32] != lease.host_program().bytes() || leg_count != 1
        || canonical_programs_transfer.len() != 146
        || canonical_programs_transfer[34..66] != lease.tenant().bytes()
        || canonical_programs_transfer[66..98] != lease.escrow_asset()
        || canonical_programs_transfer[98..130] != lease.escrow_account()
        || u128::from_be_bytes(canonical_programs_transfer[130..146].try_into()
            .map_err(|_| SandboxActivityRefusal::NonCanonical)?) != lease.escrow_amount() {
        return Err(SandboxActivityRefusal::CallMismatch);
    }
    let lease_bytes = lease.canonical_state_bytes().map_err(|_| SandboxActivityRefusal::StateDigest)?;
    let mut payload = Vec::new();
    payload.extend_from_slice(&[PAYLOAD_VERSION, OP_FUND, 0, 0]);
    payload.extend_from_slice(&lease.id().bytes());
    payload.extend_from_slice(&lease.tenant().bytes());
    payload.extend_from_slice(&lease.host_program().bytes());
    payload.extend_from_slice(&lease.escrow_account());
    payload.extend_from_slice(&lease.escrow_asset());
    payload.extend_from_slice(&lease.escrow_amount().to_be_bytes());
    payload.extend_from_slice(&lease.expiry().to_be_bytes());
    let schedule = lease.fee_schedule();
    payload.extend_from_slice(&schedule.version().to_be_bytes());
    for price in [schedule.cpu_price(), schedule.memory_byte_price(), schedule.storage_read_byte_price(),
        schedule.storage_write_byte_price(), schedule.output_value_price(), schedule.output_byte_price(),
        schedule.occupancy_byte_batch_price()] { payload.extend_from_slice(&price.to_be_bytes()); }
    payload.extend_from_slice(&u32::try_from(lease_bytes.len()).map_err(|_| SandboxActivityRefusal::LengthLimit)?.to_be_bytes());
    payload.extend_from_slice(&lease_bytes);
    payload.extend_from_slice(&u32::try_from(canonical_programs_transfer.len()).map_err(|_| SandboxActivityRefusal::LengthLimit)?.to_be_bytes());
    payload.extend_from_slice(canonical_programs_transfer);
    SandboxSubmission::decode(&payload, protocol)
}

pub fn canonical_activate(protocol: SandboxProtocolContext, lease: &Lease)
    -> Result<SandboxSubmission, SandboxActivityRefusal> {
    if lease.state() != LeaseState::Funded { return Err(SandboxActivityRefusal::LeaseNotActive); }
    let digest = lease.state_digest().map_err(|_| SandboxActivityRefusal::StateDigest)?;
    let next = u64::try_from(lease.history().len()).map_err(|_| SandboxActivityRefusal::LengthLimit)?
        .checked_add(1).ok_or(SandboxActivityRefusal::LengthLimit)?;
    let mut payload = Vec::with_capacity(76);
    payload.extend_from_slice(&[PAYLOAD_VERSION, OP_ACTIVATE, 0, 0]);
    payload.extend_from_slice(&lease.id().bytes());
    payload.extend_from_slice(&digest);
    payload.extend_from_slice(&next.to_be_bytes());
    SandboxSubmission::decode(&payload, protocol)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SandboxSubmissionReceipt {
    activity_id: [u8; 32],
    canonical_receipt: Vec<u8>,
    lease: LeaseId,
    expected_usage_sequence: u64,
    expected_lease_digest: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SandboxReceiptEvidence {
    pub canonical_activity: Vec<u8>,
    pub activity_proof: Proof,
    pub canonical_receipt: Vec<u8>,
    pub receipt_proof: Proof,
    pub canonical_header: Vec<u8>,
    pub header_signature: [u8; 64],
}

impl SandboxSubmissionReceipt {
    pub fn verify(
        verifier: &ProtocolDeploymentVerifier, submission: &SandboxSubmission,
        evidence: SandboxReceiptEvidence, now_ms: u64,
    ) -> Result<Self, SandboxActivityRefusal> {
        let head = verifier.verify_current_protocol_head(
            &evidence.canonical_receipt, &evidence.receipt_proof,
            &evidence.canonical_header, &evidence.header_signature, now_ms,
        ).map_err(|_| SandboxActivityRefusal::InvalidReceipt)?;
        verify_submission_activity(&head, submission, &evidence)?;
        Ok(Self { activity_id: head.activity_id(), canonical_receipt: evidence.canonical_receipt,
            lease: submission.lease, expected_usage_sequence: submission.expected_usage_sequence,
            expected_lease_digest: submission.expected_lease_digest })
    }
    #[must_use] pub const fn activity_id(&self) -> [u8; 32] { self.activity_id }
    #[must_use] pub fn canonical_receipt(&self) -> &[u8] { &self.canonical_receipt }
    #[must_use] pub const fn lease(&self) -> LeaseId { self.lease }
    #[must_use] pub const fn expected_usage_sequence(&self) -> u64 { self.expected_usage_sequence }
    #[must_use] pub const fn expected_lease_digest(&self) -> [u8; 32] { self.expected_lease_digest }

    /// Recovers the usage receipt from the already authenticated canonical
    /// protocol receipt. The returned type is the only public input accepted
    /// by historical ledger verification.
    pub fn authenticated_usage_receipt(&self)
        -> Result<AuthenticatedUsageReceipt, SandboxActivityRefusal> {
        if self.expected_usage_sequence == 0 {
            return Err(SandboxActivityRefusal::InvalidReceipt);
        }
        let decoded = decode_receipt(&self.canonical_receipt)
            .map_err(|_| SandboxActivityRefusal::InvalidReceipt)?;
        let protocol = decoded.protocol().ok_or(SandboxActivityRefusal::InvalidReceipt)?;
        if protocol.activity_id() != self.activity_id {
            return Err(SandboxActivityRefusal::InvalidReceipt);
        }
        let mut chunks: Vec<Option<Vec<u8>>> = Vec::new();
        let mut total = None;
        for effect in protocol.effects().iter().filter(|effect|
            effect.module_id() == ModuleId::Programs as u16
                && effect.event_type() == SANDBOX_USAGE_EVENT) {
            let body = effect.body();
            if effect.kind() != SANDBOX_USAGE_EVENT_KIND
                || body.len() < SANDBOX_USAGE_FRAME_BYTES
                || &body[..4] != SANDBOX_USAGE_MAGIC
                || body[4..36] != self.activity_id
                || u64::from_be_bytes(body[36..44].try_into()
                    .map_err(|_| SandboxActivityRefusal::InvalidReceipt)?)
                    != self.expected_usage_sequence {
                return Err(SandboxActivityRefusal::InvalidReceipt);
            }
            let index = usize::from(u16::from_be_bytes(body[44..46].try_into()
                .map_err(|_| SandboxActivityRefusal::InvalidReceipt)?));
            let count = usize::from(u16::from_be_bytes(body[46..48].try_into()
                .map_err(|_| SandboxActivityRefusal::InvalidReceipt)?));
            let declared_total = usize::try_from(u32::from_be_bytes(body[48..52].try_into()
                .map_err(|_| SandboxActivityRefusal::InvalidReceipt)?))
                .map_err(|_| SandboxActivityRefusal::LengthLimit)?;
            if count == 0 || index >= count || declared_total <= 32
                || declared_total > MAX_USAGE_STATE_VALUE_BYTES
                || count != (declared_total + SANDBOX_USAGE_CHUNK_BYTES - 1)
                    / SANDBOX_USAGE_CHUNK_BYTES
                || body.len() - SANDBOX_USAGE_FRAME_BYTES
                    != (declared_total - index * SANDBOX_USAGE_CHUNK_BYTES)
                        .min(SANDBOX_USAGE_CHUNK_BYTES)
                || total.is_some_and(|known| known != declared_total)
                || (!chunks.is_empty() && chunks.len() != count) {
                return Err(SandboxActivityRefusal::InvalidReceipt);
            }
            total = Some(declared_total);
            if chunks.is_empty() { chunks.resize_with(count, || None); }
            if chunks[index].replace(body[SANDBOX_USAGE_FRAME_BYTES..].to_vec()).is_some() {
                return Err(SandboxActivityRefusal::InvalidReceipt);
            }
        }
        let total = total.ok_or(SandboxActivityRefusal::InvalidReceipt)?;
        let mut framed = Vec::with_capacity(total);
        for chunk in chunks {
            framed.extend_from_slice(&chunk.ok_or(SandboxActivityRefusal::InvalidReceipt)?);
        }
        if framed.len() != total {
            return Err(SandboxActivityRefusal::InvalidReceipt);
        }
        let digest: [u8; 32] = framed[total - 32..].try_into()
            .map_err(|_| SandboxActivityRefusal::InvalidReceipt)?;
        let usage = UsageReceipt::decode(&framed[..total - 32], digest)
            .map_err(|_| SandboxActivityRefusal::InvalidReceipt)?;
        if usage.activity_id() != self.activity_id || usage.lease() != self.lease
            || usage.sequence() != self.expected_usage_sequence
            || usage.expected_lease_digest() != self.expected_lease_digest {
            return Err(SandboxActivityRefusal::InvalidReceipt);
        }
        Ok(AuthenticatedUsageReceipt::new(usage))
    }
}

pub trait SandboxActivityPlane {
    type Error;
    fn submit_sandbox(&mut self, submission: SandboxSubmission)
        -> Result<SandboxReceiptEvidence, Self::Error>;
}

#[derive(Debug, Eq, PartialEq)]
pub enum SandboxExecuteRefusal<E> {
    Activity(SandboxActivityRefusal),
    Submission(E),
}

pub fn execute<P: SandboxActivityPlane>(
    plane: &mut P, verifier: &ProtocolDeploymentVerifier, now_ms: u64,
    protocol: SandboxProtocolContext,
    lease: &Lease,
    expected_usage_sequence: u64,
    canonical_programs_call: &[u8],
) -> Result<SandboxSubmissionReceipt, SandboxExecuteRefusal<P::Error>> {
    let submission = canonical_execute(protocol, lease, expected_usage_sequence, canonical_programs_call)
        .map_err(SandboxExecuteRefusal::Activity)?;
    let evidence = plane.submit_sandbox(submission.clone()).map_err(SandboxExecuteRefusal::Submission)?;
    SandboxSubmissionReceipt::verify(verifier, &submission, evidence, now_ms)
        .map_err(SandboxExecuteRefusal::Activity)
}

pub fn fund_lease<P: SandboxActivityPlane>(
    plane: &mut P, verifier: &ProtocolDeploymentVerifier, now_ms: u64,
    protocol: SandboxProtocolContext,
    lease: &Lease, canonical_programs_transfer: &[u8],
) -> Result<SandboxSubmissionReceipt, SandboxExecuteRefusal<P::Error>> {
    let submission = canonical_fund(protocol, lease, canonical_programs_transfer)
        .map_err(SandboxExecuteRefusal::Activity)?;
    let evidence = plane.submit_sandbox(submission.clone()).map_err(SandboxExecuteRefusal::Submission)?;
    SandboxSubmissionReceipt::verify(verifier, &submission, evidence, now_ms)
        .map_err(SandboxExecuteRefusal::Activity)
}

pub fn activate_lease<P: SandboxActivityPlane>(
    plane: &mut P, verifier: &ProtocolDeploymentVerifier, now_ms: u64,
    protocol: SandboxProtocolContext, lease: &Lease,
) -> Result<SandboxSubmissionReceipt, SandboxExecuteRefusal<P::Error>> {
    let submission = canonical_activate(protocol, lease).map_err(SandboxExecuteRefusal::Activity)?;
    let evidence = plane.submit_sandbox(submission.clone()).map_err(SandboxExecuteRefusal::Submission)?;
    SandboxSubmissionReceipt::verify(verifier, &submission, evidence, now_ms)
        .map_err(SandboxExecuteRefusal::Activity)
}

fn verify_submission_activity(
    head: &VerifiedProtocolHead, submission: &SandboxSubmission,
    evidence: &SandboxReceiptEvidence,
) -> Result<(), SandboxActivityRefusal> {
    let header = decode_batch_header(&evidence.canonical_header)
        .map_err(|_| SandboxActivityRefusal::InvalidReceipt)?;
    if encode_batch_header(&header).map_err(|_| SandboxActivityRefusal::InvalidReceipt)?
            != evidence.canonical_header
        || batch_header_digest(&evidence.canonical_header)
            .map_err(|_| SandboxActivityRefusal::InvalidReceipt)? != head.batch_header_digest() {
        return Err(SandboxActivityRefusal::InvalidReceipt);
    }
    verify_path(&evidence.canonical_activity, &evidence.activity_proof,
        &header.activity_merkle_root()).map_err(|_| SandboxActivityRefusal::InvalidReceipt)?;
    let sandbox = ActivityType::new(ModuleId::Programs, 9)
        .map_err(|_| SandboxActivityRefusal::InvalidReceipt)?;
    let registration = ModuleRegistration::new(ModuleId::Programs, &[sandbox])
        .map_err(|_| SandboxActivityRefusal::InvalidReceipt)?;
    let registry = ModuleRegistry::new(&[registration])
        .map_err(|_| SandboxActivityRefusal::InvalidReceipt)?;
    let activity = decode_signed(&evidence.canonical_activity, &registry)
        .map_err(|_| SandboxActivityRefusal::InvalidReceipt)?;
    let identifier = activity_id(&activity).map_err(|_| SandboxActivityRefusal::InvalidReceipt)?;
    if encode_signed(&activity).map_err(|_| SandboxActivityRefusal::InvalidReceipt)?
            != evidence.canonical_activity
        || payload_hash(&activity).map_err(|_| SandboxActivityRefusal::InvalidReceipt)?
            != activity.payload_hash()
        || activity.activity_type() != sandbox || activity.payload() != submission.payload
        || identifier != head.activity_id() {
        return Err(SandboxActivityRefusal::InvalidReceipt);
    }
    let decoded = SandboxSubmission::decode(activity.payload(), submission.protocol)?;
    if decoded.lease != submission.lease
        || decoded.expected_lease_digest != submission.expected_lease_digest
        || decoded.expected_usage_sequence != submission.expected_usage_sequence {
        return Err(SandboxActivityRefusal::InvalidReceipt);
    }
    Ok(())
}

pub fn canonical_execute(
    protocol: SandboxProtocolContext,
    lease: &Lease,
    expected_usage_sequence: u64,
    canonical_programs_call: &[u8],
) -> Result<SandboxSubmission, SandboxActivityRefusal> {
    if lease.state() != LeaseState::Active { return Err(SandboxActivityRefusal::LeaseNotActive); }
    if expected_usage_sequence == 0 || canonical_programs_call.is_empty()
        || canonical_programs_call.len() > MAX_CANONICAL_CALL_BYTES {
        return Err(SandboxActivityRefusal::LengthLimit);
    }
    validate_lease_call(lease, canonical_programs_call, protocol)?;
    let call_length = u32::try_from(canonical_programs_call.len())
        .map_err(|_| SandboxActivityRefusal::LengthLimit)?;
    let expected_lease_digest = lease.state_digest()
        .map_err(|_| SandboxActivityRefusal::StateDigest)?;
    let mut payload = Vec::with_capacity(FIXED_BYTES + canonical_programs_call.len());
    payload.extend_from_slice(&[PAYLOAD_VERSION, OP_EXECUTE, 0, 0]);
    payload.extend_from_slice(&lease.id().bytes());
    payload.extend_from_slice(&expected_usage_sequence.to_be_bytes());
    payload.extend_from_slice(&expected_lease_digest);
    payload.extend_from_slice(&lease.escrow_account());
    payload.extend_from_slice(&lease.escrow_asset());
    payload.extend_from_slice(&lease.fee_destination());
    let schedule = lease.fee_schedule();
    payload.extend_from_slice(&schedule.version().to_be_bytes());
    for price in [schedule.cpu_price(), schedule.memory_byte_price(),
        schedule.storage_read_byte_price(), schedule.storage_write_byte_price(),
        schedule.output_value_price(), schedule.output_byte_price(),
        schedule.occupancy_byte_batch_price()] {
        payload.extend_from_slice(&price.to_be_bytes());
    }
    payload.extend_from_slice(&call_length.to_be_bytes());
    payload.extend_from_slice(canonical_programs_call);
    let decoded = SandboxSubmission::decode(&payload, protocol)?;
    if decoded.payload != payload { return Err(SandboxActivityRefusal::NonCanonical); }
    Ok(decoded)
}

fn validate_lease_call(lease: &Lease, call: &[u8], protocol: SandboxProtocolContext)
    -> Result<(), SandboxActivityRefusal> {
    if call.len() < CALL_FIXED_BYTES || call[..32] != lease.host_program().bytes() {
        return Err(SandboxActivityRefusal::CallMismatch);
    }
    let declared = validate_call_structure(call, protocol)?;
    let limits = lease.limits();
    let ceilings = [limits.cpu_fuel, limits.memory_bytes, limits.storage_read_bytes,
        limits.storage_write_bytes, limits.output_values, limits.output_bytes,
        limits.table_elements];
    if declared.into_iter().zip(ceilings).any(|(value, ceiling)| value > ceiling) {
        return Err(SandboxActivityRefusal::CallMismatch);
    }
    Ok(())
}

fn validate_call_structure(call: &[u8], protocol: SandboxProtocolContext)
    -> Result<[u64; 7], SandboxActivityRefusal> {
    if call.len() < CALL_FIXED_BYTES || call[..32] == [0; 32]
        || u16::from_be_bytes(call[32..34].try_into()
            .map_err(|_| SandboxActivityRefusal::NonCanonical)?) == 0 {
        return Err(SandboxActivityRefusal::NonCanonical);
    }
    let abi_version = u16::from_be_bytes(call[32..34].try_into()
        .map_err(|_| SandboxActivityRefusal::NonCanonical)?);
    if abi_version > protocol.active_programs_abi
        || abi_version > PROGRAMS_ACCOUNT_ABI_VERSION
        || (abi_version == PROGRAMS_ACCOUNT_ABI_VERSION
            && protocol.protocol_version != OCCUPANCY_PROTOCOL_VERSION) {
        return Err(SandboxActivityRefusal::ProtocolMismatch);
    }
    let entrypoint_length = usize::from(u16::from_be_bytes(call[34..36].try_into()
        .map_err(|_| SandboxActivityRefusal::NonCanonical)?));
    let calldata_length = usize::try_from(u32::from_be_bytes(call[36..40].try_into()
        .map_err(|_| SandboxActivityRefusal::NonCanonical)?))
        .map_err(|_| SandboxActivityRefusal::LengthLimit)?;
    let capabilities_length = usize::from(u16::from_be_bytes(call[40..42].try_into()
        .map_err(|_| SandboxActivityRefusal::NonCanonical)?));
    let access_length = usize::try_from(u32::from_be_bytes(call[42..46].try_into()
        .map_err(|_| SandboxActivityRefusal::NonCanonical)?))
        .map_err(|_| SandboxActivityRefusal::LengthLimit)?;
    let response_capacity = usize::try_from(u32::from_be_bytes(call[46..50].try_into()
        .map_err(|_| SandboxActivityRefusal::NonCanonical)?))
        .map_err(|_| SandboxActivityRefusal::LengthLimit)?;
    let capabilities_start = CALL_FIXED_BYTES.checked_add(entrypoint_length)
        .and_then(|value| value.checked_add(calldata_length))
        .ok_or(SandboxActivityRefusal::LengthLimit)?;
    let access_start = capabilities_start.checked_add(capabilities_length)
        .ok_or(SandboxActivityRefusal::LengthLimit)?;
    let entrypoint = call.get(CALL_FIXED_BYTES..CALL_FIXED_BYTES.checked_add(entrypoint_length)
        .ok_or(SandboxActivityRefusal::LengthLimit)?)
        .ok_or(SandboxActivityRefusal::NonCanonical)?;
    if entrypoint.is_empty() || entrypoint.len() > 128
        || entrypoint.iter().any(|byte| !byte.is_ascii_alphanumeric() && !matches!(byte, b'_' | b'.'))
        || calldata_length > 1_048_576 || capabilities_length > 65_535
        || access_length > 1_048_576 || response_capacity > 1_048_576
        || access_start.checked_add(access_length) != Some(call.len())
        || call.get(capabilities_start..access_start) != Some(LEASE_STORAGE_CAPABILITIES) {
        return Err(SandboxActivityRefusal::CallMismatch);
    }
    let declared = [
        u64::from_be_bytes(call[50..58].try_into().map_err(|_| SandboxActivityRefusal::NonCanonical)?),
        u64::from_be_bytes(call[58..66].try_into().map_err(|_| SandboxActivityRefusal::NonCanonical)?),
        u64::from_be_bytes(call[66..74].try_into().map_err(|_| SandboxActivityRefusal::NonCanonical)?),
        u64::from_be_bytes(call[74..82].try_into().map_err(|_| SandboxActivityRefusal::NonCanonical)?),
        u64::from_be_bytes(call[82..90].try_into().map_err(|_| SandboxActivityRefusal::NonCanonical)?),
        u64::from_be_bytes(call[90..98].try_into().map_err(|_| SandboxActivityRefusal::NonCanonical)?),
        u64::from_be_bytes(call[98..106].try_into().map_err(|_| SandboxActivityRefusal::NonCanonical)?),
    ];
    if declared.contains(&0) {
        return Err(SandboxActivityRefusal::CallMismatch);
    }
    Ok(declared)
}

pub fn canonical_submission_digest(submission: &SandboxSubmission)
    -> Result<[u8; 32], SandboxActivityRefusal> {
    let mut bytes = b"LayerX/programs/sandbox/submission/v1\0".to_vec();
    bytes.extend_from_slice(&submission.activity_type.to_be_bytes());
    bytes.extend_from_slice(&submission.payload);
    hash_bytes(HashAlgorithm::Sha256, &bytes).map_err(|_| SandboxActivityRefusal::StateDigest)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SandboxActivityRefusal {
    NonCanonical, LengthLimit, LeaseNotActive, StateDigest, InvalidReceipt, CallMismatch,
    ProtocolMismatch,
}

impl Display for SandboxActivityRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result { write!(formatter, "{self:?}") }
}

impl std::error::Error for SandboxActivityRefusal {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_and_replayed_sequences_are_not_canonical_submissions() {
        let protocol = SandboxProtocolContext::new(2, 3).expect("protocol");
        let mut call = vec![0; CALL_FIXED_BYTES];
        call[..32].copy_from_slice(&[9; 32]);
        call[32..34].copy_from_slice(&2u16.to_be_bytes());
        call[34..36].copy_from_slice(&1u16.to_be_bytes());
        call[40..42].copy_from_slice(&4u16.to_be_bytes());
        call[46..50].copy_from_slice(&1u32.to_be_bytes());
        for index in 0..7 { call[50 + index * 8..58 + index * 8].copy_from_slice(&1u64.to_be_bytes()); }
        call.push(b'x');
        call.extend_from_slice(LEASE_STORAGE_CAPABILITIES);
        let mut payload = vec![0; FIXED_BYTES];
        payload[0] = PAYLOAD_VERSION;
        payload[1] = OP_EXECUTE;
        payload[4..36].copy_from_slice(&[1; 32]);
        payload[44..76].copy_from_slice(&[2; 32]);
        payload[76..108].copy_from_slice(&[3; 32]);
        payload[108..140].copy_from_slice(&[4; 32]);
        payload[140..172].copy_from_slice(&[5; 32]);
        payload[172..176].copy_from_slice(&1u32.to_be_bytes());
        payload[232..236].copy_from_slice(&(call.len() as u32).to_be_bytes());
        payload.extend_from_slice(&call);
        assert_eq!(SandboxSubmission::decode(&payload, protocol), Err(SandboxActivityRefusal::NonCanonical));
        payload[36..44].copy_from_slice(&1u64.to_be_bytes());
        assert!(SandboxSubmission::decode(&payload, protocol).is_ok());
        assert_eq!(SandboxSubmission::decode(&payload,
            SandboxProtocolContext { protocol_version: 1, active_programs_abi: 3 }),
            Err(SandboxActivityRefusal::ProtocolMismatch));
        let capability = FIXED_BYTES + CALL_FIXED_BYTES + 1;
        payload[capability] = 1;
        assert_eq!(SandboxSubmission::decode(&payload, protocol), Err(SandboxActivityRefusal::CallMismatch));
    }
}
