//! Receipt-backed protocol state for bounded sandbox leases.

use core::fmt::{self, Display};
use std::collections::BTreeMap;

use layerx_programs_runtime::{derive_program_account, hash_bytes, AuthorizationContext, CodeHash, HashAlgorithm, PrincipalId, ProgramId, ResourceBudget, StorageNamespace};
use layerx_programs::VerifiedProtocolHead;
use layerx_proof::merkle::{verify_path, Proof};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_wire::activity::{decode_signed, encode_signed};
use layerx_wire::hash::{activity_id, batch_header_digest, payload_hash};
use layerx_wire::receipt::{decode_batch_header, encode_batch_header};

const NAMESPACE_DOMAIN: &[u8] = b"LayerX/programs/sandbox/namespace/v1\0";
const USAGE_OBSERVATION_DOMAIN: &[u8] = b"LayerX/programs/sandbox/usage-observation/v1\0";
const SANDBOX_TRANSITION_ENTRYPOINT: &[u8] = b"sandbox_transition";
const PROGRAMS_CALL_ORDINAL: u16 = 3;
const PROGRAMS_CALL_FIXED_BYTES: usize = 106;
const TRANSITION_CALLDATA_BYTES: usize = 101;
const ESCROW_SEED_DOMAIN: &[u8] = b"sandbox-lease-escrow/v1\0";
const LEASE_STATE_DOMAIN: &[u8] = b"LayerX/programs/sandbox/lease-state/v1\0";
const MAX_LEASE_TRANSITIONS: usize = 6;

pub const MAX_CONCURRENT_LEASES_PER_PRINCIPAL: u32 = 32;
pub const MAX_LEASE_CPU_FUEL: u64 = 1_000_000_000;
pub const MAX_LEASE_MEMORY_BYTES: u64 = 1 << 30;
pub const MAX_LEASE_STORAGE_READ_BYTES: u64 = 1 << 30;
pub const MAX_LEASE_STORAGE_WRITE_BYTES: u64 = 1 << 30;
pub const MAX_LEASE_OUTPUT_VALUES: u64 = 65_536;
pub const MAX_LEASE_OUTPUT_BYTES: u64 = 1 << 30;
pub const MAX_LEASE_TABLE_ELEMENTS: u64 = 1 << 20;
pub const MAX_LEASE_NAMESPACE_BYTES: u64 = 1 << 30;
pub const MAX_LEASE_LIFETIME_BATCHES: u64 = 1_000_000;
pub const MAX_LEASE_ESCROW: u128 = 1_000_000_000_000_000_000_000_000;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct LeaseId([u8; 32]);

impl LeaseId {
    pub fn new(bytes: [u8; 32]) -> Result<Self, LeaseRefusal> {
        if bytes == [0; 32] { return Err(LeaseRefusal::ReservedIdentifier); }
        Ok(Self(bytes))
    }

    #[must_use]
    pub const fn bytes(self) -> [u8; 32] { self.0 }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct EphemeralNamespace { host: ProgramId, lease: LeaseId, prefix: [u8; 32] }

impl EphemeralNamespace {
    #[must_use]
    pub fn derive(host: ProgramId, lease: LeaseId) -> Result<Self, LeaseRefusal> {
        let mut preimage = Vec::with_capacity(NAMESPACE_DOMAIN.len() + 64);
        preimage.extend_from_slice(NAMESPACE_DOMAIN);
        preimage.extend_from_slice(&host.bytes());
        preimage.extend_from_slice(&lease.bytes());
        Ok(Self { host, lease, prefix: hash_bytes(HashAlgorithm::Sha256, &preimage)
            .map_err(|_| LeaseRefusal::HashRefusal)? })
    }

    #[must_use]
    pub const fn bytes(self) -> [u8; 32] { self.prefix }

    #[must_use] pub const fn host(self) -> ProgramId { self.host }
    #[must_use] pub const fn lease(self) -> LeaseId { self.lease }

    pub fn storage_namespace(self) -> Result<StorageNamespace, LeaseRefusal> {
        let principal = PrincipalId::new(self.prefix).map_err(|_| LeaseRefusal::HashRefusal)?;
        Ok(StorageNamespace::principal(self.host, principal))
    }

    pub fn execution_principal(self) -> Result<PrincipalId, LeaseRefusal> {
        PrincipalId::new(self.prefix).map_err(|_| LeaseRefusal::HashRefusal)
    }

    #[must_use] pub const fn key_prefix(self) -> [u8; 32] { self.prefix }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LeaseLimits {
    pub cpu_fuel: u64,
    pub memory_bytes: u64,
    pub storage_read_bytes: u64,
    pub storage_write_bytes: u64,
    pub output_values: u64,
    pub output_bytes: u64,
    pub table_elements: u64,
    pub namespace_bytes: u64,
}

impl LeaseLimits {
    #[must_use]
    pub fn from_execution_budget(budget: ResourceBudget, namespace_bytes: u64) -> Self {
        Self {
            cpu_fuel: budget.cpu_fuel(), memory_bytes: budget.memory_bytes(),
            storage_read_bytes: budget.storage_read_bytes(),
            storage_write_bytes: budget.storage_write_bytes(),
            output_values: u64::from(budget.output_values()), output_bytes: budget.output_bytes(),
            table_elements: u64::from(budget.table_elements()), namespace_bytes,
        }
    }

    pub fn validate(self) -> Result<Self, LeaseRefusal> {
        let checks = [
            (BoundKind::CpuFuel, self.cpu_fuel, MAX_LEASE_CPU_FUEL),
            (BoundKind::MemoryBytes, self.memory_bytes, MAX_LEASE_MEMORY_BYTES),
            (BoundKind::StorageReadBytes, self.storage_read_bytes, MAX_LEASE_STORAGE_READ_BYTES),
            (BoundKind::StorageWriteBytes, self.storage_write_bytes, MAX_LEASE_STORAGE_WRITE_BYTES),
            (BoundKind::OutputValues, self.output_values, MAX_LEASE_OUTPUT_VALUES),
            (BoundKind::OutputBytes, self.output_bytes, MAX_LEASE_OUTPUT_BYTES),
            (BoundKind::TableElements, self.table_elements, MAX_LEASE_TABLE_ELEMENTS),
            (BoundKind::NamespaceBytes, self.namespace_bytes, MAX_LEASE_NAMESPACE_BYTES),
        ];
        for (bound, declared, maximum) in checks {
            if declared > maximum {
                return Err(LeaseRefusal::InvalidDeclaredBound { bound, declared, maximum });
            }
        }
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LeaseUsage {
    pub cpu_fuel: u64,
    /// Maximum simultaneously resident linear-memory bytes observed so far.
    pub memory_bytes: u64,
    pub storage_read_bytes: u64,
    pub storage_write_bytes: u64,
    pub output_values: u64,
    pub output_bytes: u64,
    pub table_elements: u64,
    pub namespace_bytes: u64,
}

impl LeaseUsage {
    fn first_exceeded(self, limits: LeaseLimits) -> Option<(BoundKind, u128, u128)> {
        [
            (BoundKind::CpuFuel, self.cpu_fuel, limits.cpu_fuel),
            (BoundKind::MemoryBytes, self.memory_bytes, limits.memory_bytes),
            (BoundKind::StorageReadBytes, self.storage_read_bytes, limits.storage_read_bytes),
            (BoundKind::StorageWriteBytes, self.storage_write_bytes, limits.storage_write_bytes),
            (BoundKind::OutputValues, self.output_values, limits.output_values),
            (BoundKind::OutputBytes, self.output_bytes, limits.output_bytes),
            (BoundKind::TableElements, self.table_elements, limits.table_elements),
            (BoundKind::NamespaceBytes, self.namespace_bytes, limits.namespace_bytes),
        ].into_iter().find(|(_, consumed, limit)| consumed > limit)
            .map(|(bound, consumed, limit)| (bound, u128::from(consumed), u128::from(limit)))
    }

    fn regressed_from(self, prior: Self) -> bool {
        self.cpu_fuel < prior.cpu_fuel
            || self.memory_bytes < prior.memory_bytes
            || self.storage_read_bytes < prior.storage_read_bytes
            || self.storage_write_bytes < prior.storage_write_bytes
            || self.output_values < prior.output_values
            || self.output_bytes < prior.output_bytes
            || self.table_elements < prior.table_elements
            || self.namespace_bytes < prior.namespace_bytes
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BoundKind {
    CpuFuel,
    MemoryBytes,
    StorageReadBytes,
    StorageWriteBytes,
    OutputValues,
    OutputBytes,
    TableElements,
    NamespaceBytes,
    LifetimeBatches,
    Escrow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum LeaseState { Requested = 0, Funded = 1, Active = 2, Settling = 3, Expired = 4, Destroyed = 5 }

impl LeaseState {
    #[must_use]
    pub const fn is_terminal(self) -> bool { matches!(self, Self::Destroyed) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum LeaseActivity { Request = 0, Fund = 1, Activate = 2, BeginSettlement = 3, Expire = 4, Destroy = 5, CloseBoundExceeded = 6 }

const fn declared_edge(activity: LeaseActivity, from: LeaseState, to: LeaseState) -> bool {
    matches!((activity, from, to),
        (LeaseActivity::Request, LeaseState::Requested, LeaseState::Requested)
        | (LeaseActivity::Fund, LeaseState::Requested, LeaseState::Funded)
        | (LeaseActivity::Activate, LeaseState::Funded, LeaseState::Active)
        | (LeaseActivity::BeginSettlement, LeaseState::Active, LeaseState::Settling)
        | (LeaseActivity::CloseBoundExceeded, LeaseState::Active, LeaseState::Settling)
        | (LeaseActivity::Expire, LeaseState::Requested, LeaseState::Expired)
        | (LeaseActivity::Expire, LeaseState::Funded, LeaseState::Expired)
        | (LeaseActivity::Expire, LeaseState::Active, LeaseState::Expired)
        | (LeaseActivity::Expire, LeaseState::Settling, LeaseState::Expired)
        | (LeaseActivity::Destroy, LeaseState::Expired, LeaseState::Destroyed))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LeaseTransition {
    pub lease: LeaseId,
    pub tenant: PrincipalId,
    pub activity: LeaseActivity,
    pub from: LeaseState,
    pub to: LeaseState,
    pub activity_id: [u8; 32],
    pub usage_observation_digest: [u8; 32],
}

#[must_use]
pub fn usage_observation_digest(
    lease: LeaseId, usage: LeaseUsage, escrow_consumed: u128, observed_batch: u64,
) -> Result<[u8; 32], LeaseRefusal> {
    let mut preimage = Vec::new();
    preimage.extend_from_slice(USAGE_OBSERVATION_DOMAIN);
    preimage.extend_from_slice(&lease.bytes());
    for value in [usage.cpu_fuel, usage.memory_bytes, usage.storage_read_bytes,
        usage.storage_write_bytes, usage.output_values, usage.output_bytes,
        usage.table_elements, usage.namespace_bytes, observed_batch] {
        preimage.extend_from_slice(&value.to_be_bytes());
    }
    preimage.extend_from_slice(&escrow_consumed.to_be_bytes());
    hash_bytes(HashAlgorithm::Sha256, &preimage).map_err(|_| LeaseRefusal::HashRefusal)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransitionEvidence {
    activity_id: [u8; 32],
    receipt_digest: [u8; 32],
    batch_sequence: u64,
    declared_transition: LeaseTransition,
    invoking_principal: PrincipalId,
}

impl TransitionEvidence {
    pub fn verify_call(
        head: &VerifiedProtocolHead, lease: &Lease, transition: LeaseTransition,
        authorization: &AuthorizationContext,
        canonical_activity: &[u8], activity_proof: &Proof, canonical_header: &[u8],
    ) -> Result<Self, LeaseRefusal> {
        let header = decode_batch_header(canonical_header)
            .map_err(|_| LeaseRefusal::InvalidCanonicalEvidence)?;
        if encode_batch_header(&header).map_err(|_| LeaseRefusal::InvalidCanonicalEvidence)?
            != canonical_header
            || batch_header_digest(canonical_header).map_err(|_| LeaseRefusal::InvalidCanonicalEvidence)?
                != head.batch_header_digest()
        {
            return Err(LeaseRefusal::InvalidCanonicalEvidence);
        }
        verify_path(canonical_activity, activity_proof, &header.activity_merkle_root())
            .map_err(|_| LeaseRefusal::InvalidCanonicalEvidence)?;
        let call = ActivityType::new(ModuleId::Programs, PROGRAMS_CALL_ORDINAL)
            .map_err(|_| LeaseRefusal::InvalidCanonicalEvidence)?;
        let registration = ModuleRegistration::new(ModuleId::Programs, &[call])
            .map_err(|_| LeaseRefusal::InvalidCanonicalEvidence)?;
        let registry = ModuleRegistry::new(&[registration])
            .map_err(|_| LeaseRefusal::InvalidCanonicalEvidence)?;
        let activity = decode_signed(canonical_activity, &registry)
            .map_err(|_| LeaseRefusal::InvalidCanonicalEvidence)?;
        if encode_signed(&activity).map_err(|_| LeaseRefusal::InvalidCanonicalEvidence)?
                != canonical_activity
            || payload_hash(&activity).map_err(|_| LeaseRefusal::InvalidCanonicalEvidence)?
                != activity.payload_hash()
        {
            return Err(LeaseRefusal::InvalidCanonicalEvidence);
        }
        let identifier = activity_id(&activity).map_err(|_| LeaseRefusal::InvalidCanonicalEvidence)?;
        if identifier != head.activity_id() || identifier != transition.activity_id {
            return Err(LeaseRefusal::ActivityReceiptMismatch);
        }
        verify_transition_call(activity.payload(), lease, transition)?;
        if authorization.principal() != lease.tenant || transition.tenant != authorization.principal() {
            return Err(LeaseRefusal::TenantMismatch);
        }
        Ok(Self {
            activity_id: identifier,
            receipt_digest: head.receipt_digest(),
            batch_sequence: header.batch_number(),
            declared_transition: transition,
            invoking_principal: authorization.principal(),
        })
    }
}

fn verify_transition_call(
    payload: &[u8], lease: &Lease, transition: LeaseTransition,
) -> Result<(), LeaseRefusal> {
    if payload.len() < PROGRAMS_CALL_FIXED_BYTES { return Err(LeaseRefusal::InvalidCanonicalEvidence); }
    let entrypoint_length = usize::from(u16::from_be_bytes([payload[34], payload[35]]));
    let calldata_length = usize::try_from(u32::from_be_bytes(payload[36..40].try_into()
        .map_err(|_| LeaseRefusal::InvalidCanonicalEvidence)?))
        .map_err(|_| LeaseRefusal::InvalidCanonicalEvidence)?;
    let entrypoint_end = PROGRAMS_CALL_FIXED_BYTES.checked_add(entrypoint_length)
        .ok_or(LeaseRefusal::InvalidCanonicalEvidence)?;
    let calldata_end = entrypoint_end.checked_add(calldata_length)
        .ok_or(LeaseRefusal::InvalidCanonicalEvidence)?;
    if payload.get(..32) != Some(lease.host_program.bytes().as_slice())
        || payload.get(PROGRAMS_CALL_FIXED_BYTES..entrypoint_end) != Some(SANDBOX_TRANSITION_ENTRYPOINT)
        || payload.get(entrypoint_end..calldata_end) != Some(transition_calldata(transition).as_slice())
    {
        return Err(LeaseRefusal::ActivityReceiptMismatch);
    }
    Ok(())
}

fn transition_calldata(transition: LeaseTransition) -> [u8; TRANSITION_CALLDATA_BYTES] {
    let mut bytes = [0u8; TRANSITION_CALLDATA_BYTES];
    bytes[..2].copy_from_slice(&1u16.to_be_bytes());
    bytes[2..34].copy_from_slice(&transition.lease.bytes());
    bytes[34..66].copy_from_slice(&transition.tenant.bytes());
    bytes[66] = transition.activity as u8;
    bytes[67] = transition.from as u8;
    bytes[68] = transition.to as u8;
    bytes[69..101].copy_from_slice(&transition.usage_observation_digest);
    bytes
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LeaseTransitionReceipt {
    pub lease: LeaseId,
    pub transition: LeaseTransition,
    pub receipt_digest: [u8; 32],
    pub batch_sequence: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransitionOutcome {
    Advanced(LeaseTransitionReceipt),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UsageOutcome {
    Recorded(LeaseUsage),
    ClosedByBound {
        receipt: LeaseTransitionReceipt,
        bound: BoundKind,
        consumed: u128,
        limit: u128,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Lease {
    id: LeaseId,
    tenant: PrincipalId,
    host_program: ProgramId,
    image_code_hash: CodeHash,
    namespace: EphemeralNamespace,
    escrow_asset: [u8; 32],
    escrow_account: [u8; 32],
    escrow_amount: u128,
    limits: LeaseLimits,
    opened_at: u64,
    expiry: u64,
    state: LeaseState,
    usage: LeaseUsage,
    escrow_consumed: u128,
    history: Vec<LeaseTransitionReceipt>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LeaseStateWitness {
    canonical_state: Vec<u8>,
    digest: [u8; 32],
}

impl LeaseStateWitness {
    #[must_use] pub fn canonical_state(&self) -> &[u8] { &self.canonical_state }
    #[must_use] pub const fn digest(&self) -> [u8; 32] { self.digest }
}

impl Lease {
    #[allow(clippy::too_many_arguments)]
    pub fn request(
        id: LeaseId, tenant: PrincipalId, host_program: ProgramId, image_code_hash: CodeHash,
        escrow_asset: [u8; 32], escrow_amount: u128, limits: LeaseLimits,
        opened_at: u64, expiry: u64,
    ) -> Result<Self, LeaseRefusal> {
        if image_code_hash == [0; 32] || escrow_asset == [0; 32] {
            return Err(LeaseRefusal::ReservedIdentifier);
        }
        if escrow_amount == 0 || escrow_amount > MAX_LEASE_ESCROW {
            return Err(LeaseRefusal::InvalidEscrow { declared: escrow_amount, maximum: MAX_LEASE_ESCROW });
        }
        let lifetime = expiry.checked_sub(opened_at).ok_or(LeaseRefusal::InvalidExpiry)?;
        if lifetime == 0 || lifetime > MAX_LEASE_LIFETIME_BATCHES {
            return Err(LeaseRefusal::InvalidLifetime { declared: lifetime, maximum: MAX_LEASE_LIFETIME_BATCHES });
        }
        let mut escrow_seed = Vec::with_capacity(ESCROW_SEED_DOMAIN.len() + 32);
        escrow_seed.extend_from_slice(ESCROW_SEED_DOMAIN);
        escrow_seed.extend_from_slice(&id.bytes());
        let escrow_account = derive_program_account(host_program, &escrow_seed)
            .map_err(|_| LeaseRefusal::EscrowAccountDerivation)?
            .bytes();
        Ok(Self {
            id, tenant, host_program, image_code_hash, namespace: EphemeralNamespace::derive(host_program, id)?,
            escrow_asset, escrow_account, escrow_amount, limits: limits.validate()?, opened_at, expiry,
            state: LeaseState::Requested, usage: LeaseUsage::default(), escrow_consumed: 0,
            history: Vec::new(),
        })
    }

    #[must_use] pub const fn id(&self) -> LeaseId { self.id }
    #[must_use] pub const fn tenant(&self) -> PrincipalId { self.tenant }
    #[must_use] pub const fn host_program(&self) -> ProgramId { self.host_program }
    #[must_use] pub const fn image_code_hash(&self) -> CodeHash { self.image_code_hash }
    #[must_use] pub const fn namespace(&self) -> EphemeralNamespace { self.namespace }
    #[must_use] pub const fn escrow_asset(&self) -> [u8; 32] { self.escrow_asset }
    #[must_use] pub const fn escrow_account(&self) -> [u8; 32] { self.escrow_account }
    #[must_use] pub const fn escrow_amount(&self) -> u128 { self.escrow_amount }
    #[must_use] pub const fn limits(&self) -> LeaseLimits { self.limits }
    #[must_use] pub const fn opened_at(&self) -> u64 { self.opened_at }
    #[must_use] pub const fn expiry(&self) -> u64 { self.expiry }
    #[must_use] pub const fn state(&self) -> LeaseState { self.state }
    #[must_use] pub const fn usage(&self) -> LeaseUsage { self.usage }
    #[must_use] pub const fn escrow_consumed(&self) -> u128 { self.escrow_consumed }
    #[must_use] pub fn history(&self) -> &[LeaseTransitionReceipt] { &self.history }

    #[must_use]
    pub fn request_binding_digest(&self) -> Result<[u8; 32], LeaseRefusal> {
        let mut preimage = Vec::new();
        preimage.extend_from_slice(b"LayerX/programs/sandbox/request/v1\0");
        for value in [self.id.bytes(), self.tenant.bytes(), self.host_program.bytes(),
            self.image_code_hash, self.namespace.bytes(), self.escrow_asset, self.escrow_account] {
            preimage.extend_from_slice(&value);
        }
        preimage.extend_from_slice(&self.escrow_amount.to_be_bytes());
        for value in [self.limits.cpu_fuel, self.limits.memory_bytes,
            self.limits.storage_read_bytes, self.limits.storage_write_bytes,
            self.limits.output_values, self.limits.output_bytes, self.limits.table_elements,
            self.limits.namespace_bytes,
            self.opened_at, self.expiry] {
            preimage.extend_from_slice(&value.to_be_bytes());
        }
        hash_bytes(HashAlgorithm::Sha256, &preimage).map_err(|_| LeaseRefusal::HashRefusal)
    }

    pub fn canonical_state_bytes(&self) -> Result<Vec<u8>, LeaseRefusal> {
        let mut out = Vec::new();
        out.extend_from_slice(LEASE_STATE_DOMAIN);
        for value in [self.id.bytes(), self.tenant.bytes(), self.host_program.bytes(),
            self.image_code_hash, self.namespace.bytes(), self.escrow_asset, self.escrow_account] {
            out.extend_from_slice(&value);
        }
        out.extend_from_slice(&self.escrow_amount.to_be_bytes());
        for value in [self.limits.cpu_fuel, self.limits.memory_bytes,
            self.limits.storage_read_bytes, self.limits.storage_write_bytes,
            self.limits.output_values, self.limits.output_bytes, self.limits.table_elements,
            self.limits.namespace_bytes,
            self.opened_at, self.expiry, self.usage.cpu_fuel, self.usage.memory_bytes,
            self.usage.storage_read_bytes, self.usage.storage_write_bytes,
            self.usage.output_values, self.usage.output_bytes, self.usage.table_elements,
            self.usage.namespace_bytes] {
            out.extend_from_slice(&value.to_be_bytes());
        }
        out.extend_from_slice(&self.escrow_consumed.to_be_bytes());
        out.push(self.state as u8);
        out.push(u8::try_from(self.history.len()).map_err(|_| LeaseRefusal::HistoryOverflow)?);
        for receipt in &self.history {
            out.push(receipt.transition.activity as u8);
            out.push(receipt.transition.from as u8);
            out.push(receipt.transition.to as u8);
            out.extend_from_slice(&receipt.transition.activity_id);
            out.extend_from_slice(&receipt.transition.usage_observation_digest);
            out.extend_from_slice(&receipt.receipt_digest);
            out.extend_from_slice(&receipt.batch_sequence.to_be_bytes());
        }
        Ok(out)
    }

    pub fn state_digest(&self) -> Result<[u8; 32], LeaseRefusal> {
        hash_bytes(HashAlgorithm::Sha256, &self.canonical_state_bytes()?)
            .map_err(|_| LeaseRefusal::HashRefusal)
    }

    #[must_use]
    pub fn state_witness(&self) -> Result<LeaseStateWitness, LeaseRefusal> {
        let canonical_state = self.canonical_state_bytes()?;
        let digest = hash_bytes(HashAlgorithm::Sha256, &canonical_state)
            .map_err(|_| LeaseRefusal::HashRefusal)?;
        Ok(LeaseStateWitness { canonical_state, digest })
    }

    #[must_use]
    pub fn verifies_state_witness(&self, witness: &LeaseStateWitness) -> Result<bool, LeaseRefusal> {
        Ok(witness.canonical_state == self.canonical_state_bytes()?
            && witness.digest == self.state_digest()?)
    }

    pub fn decode_state(bytes: &[u8]) -> Result<Self, LeaseRefusal> {
        let mut cursor = StateCursor::new(bytes);
        if cursor.take(LEASE_STATE_DOMAIN.len())? != LEASE_STATE_DOMAIN {
            return Err(LeaseRefusal::InvalidStateEncoding);
        }
        let id = LeaseId::new(cursor.array()?)?;
        let tenant = PrincipalId::new(cursor.array()?).map_err(|_| LeaseRefusal::InvalidStateEncoding)?;
        let host = ProgramId::new(cursor.array()?).map_err(|_| LeaseRefusal::InvalidStateEncoding)?;
        let image = cursor.array()?;
        let encoded_namespace = cursor.array()?;
        let asset = cursor.array()?;
        let encoded_account = cursor.array()?;
        let amount = cursor.u128()?;
        let limits = LeaseLimits {
            cpu_fuel: cursor.u64()?, memory_bytes: cursor.u64()?,
            storage_read_bytes: cursor.u64()?, storage_write_bytes: cursor.u64()?,
            output_values: cursor.u64()?, output_bytes: cursor.u64()?,
            table_elements: cursor.u64()?, namespace_bytes: cursor.u64()?,
        };
        let opened_at = cursor.u64()?;
        let expiry = cursor.u64()?;
        let usage = LeaseUsage {
            cpu_fuel: cursor.u64()?, memory_bytes: cursor.u64()?,
            storage_read_bytes: cursor.u64()?, storage_write_bytes: cursor.u64()?,
            output_values: cursor.u64()?, output_bytes: cursor.u64()?,
            table_elements: cursor.u64()?, namespace_bytes: cursor.u64()?,
        };
        let escrow_consumed = cursor.u128()?;
        let state = state_from_tag(cursor.u8()?)?;
        let history_length = usize::from(cursor.u8()?);
        if history_length > MAX_LEASE_TRANSITIONS { return Err(LeaseRefusal::HistoryOverflow); }
        let mut lease = Self::request(id, tenant, host, image, asset, amount, limits, opened_at, expiry)?;
        if lease.namespace.bytes() != encoded_namespace || lease.escrow_account != encoded_account {
            return Err(LeaseRefusal::InvalidStateEncoding);
        }
        let mut prior_state = LeaseState::Requested;
        let mut prior_batch = None;
        for index in 0..history_length {
            let activity = activity_from_tag(cursor.u8()?)?;
            let from = state_from_tag(cursor.u8()?)?;
            let to = state_from_tag(cursor.u8()?)?;
            let transition = LeaseTransition { lease: id, tenant, activity, from, to,
                activity_id: cursor.array()?, usage_observation_digest: cursor.array()? };
            let receipt_digest = cursor.array()?;
            let batch_sequence = cursor.u64()?;
            if !declared_edge(activity, from, to) || from != prior_state
                || transition.activity_id == [0; 32] || receipt_digest == [0; 32]
                || prior_batch.is_some_and(|prior| batch_sequence < prior)
                || lease.history.iter().any(|prior| prior.transition.activity_id == transition.activity_id
                    || prior.receipt_digest == receipt_digest)
                || (index == 0 && (activity != LeaseActivity::Request
                    || batch_sequence != opened_at
                    || transition.usage_observation_digest != lease.request_binding_digest()?))
                || (index != 0 && activity == LeaseActivity::Request)
                || (!matches!(activity, LeaseActivity::Request | LeaseActivity::CloseBoundExceeded)
                    && transition.usage_observation_digest != [0; 32])
                || (matches!(activity, LeaseActivity::Fund | LeaseActivity::Activate | LeaseActivity::BeginSettlement)
                    && batch_sequence >= expiry)
                || (activity == LeaseActivity::Expire && from != LeaseState::Settling
                    && batch_sequence < expiry)
            {
                return Err(LeaseRefusal::InvalidStateEncoding);
            }
            lease.history.push(LeaseTransitionReceipt { lease: id, transition,
                receipt_digest, batch_sequence });
            prior_state = to;
            prior_batch = Some(batch_sequence);
        }
        if !cursor.is_empty() || prior_state != state || history_length == 0 {
            return Err(LeaseRefusal::InvalidStateEncoding);
        }
        lease.state = state;
        lease.usage = usage;
        lease.escrow_consumed = escrow_consumed;
        let close = lease.history.iter().find(|receipt|
            receipt.transition.activity == LeaseActivity::CloseBoundExceeded)
            .copied();
        let accounting_exceeded = usage.first_exceeded(limits).is_some()
            || escrow_consumed > amount;
        if (matches!(state, LeaseState::Requested | LeaseState::Funded)
            && (usage != LeaseUsage::default() || escrow_consumed != 0))
            || (close.is_none() && accounting_exceeded) {
            return Err(LeaseRefusal::InvalidStateEncoding);
        }
        if let Some(close) = close {
            let expected = usage_observation_digest(id, usage, escrow_consumed, close.batch_sequence)?;
            if close.transition.usage_observation_digest != expected
                || !accounting_exceeded && close.batch_sequence < expiry {
                return Err(LeaseRefusal::InvalidStateEncoding);
            }
        }
        if lease.canonical_state_bytes()? != bytes { return Err(LeaseRefusal::InvalidStateEncoding); }
        Ok(lease)
    }

    pub fn transition(
        &mut self, transition: LeaseTransition, evidence: TransitionEvidence,
    ) -> Result<TransitionOutcome, LeaseRefusal> {
        if matches!(transition.activity, LeaseActivity::Request | LeaseActivity::CloseBoundExceeded) {
            return Err(LeaseRefusal::IntrinsicActivityRequired);
        }
        self.apply_transition(transition, evidence)
    }

    fn apply_transition(
        &mut self, transition: LeaseTransition, evidence: TransitionEvidence,
    ) -> Result<TransitionOutcome, LeaseRefusal> {
        if transition != evidence.declared_transition || transition.activity_id != evidence.activity_id {
            return Err(LeaseRefusal::ActivityReceiptMismatch);
        }
        if transition.lease != self.id { return Err(LeaseRefusal::LeaseMismatch); }
        if transition.tenant != self.tenant { return Err(LeaseRefusal::TenantMismatch); }
        if evidence.invoking_principal != self.tenant { return Err(LeaseRefusal::TenantMismatch); }
        if !matches!(transition.activity, LeaseActivity::Request | LeaseActivity::CloseBoundExceeded)
            && transition.usage_observation_digest != [0; 32] {
            return Err(LeaseRefusal::UnexpectedUsageObservation);
        }
        if self.history.iter().any(|prior| prior.transition.activity_id == evidence.activity_id
            || prior.receipt_digest == evidence.receipt_digest) {
            return Err(LeaseRefusal::ReplayedEvidence);
        }
        if transition.from != self.state { return Err(LeaseRefusal::StaleState { expected: self.state, declared: transition.from }); }
        if transition.activity == LeaseActivity::Request
            && (!self.history.is_empty()
                || transition.usage_observation_digest != self.request_binding_digest()?
                || evidence.batch_sequence != self.opened_at) {
            return Err(LeaseRefusal::InvalidRequestBinding);
        }
        if matches!(transition.activity,
            LeaseActivity::Fund | LeaseActivity::Activate | LeaseActivity::BeginSettlement)
            && evidence.batch_sequence >= self.expiry {
            return Err(LeaseRefusal::LeaseExpired {
                expiry: self.expiry, observed: evidence.batch_sequence,
            });
        }
        if evidence.batch_sequence < self.opened_at { return Err(LeaseRefusal::InvalidSequence); }
        if self.history.last().is_some_and(|prior| evidence.batch_sequence < prior.batch_sequence) {
            return Err(LeaseRefusal::InvalidSequence);
        }
        if self.history.len() >= MAX_LEASE_TRANSITIONS { return Err(LeaseRefusal::HistoryOverflow); }
        if !declared_edge(transition.activity, transition.from, transition.to) { return Err(LeaseRefusal::InvalidTransition); }
        if transition.activity == LeaseActivity::Expire
            && transition.from != LeaseState::Settling
            && evidence.batch_sequence < self.expiry {
            return Err(LeaseRefusal::NotExpired { expiry: self.expiry, observed: evidence.batch_sequence });
        }
        let receipt = LeaseTransitionReceipt { lease: self.id, transition,
            receipt_digest: evidence.receipt_digest, batch_sequence: evidence.batch_sequence };
        self.state = transition.to;
        self.history.push(receipt);
        Ok(TransitionOutcome::Advanced(receipt))
    }

    pub fn record_usage(
        &mut self, usage: LeaseUsage, escrow_consumed: u128, observed_batch: u64,
        closure: Option<(LeaseTransition, TransitionEvidence)>,
    ) -> Result<UsageOutcome, LeaseRefusal> {
        if self.state != LeaseState::Active { return Err(LeaseRefusal::LeaseNotActive); }
        if observed_batch < self.opened_at { return Err(LeaseRefusal::InvalidSequence); }
        if usage.regressed_from(self.usage) || escrow_consumed < self.escrow_consumed {
            return Err(LeaseRefusal::UsageRegression);
        }
        let exceeded = usage.first_exceeded(self.limits).or_else(|| {
            let elapsed = observed_batch.checked_sub(self.opened_at)?;
            let lifetime = self.expiry.checked_sub(self.opened_at)?;
            (observed_batch >= self.expiry)
                .then_some((BoundKind::LifetimeBatches, u128::from(elapsed), u128::from(lifetime)))
        }).or_else(|| (escrow_consumed > self.escrow_amount)
            .then_some((BoundKind::Escrow, escrow_consumed, self.escrow_amount)));
        let Some((bound, consumed, limit)) = exceeded else {
            self.usage = usage;
            self.escrow_consumed = escrow_consumed;
            return Ok(UsageOutcome::Recorded(usage));
        };
        let (closure, evidence) = closure.ok_or(LeaseRefusal::MissingClosureActivity)?;
        if closure.activity != LeaseActivity::CloseBoundExceeded {
            return Err(LeaseRefusal::WrongActivity);
        }
        if evidence.batch_sequence != observed_batch {
            return Err(LeaseRefusal::ObservationReceiptMismatch);
        }
        let expected_observation = usage_observation_digest(
            self.id, usage, escrow_consumed, observed_batch,
        )?;
        if closure.usage_observation_digest != expected_observation {
            return Err(LeaseRefusal::ObservationReceiptMismatch);
        }
        let mut candidate = self.clone();
        candidate.usage = usage;
        candidate.escrow_consumed = escrow_consumed;
        let TransitionOutcome::Advanced(receipt) = candidate.apply_transition(closure, evidence)?;
        *self = candidate;
        Ok(UsageOutcome::ClosedByBound { receipt, bound, consumed, limit })
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LeaseBook { leases: BTreeMap<LeaseId, Lease>, active_by_principal: BTreeMap<PrincipalId, u32> }

impl LeaseBook {
    #[must_use] pub const fn new() -> Self { Self { leases: BTreeMap::new(), active_by_principal: BTreeMap::new() } }

    pub fn insert_requested(
        &mut self, mut lease: Lease, request: LeaseTransition, evidence: TransitionEvidence,
    ) -> Result<LeaseTransitionReceipt, LeaseRefusal> {
        if self.leases.contains_key(&lease.id) { return Err(LeaseRefusal::DuplicateLease); }
        let count = self.active_by_principal.get(&lease.tenant).copied().unwrap_or(0);
        ensure_principal_capacity(count)?;
        if request.activity != LeaseActivity::Request { return Err(LeaseRefusal::WrongActivity); }
        let TransitionOutcome::Advanced(receipt) = lease.apply_transition(request, evidence)?;
        self.active_by_principal.insert(lease.tenant, count + 1);
        self.leases.insert(lease.id, lease);
        Ok(receipt)
    }

    #[must_use] pub fn get(&self, id: LeaseId) -> Option<&Lease> { self.leases.get(&id) }

    pub fn record_usage(
        &mut self, id: LeaseId, usage: LeaseUsage, escrow_consumed: u128,
        observed_batch: u64, closure: Option<(LeaseTransition, TransitionEvidence)>,
    ) -> Result<UsageOutcome, LeaseRefusal> {
        self.leases.get_mut(&id).ok_or(LeaseRefusal::UnknownLease)?
            .record_usage(usage, escrow_consumed, observed_batch, closure)
    }

    pub fn transition(&mut self, id: LeaseId, transition: LeaseTransition, evidence: TransitionEvidence) -> Result<TransitionOutcome, LeaseRefusal> {
        let lease = self.leases.get_mut(&id).ok_or(LeaseRefusal::UnknownLease)?;
        let tenant = lease.tenant;
        let was_terminal = lease.state.is_terminal();
        let outcome = lease.transition(transition, evidence)?;
        if !was_terminal && lease.state.is_terminal() {
            let count = self.active_by_principal.get_mut(&tenant).ok_or(LeaseRefusal::ProtocolStateCorrupt)?;
            *count = count.checked_sub(1).ok_or(LeaseRefusal::ProtocolStateCorrupt)?;
        }
        Ok(outcome)
    }
}

fn ensure_principal_capacity(count: u32) -> Result<(), LeaseRefusal> {
    if count >= MAX_CONCURRENT_LEASES_PER_PRINCIPAL {
        Err(LeaseRefusal::PrincipalLeaseLimit)
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LeaseRefusal {
    ReservedIdentifier,
    InvalidDeclaredBound { bound: BoundKind, declared: u64, maximum: u64 },
    InvalidEscrow { declared: u128, maximum: u128 },
    InvalidLifetime { declared: u64, maximum: u64 },
    InvalidExpiry,
    EscrowAccountDerivation,
    ActivityReceiptMismatch,
    LeaseMismatch,
    TenantMismatch,
    ObservationReceiptMismatch,
    UnexpectedUsageObservation,
    InvalidRequestBinding,
    IntrinsicActivityRequired,
    LeaseExpired { expiry: u64, observed: u64 },
    ReplayedEvidence,
    InvalidCanonicalEvidence,
    InvalidSequence,
    StaleState { expected: LeaseState, declared: LeaseState },
    InvalidTransition,
    NotExpired { expiry: u64, observed: u64 },
    LeaseNotActive,
    UsageRegression,
    MissingClosureActivity,
    WrongActivity,
    DuplicateLease,
    UnknownLease,
    PrincipalLeaseLimit,
    ProtocolStateCorrupt,
    HistoryOverflow,
    HashRefusal,
    InvalidStateEncoding,
}

struct StateCursor<'a> { bytes: &'a [u8], offset: usize }
impl<'a> StateCursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self { Self { bytes, offset: 0 } }
    fn take(&mut self, length: usize) -> Result<&'a [u8], LeaseRefusal> {
        let end = self.offset.checked_add(length).ok_or(LeaseRefusal::InvalidStateEncoding)?;
        let value = self.bytes.get(self.offset..end).ok_or(LeaseRefusal::InvalidStateEncoding)?;
        self.offset = end; Ok(value)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], LeaseRefusal> {
        self.take(N)?.try_into().map_err(|_| LeaseRefusal::InvalidStateEncoding)
    }
    fn u8(&mut self) -> Result<u8, LeaseRefusal> { Ok(self.array::<1>()?[0]) }
    fn u64(&mut self) -> Result<u64, LeaseRefusal> { Ok(u64::from_be_bytes(self.array()?)) }
    fn u128(&mut self) -> Result<u128, LeaseRefusal> { Ok(u128::from_be_bytes(self.array()?)) }
    fn is_empty(&self) -> bool { self.offset == self.bytes.len() }
}

fn state_from_tag(tag: u8) -> Result<LeaseState, LeaseRefusal> {
    match tag { 0 => Ok(LeaseState::Requested), 1 => Ok(LeaseState::Funded),
        2 => Ok(LeaseState::Active), 3 => Ok(LeaseState::Settling),
        4 => Ok(LeaseState::Expired), 5 => Ok(LeaseState::Destroyed),
        _ => Err(LeaseRefusal::InvalidStateEncoding) }
}
fn activity_from_tag(tag: u8) -> Result<LeaseActivity, LeaseRefusal> {
    match tag { 0 => Ok(LeaseActivity::Request), 1 => Ok(LeaseActivity::Fund),
        2 => Ok(LeaseActivity::Activate), 3 => Ok(LeaseActivity::BeginSettlement),
        4 => Ok(LeaseActivity::Expire), 5 => Ok(LeaseActivity::Destroy),
        6 => Ok(LeaseActivity::CloseBoundExceeded),
        _ => Err(LeaseRefusal::InvalidStateEncoding) }
}

impl Display for LeaseRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result { write!(formatter, "{self:?}") }
}

impl std::error::Error for LeaseRefusal {}

#[cfg(test)]
mod tests {
    use super::*;

    fn lease(id: u8, tenant: u8) -> Lease {
        Lease::request(
            LeaseId::new([id; 32]).expect("lease id"),
            PrincipalId::new([tenant; 32]).expect("tenant"),
            ProgramId::new([3; 32]).expect("program"), [4; 32], [5; 32], 100,
            LeaseLimits { cpu_fuel: 10, memory_bytes: 10, storage_read_bytes: 10,
                storage_write_bytes: 10, output_values: 10, output_bytes: 10,
                table_elements: 10, namespace_bytes: 10 },
            10, 20,
        ).expect("valid lease")
    }

    #[test]
    fn namespace_is_deterministic_isolated_and_resource_excess_is_typed() {
        let left = lease(1, 2);
        let right = lease(2, 2);
        assert_ne!(left.namespace(), right.namespace());
        assert_eq!(Ok(left.namespace()), EphemeralNamespace::derive(left.host_program(), left.id()));
        assert_eq!(left.namespace().storage_namespace().map(StorageNamespace::program), Ok(left.host_program()));
        assert_eq!(LeaseUsage { cpu_fuel: 11, ..LeaseUsage::default() }.first_exceeded(left.limits()),
            Some((BoundKind::CpuFuel, 11, 10)));
    }

    #[test]
    fn transition_matrix_refuses_every_undeclared_edge() {
        let states = [LeaseState::Requested, LeaseState::Funded, LeaseState::Active,
            LeaseState::Settling, LeaseState::Expired, LeaseState::Destroyed];
        let activities = [LeaseActivity::Request, LeaseActivity::Fund, LeaseActivity::Activate,
            LeaseActivity::BeginSettlement, LeaseActivity::Expire, LeaseActivity::Destroy,
            LeaseActivity::CloseBoundExceeded];
        for from in states {
            for to in states {
                for activity in activities {
                    let declared = matches!((activity, from, to),
                        (LeaseActivity::Request, LeaseState::Requested, LeaseState::Requested)
                        | (LeaseActivity::Fund, LeaseState::Requested, LeaseState::Funded)
                        | (LeaseActivity::Activate, LeaseState::Funded, LeaseState::Active)
                        | (LeaseActivity::BeginSettlement, LeaseState::Active, LeaseState::Settling)
                        | (LeaseActivity::CloseBoundExceeded, LeaseState::Active, LeaseState::Settling)
                        | (LeaseActivity::Expire, LeaseState::Requested | LeaseState::Funded | LeaseState::Active | LeaseState::Settling, LeaseState::Expired)
                        | (LeaseActivity::Destroy, LeaseState::Expired, LeaseState::Destroyed));
                    assert_eq!(declared_edge(activity, from, to), declared,
                        "{activity:?}: {from:?} -> {to:?}");
                }
            }
        }
        assert!(activities.into_iter().all(|activity| !declared_edge(activity, LeaseState::Destroyed, LeaseState::Funded)));
    }

    #[test]
    fn principal_concurrency_and_declaration_bounds_are_enforced() {
        assert_eq!(ensure_principal_capacity(MAX_CONCURRENT_LEASES_PER_PRINCIPAL - 1), Ok(()));
        assert_eq!(ensure_principal_capacity(MAX_CONCURRENT_LEASES_PER_PRINCIPAL), Err(LeaseRefusal::PrincipalLeaseLimit));
        let mut invalid = lease(34, 8);
        invalid.limits.namespace_bytes = MAX_LEASE_NAMESPACE_BYTES + 1;
        assert!(matches!(invalid.limits.validate(), Err(LeaseRefusal::InvalidDeclaredBound { bound: BoundKind::NamespaceBytes, .. })));
        let zero = LeaseLimits { cpu_fuel: 0, memory_bytes: 0, storage_read_bytes: 0,
            storage_write_bytes: 0, output_values: 0, output_bytes: 0,
            table_elements: 0, namespace_bytes: 0 };
        assert_eq!(zero.validate(), Ok(zero));
        let maximum = LeaseLimits { cpu_fuel: MAX_LEASE_CPU_FUEL,
            memory_bytes: MAX_LEASE_MEMORY_BYTES,
            storage_read_bytes: MAX_LEASE_STORAGE_READ_BYTES,
            storage_write_bytes: MAX_LEASE_STORAGE_WRITE_BYTES,
            output_values: MAX_LEASE_OUTPUT_VALUES, output_bytes: MAX_LEASE_OUTPUT_BYTES,
            table_elements: MAX_LEASE_TABLE_ELEMENTS,
            namespace_bytes: MAX_LEASE_NAMESPACE_BYTES };
        assert_eq!(maximum.validate(), Ok(maximum));
    }
}
