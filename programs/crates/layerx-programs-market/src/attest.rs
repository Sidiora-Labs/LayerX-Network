//! Persistent attester policy and external-input admission for compute leases.

use layerx_program_sdk::{AccountId, Field, ProgramError, Reason};

#[cfg(target_arch = "wasm32")]
use layerx_program_sdk::{
    crypto::{self, Ed25519Message, HashAlgorithm, HashInput},
    event, storage::shared, EventData, EventTopic, StorageValue,
};

pub const MAX_ATTESTERS: usize = 8;
pub const MAX_ATTESTER_NAME_BYTES: usize = 32;
pub const MAX_INPUTS_PER_LEASE: u32 = 1_024;
pub const ATTESTER_SET_CAPACITY: usize = 252 + MAX_ATTESTERS * (33 + MAX_ATTESTER_NAME_BYTES);
pub const ATTESTED_INPUT_CAPACITY: usize = 244;
pub const ATTESTATION_STATEMENT_CAPACITY: usize = 256;
pub const ATTESTATION_REQUEST_CAPACITY: usize = 242;
pub const ATTESTATION_SIGNATURE_BYTES: usize = 64;

const VERSION: u8 = 1;
const POLICY_DOMAIN: &[u8] = b"LXP/market-attesters/v1\0";
const STATEMENT_DOMAIN: &[u8] = b"LXP/market-attested-input/v1\0";
const COMMITTED_INPUT_DOMAIN: &[u8] = b"LXP/market-committed-set/v1\0";
const ADMITTED_INPUT_DOMAIN: &[u8] = b"LXP/market-admitted-set/v1\0";
const ACCUMULATOR_DOMAIN: &[u8] = b"LXP/market-input-accumulator/v1\0";
const SETTLEMENT_INPUT_DOMAIN: &[u8] = b"LXP/market-settlement-inputs/v1\0";
const POLICY_PREFIX: &[u8] = b"lx.market.attesters/";
const INPUT_PREFIX: &[u8] = b"lx.market.input/";
const REPLAY_PREFIX: &[u8] = b"lx.market.attested/";
const TOPIC_INPUT: &[u8] = b"lx.market.input";

pub type LeaseId = [u8; 32];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ExternalInputSource {
    HttpsApi = 1,
    HardwareSensor = 2,
    ConfidentialCompute = 3,
    HumanOperator = 4,
}

impl ExternalInputSource {
    fn decode(value: u8) -> Result<Self, ProgramError> {
        match value {
            1 => Ok(Self::HttpsApi),
            2 => Ok(Self::HardwareSensor),
            3 => Ok(Self::ConfidentialCompute),
            4 => Ok(Self::HumanOperator),
            _ => Err(malformed()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum EvidenceClass {
    Attested = 1,
    VerifiedExecution = 2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum InputStatus {
    Committed = 1,
    Attested = 2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Attester<'a> {
    pub name: &'a [u8],
    pub ed25519_key: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AttesterSet<'a> {
    pub lease_id: LeaseId,
    pub tenant: AccountId,
    pub revision: u64,
    pub sealed_at: Option<u64>,
    pub committed_inputs: u32,
    pub admitted_inputs: u32,
    pub last_committed_input: [u8; 32],
    pub last_admitted_input: [u8; 32],
    pub committed_root: [u8; 32],
    pub admitted_root: [u8; 32],
    pub settlement_commitment: Option<[u8; 32]>,
    pub entries: [Option<Attester<'a>>; MAX_ATTESTERS],
}

impl<'a> AttesterSet<'a> {
    pub fn new(
        lease_id: LeaseId,
        tenant: AccountId,
        revision: u64,
        entries: [Option<Attester<'a>>; MAX_ATTESTERS],
    ) -> Result<Self, ProgramError> {
        if lease_id == [0; 32] || revision == 0 {
            return Err(malformed());
        }
        let mut count = 0usize;
        for (index, entry) in entries.iter().enumerate() {
            let Some(attester) = entry else { continue };
            if index != count || !valid_name(attester.name) || attester.ed25519_key == [0; 32] {
                return Err(malformed());
            }
            for prior in entries[..index].iter().flatten() {
                if prior.name == attester.name {
                    return Err(ProgramError::value(Field::CallInput, Reason::Duplicate));
                }
            }
            count = count.checked_add(1).ok_or_else(malformed)?;
        }
        if count == 0 {
            return Err(malformed());
        }
        Ok(Self {
            lease_id,
            tenant,
            revision,
            sealed_at: None,
            committed_inputs: 0,
            admitted_inputs: 0,
            last_committed_input: [0; 32],
            last_admitted_input: [0; 32],
            committed_root: [0; 32],
            admitted_root: [0; 32],
            settlement_commitment: None,
            entries,
        })
    }

    pub fn named_key(&self, name: &[u8]) -> Result<[u8; 32], ProgramError> {
        self.entries
            .iter()
            .flatten()
            .find(|attester| attester.name == name)
            .map(|attester| attester.ed25519_key)
            .ok_or_else(malformed)
    }

    pub fn ready_for_settlement(&self) -> bool {
        self.sealed_at.is_some()
            && self.committed_inputs > 0
            && self.admitted_inputs == self.committed_inputs
            && self.settlement_commitment.is_some()
    }

    pub fn settlement_input_commitment(&self) -> Result<[u8; 32], ProgramError> {
        if !self.ready_for_settlement() { return Err(malformed()); }
        self.settlement_commitment.ok_or_else(malformed)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InputCommitment {
    pub lease_id: LeaseId,
    pub input_id: [u8; 32],
    pub payload_digest: [u8; 32],
    pub payload_length: u64,
    pub source: ExternalInputSource,
    pub source_locator_digest: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Attestation<'a> {
    pub input: InputCommitment,
    pub observed_at: u64,
    pub attester_name: &'a [u8],
    pub signature: [u8; ATTESTATION_SIGNATURE_BYTES],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AttestedInput {
    pub commitment: InputCommitment,
    pub predecessor_input_id: [u8; 32],
    pub observed_at: u64,
    pub attester_name_digest: [u8; 32],
    pub statement_digest: [u8; 32],
    pub evidence: EvidenceClass,
    pub status: InputStatus,
}

pub fn encode_attestation(attestation: Attestation<'_>, output: &mut [u8]) -> Result<usize, ProgramError> {
    if !valid_name(attestation.attester_name) { return Err(malformed()); }
    let mut offset = 0usize;
    append(output, &mut offset, &attestation.input.lease_id)?;
    append(output, &mut offset, &attestation.input.input_id)?;
    append(output, &mut offset, &attestation.input.payload_digest)?;
    append(output, &mut offset, &attestation.input.payload_length.to_be_bytes())?;
    append(output, &mut offset, &[attestation.input.source as u8])?;
    append(output, &mut offset, &attestation.input.source_locator_digest)?;
    append(output, &mut offset, &attestation.observed_at.to_be_bytes())?;
    append(output, &mut offset, &[u8::try_from(attestation.attester_name.len()).map_err(|_| malformed())?])?;
    append(output, &mut offset, attestation.attester_name)?;
    append(output, &mut offset, &attestation.signature)?;
    Ok(offset)
}

pub fn decode_attestation(input: &[u8]) -> Result<Attestation<'_>, ProgramError> {
    let mut cursor = Cursor::new(input);
    let attestation = Attestation {
        input: InputCommitment {
            lease_id: cursor.array()?,
            input_id: cursor.array()?,
            payload_digest: cursor.array()?,
            payload_length: cursor.u64()?,
            source: ExternalInputSource::decode(cursor.byte()?)?,
            source_locator_digest: cursor.array()?,
        },
        observed_at: cursor.u64()?,
        attester_name: cursor.name()?,
        signature: cursor.array()?,
    };
    cursor.finish()?;
    Ok(attestation)
}

pub fn commit_input(
    policy: &mut AttesterSet<'_>,
    commitment: InputCommitment,
    principal: AccountId,
) -> Result<AttestedInput, ProgramError> {
    if principal != policy.tenant
        || policy.sealed_at.is_some()
        || commitment.lease_id != policy.lease_id
        || commitment.input_id == [0; 32]
        || commitment.payload_digest == [0; 32]
        || commitment.payload_length == 0
        || commitment.source_locator_digest == [0; 32]
        || policy.committed_inputs >= MAX_INPUTS_PER_LEASE
        || (policy.last_committed_input != [0; 32]
            && commitment.input_id <= policy.last_committed_input)
    {
        return Err(malformed());
    }
    policy.committed_inputs = policy.committed_inputs.checked_add(1).ok_or_else(malformed)?;
    let predecessor_input_id = policy.last_committed_input;
    policy.last_committed_input = commitment.input_id;
    Ok(AttestedInput {
        commitment,
        predecessor_input_id,
        observed_at: 0,
        attester_name_digest: [0; 32],
        statement_digest: [0; 32],
        evidence: EvidenceClass::Attested,
        status: InputStatus::Committed,
    })
}

pub fn seal_inputs(
    policy: &mut AttesterSet<'_>,
    principal: AccountId,
    height: u64,
) -> Result<(), ProgramError> {
    if principal != policy.tenant || policy.sealed_at.is_some() || policy.committed_inputs == 0 {
        return Err(malformed());
    }
    policy.sealed_at = Some(height);
    Ok(())
}

fn valid_name(name: &[u8]) -> bool {
    !name.is_empty()
        && name.len() <= MAX_ATTESTER_NAME_BYTES
        && name.iter().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'-' | b'.')
        })
}

fn malformed() -> ProgramError {
    ProgramError::value(Field::CallInput, Reason::Malformed)
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self { Self { bytes, offset: 0 } }
    fn take(&mut self, length: usize) -> Result<&'a [u8], ProgramError> {
        let end = self.offset.checked_add(length).ok_or_else(malformed)?;
        let value = self.bytes.get(self.offset..end).ok_or_else(malformed)?;
        self.offset = end;
        Ok(value)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], ProgramError> {
        self.take(N)?.try_into().map_err(|_| malformed())
    }
    fn byte(&mut self) -> Result<u8, ProgramError> { Ok(self.take(1)?[0]) }
    fn u32(&mut self) -> Result<u32, ProgramError> { Ok(u32::from_be_bytes(self.array()?)) }
    fn u64(&mut self) -> Result<u64, ProgramError> { Ok(u64::from_be_bytes(self.array()?)) }
    fn account(&mut self) -> Result<AccountId, ProgramError> { AccountId::new(self.array()?) }
    fn name(&mut self) -> Result<&'a [u8], ProgramError> {
        let length = usize::from(self.byte()?);
        let name = self.take(length)?;
        if !valid_name(name) { return Err(malformed()); }
        Ok(name)
    }
    fn finish(self) -> Result<(), ProgramError> {
        if self.offset == self.bytes.len() { Ok(()) } else { Err(malformed()) }
    }
}

fn append(output: &mut [u8], offset: &mut usize, value: &[u8]) -> Result<(), ProgramError> {
    let end = offset.checked_add(value.len()).ok_or_else(malformed)?;
    output.get_mut(*offset..end).ok_or_else(malformed)?.copy_from_slice(value);
    *offset = end;
    Ok(())
}

pub fn encode_policy(policy: AttesterSet<'_>, output: &mut [u8]) -> Result<usize, ProgramError> {
    let mut offset = 0usize;
    append(output, &mut offset, &[VERSION])?;
    append(output, &mut offset, &policy.lease_id)?;
    append(output, &mut offset, &policy.tenant.bytes())?;
    append(output, &mut offset, &policy.revision.to_be_bytes())?;
    match policy.sealed_at {
        Some(height) => { append(output, &mut offset, &[1])?; append(output, &mut offset, &height.to_be_bytes())?; }
        None => append(output, &mut offset, &[0])?,
    }
    append(output, &mut offset, &policy.committed_inputs.to_be_bytes())?;
    append(output, &mut offset, &policy.admitted_inputs.to_be_bytes())?;
    append(output, &mut offset, &policy.last_committed_input)?;
    append(output, &mut offset, &policy.last_admitted_input)?;
    append(output, &mut offset, &policy.committed_root)?;
    append(output, &mut offset, &policy.admitted_root)?;
    match policy.settlement_commitment {
        Some(root) => { append(output, &mut offset, &[1])?; append(output, &mut offset, &root)?; }
        None => append(output, &mut offset, &[0])?,
    }
    let count = policy.entries.iter().flatten().count();
    append(output, &mut offset, &[u8::try_from(count).map_err(|_| malformed())?])?;
    for attester in policy.entries.iter().flatten() {
        append(output, &mut offset, &[u8::try_from(attester.name.len()).map_err(|_| malformed())?])?;
        append(output, &mut offset, attester.name)?;
        append(output, &mut offset, &attester.ed25519_key)?;
    }
    Ok(offset)
}

pub fn decode_policy(input: &[u8]) -> Result<AttesterSet<'_>, ProgramError> {
    let mut cursor = Cursor::new(input);
    if cursor.byte()? != VERSION { return Err(malformed()); }
    let lease_id = cursor.array()?;
    let tenant = cursor.account()?;
    let revision = cursor.u64()?;
    let sealed_at = match cursor.byte()? { 0 => None, 1 => Some(cursor.u64()?), _ => return Err(malformed()) };
    let committed_inputs = cursor.u32()?;
    let admitted_inputs = cursor.u32()?;
    if admitted_inputs > committed_inputs { return Err(malformed()); }
    let last_committed_input = cursor.array()?;
    let last_admitted_input = cursor.array()?;
    let committed_root = cursor.array()?;
    let admitted_root = cursor.array()?;
    let settlement_commitment = match cursor.byte()? {
        0 => None, 1 => Some(cursor.array()?), _ => return Err(malformed()),
    };
    let count = usize::from(cursor.byte()?);
    if count == 0 || count > MAX_ATTESTERS { return Err(malformed()); }
    let mut entries = [None; MAX_ATTESTERS];
    for entry in entries.iter_mut().take(count) {
        *entry = Some(Attester { name: cursor.name()?, ed25519_key: cursor.array()? });
    }
    cursor.finish()?;
    let mut policy = AttesterSet::new(lease_id, tenant, revision, entries)?;
    policy.sealed_at = sealed_at;
    policy.committed_inputs = committed_inputs;
    policy.admitted_inputs = admitted_inputs;
    policy.last_committed_input = last_committed_input;
    policy.last_admitted_input = last_admitted_input;
    policy.committed_root = committed_root;
    policy.admitted_root = admitted_root;
    policy.settlement_commitment = settlement_commitment;
    if (policy.committed_inputs == 0) != (policy.last_committed_input == [0; 32])
        || (policy.admitted_inputs == 0) != (policy.last_admitted_input == [0; 32])
        || policy.settlement_commitment.is_some() != policy.ready_for_settlement()
        || policy.settlement_commitment == Some([0; 32])
    {
        return Err(malformed());
    }
    Ok(policy)
}

pub fn encode_input(input: AttestedInput, output: &mut [u8]) -> Result<usize, ProgramError> {
    let mut offset = 0usize;
    append(output, &mut offset, &[VERSION, input.status as u8, input.evidence as u8, input.commitment.source as u8])?;
    append(output, &mut offset, &input.commitment.lease_id)?;
    append(output, &mut offset, &input.commitment.input_id)?;
    append(output, &mut offset, &input.commitment.payload_digest)?;
    append(output, &mut offset, &input.commitment.payload_length.to_be_bytes())?;
    append(output, &mut offset, &input.commitment.source_locator_digest)?;
    append(output, &mut offset, &input.predecessor_input_id)?;
    append(output, &mut offset, &input.observed_at.to_be_bytes())?;
    append(output, &mut offset, &input.attester_name_digest)?;
    append(output, &mut offset, &input.statement_digest)?;
    Ok(offset)
}

pub fn decode_input(input: &[u8]) -> Result<AttestedInput, ProgramError> {
    let mut cursor = Cursor::new(input);
    if cursor.byte()? != VERSION { return Err(malformed()); }
    let status = match cursor.byte()? { 1 => InputStatus::Committed, 2 => InputStatus::Attested, _ => return Err(malformed()) };
    let evidence = match cursor.byte()? { 1 => EvidenceClass::Attested, 2 => EvidenceClass::VerifiedExecution, _ => return Err(malformed()) };
    let source = ExternalInputSource::decode(cursor.byte()?)?;
    let result = AttestedInput {
        commitment: InputCommitment {
            lease_id: cursor.array()?, input_id: cursor.array()?, payload_digest: cursor.array()?,
            payload_length: cursor.u64()?, source, source_locator_digest: cursor.array()?,
        },
        predecessor_input_id: cursor.array()?,
        observed_at: cursor.u64()?, attester_name_digest: cursor.array()?, statement_digest: cursor.array()?,
        evidence, status,
    };
    cursor.finish()?;
    if result.evidence != EvidenceClass::Attested { return Err(malformed()); }
    Ok(result)
}

pub fn statement_bytes(
    policy_root: [u8; 32],
    revision: u64,
    commitment: InputCommitment,
    observed_at: u64,
    attester_name: &[u8],
    output: &mut [u8],
) -> Result<usize, ProgramError> {
    if !valid_name(attester_name) { return Err(malformed()); }
    let mut offset = 0usize;
    append(output, &mut offset, STATEMENT_DOMAIN)?;
    append(output, &mut offset, &policy_root)?;
    append(output, &mut offset, &revision.to_be_bytes())?;
    append(output, &mut offset, &commitment.lease_id)?;
    append(output, &mut offset, &commitment.input_id)?;
    append(output, &mut offset, &commitment.payload_digest)?;
    append(output, &mut offset, &commitment.payload_length.to_be_bytes())?;
    append(output, &mut offset, &[commitment.source as u8])?;
    append(output, &mut offset, &commitment.source_locator_digest)?;
    append(output, &mut offset, &observed_at.to_be_bytes())?;
    append(output, &mut offset, &[u8::try_from(attester_name.len()).map_err(|_| malformed())?])?;
    append(output, &mut offset, attester_name)?;
    Ok(offset)
}

pub fn encode_policy_commitment(policy: AttesterSet<'_>, output: &mut [u8]) -> Result<usize, ProgramError> {
    let mut offset = 0usize;
    append(output, &mut offset, POLICY_DOMAIN)?;
    append(output, &mut offset, &policy.lease_id)?;
    append(output, &mut offset, &policy.tenant.bytes())?;
    append(output, &mut offset, &policy.revision.to_be_bytes())?;
    let count = policy.entries.iter().flatten().count();
    append(output, &mut offset, &[u8::try_from(count).map_err(|_| malformed())?])?;
    for attester in policy.entries.iter().flatten() {
        append(output, &mut offset, &[u8::try_from(attester.name.len()).map_err(|_| malformed())?])?;
        append(output, &mut offset, attester.name)?;
        append(output, &mut offset, &attester.ed25519_key)?;
    }
    Ok(offset)
}

#[cfg(target_arch = "wasm32")]
fn state_key(prefix: &[u8], lease_id: LeaseId, suffix: Option<[u8; 32]>) -> Result<([u8; 96], usize), ProgramError> {
    let mut key = [0; 96];
    let mut length = prefix.len();
    key.get_mut(..length).ok_or_else(malformed)?.copy_from_slice(prefix);
    append(&mut key, &mut length, &lease_id)?;
    if let Some(value) = suffix { append(&mut key, &mut length, &value)?; }
    Ok((key, length))
}

#[cfg(target_arch = "wasm32")]
fn read(prefix: &[u8], lease_id: LeaseId, suffix: Option<[u8; 32]>, output: &mut [u8]) -> Result<usize, ProgramError> {
    let (key, length) = state_key(prefix, lease_id, suffix)?;
    shared::read(shared::SharedStorageKey::new(&key[..length])?, output)?
        .ok_or_else(|| ProgramError::value(Field::StorageValue, Reason::Malformed))
}

#[cfg(target_arch = "wasm32")]
fn write(prefix: &[u8], lease_id: LeaseId, suffix: Option<[u8; 32]>, value: &[u8]) -> Result<(), ProgramError> {
    let (key, length) = state_key(prefix, lease_id, suffix)?;
    shared::write(shared::SharedStorageKey::new(&key[..length])?, StorageValue::new(value)?)
}

#[cfg(target_arch = "wasm32")]
fn absent(prefix: &[u8], lease_id: LeaseId, suffix: Option<[u8; 32]>, scratch: &mut [u8]) -> Result<(), ProgramError> {
    let (key, length) = state_key(prefix, lease_id, suffix)?;
    if shared::read(shared::SharedStorageKey::new(&key[..length])?, scratch)?.is_some() {
        return Err(ProgramError::value(Field::StorageValue, Reason::Duplicate));
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn digest(bytes: &[u8]) -> Result<[u8; 32], ProgramError> {
    crypto::hash(HashAlgorithm::Sha256, HashInput::new(bytes)?)
}

#[cfg(target_arch = "wasm32")]
fn committed_leaf(commitment: InputCommitment) -> Result<[u8; 32], ProgramError> {
    let mut bytes = [0; 192];
    let mut offset = 0;
    append(&mut bytes, &mut offset, COMMITTED_INPUT_DOMAIN)?;
    append(&mut bytes, &mut offset, &commitment.lease_id)?;
    append(&mut bytes, &mut offset, &commitment.input_id)?;
    append(&mut bytes, &mut offset, &commitment.payload_digest)?;
    append(&mut bytes, &mut offset, &commitment.payload_length.to_be_bytes())?;
    append(&mut bytes, &mut offset, &[commitment.source as u8])?;
    append(&mut bytes, &mut offset, &commitment.source_locator_digest)?;
    digest(&bytes[..offset])
}

#[cfg(target_arch = "wasm32")]
fn admitted_leaf(input: AttestedInput) -> Result<[u8; 32], ProgramError> {
    let mut canonical = [0; ATTESTED_INPUT_CAPACITY];
    let canonical_length = encode_input(input, &mut canonical)?;
    let mut bytes = [0; 320];
    let mut offset = 0;
    append(&mut bytes, &mut offset, ADMITTED_INPUT_DOMAIN)?;
    append(&mut bytes, &mut offset, &canonical[..canonical_length])?;
    digest(&bytes[..offset])
}

#[cfg(target_arch = "wasm32")]
fn accumulate(prior: [u8; 32], leaf: [u8; 32]) -> Result<[u8; 32], ProgramError> {
    let mut bytes = [0; 128];
    let mut offset = 0;
    append(&mut bytes, &mut offset, ACCUMULATOR_DOMAIN)?;
    append(&mut bytes, &mut offset, &prior)?;
    append(&mut bytes, &mut offset, &leaf)?;
    digest(&bytes[..offset])
}

#[cfg(target_arch = "wasm32")]
fn freeze_settlement_commitment(policy: AttesterSet<'_>) -> Result<[u8; 32], ProgramError> {
    if policy.sealed_at.is_none() || policy.committed_inputs == 0
        || policy.admitted_inputs != policy.committed_inputs
        || policy.last_admitted_input != policy.last_committed_input
        || policy.committed_root == [0; 32] || policy.admitted_root == [0; 32]
    {
        return Err(malformed());
    }
    let mut policy_bytes = [0; ATTESTER_SET_CAPACITY];
    let policy_length = encode_policy_commitment(policy, &mut policy_bytes)?;
    let policy_root = digest(&policy_bytes[..policy_length])?;
    let mut bytes = [0; 192];
    let mut offset = 0;
    append(&mut bytes, &mut offset, SETTLEMENT_INPUT_DOMAIN)?;
    append(&mut bytes, &mut offset, &policy.lease_id)?;
    append(&mut bytes, &mut offset, &policy.revision.to_be_bytes())?;
    append(&mut bytes, &mut offset, &policy_root)?;
    append(&mut bytes, &mut offset, &policy.committed_inputs.to_be_bytes())?;
    append(&mut bytes, &mut offset, &policy.committed_root)?;
    append(&mut bytes, &mut offset, &policy.admitted_root)?;
    digest(&bytes[..offset])
}

#[cfg(target_arch = "wasm32")]
pub fn configure(
    lease_id: LeaseId,
    tenant: AccountId,
    revision: u64,
    entries: [Option<Attester<'_>>; MAX_ATTESTERS],
    scratch: &mut [u8; ATTESTER_SET_CAPACITY],
) -> Result<(), ProgramError> {
    absent(POLICY_PREFIX, lease_id, None, scratch)?;
    let policy = AttesterSet::new(lease_id, tenant, revision, entries)?;
    let written = encode_policy(policy, scratch)?;
    write(POLICY_PREFIX, lease_id, None, &scratch[..written])
}

#[cfg(target_arch = "wasm32")]
pub fn commit(
    commitment: InputCommitment,
    principal: AccountId,
) -> Result<(), ProgramError> {
    let mut policy_bytes = [0; ATTESTER_SET_CAPACITY];
    let policy_length = read(POLICY_PREFIX, commitment.lease_id, None, &mut policy_bytes)?;
    let mut policy = decode_policy(&policy_bytes[..policy_length])?;
    let mut input_bytes = [0; ATTESTED_INPUT_CAPACITY];
    absent(INPUT_PREFIX, commitment.lease_id, Some(commitment.input_id), &mut input_bytes)?;
    let input = commit_input(&mut policy, commitment, principal)?;
    policy.committed_root = accumulate(policy.committed_root,
        committed_leaf(commitment)?)?;
    let input_length = encode_input(input, &mut input_bytes)?;
    let mut policy_output = [0; ATTESTER_SET_CAPACITY];
    let policy_written = encode_policy(policy, &mut policy_output)?;
    write(INPUT_PREFIX, commitment.lease_id, Some(commitment.input_id), &input_bytes[..input_length])?;
    write(POLICY_PREFIX, commitment.lease_id, None, &policy_output[..policy_written])?;
    event::emit(EventTopic::new(TOPIC_INPUT)?, EventData::new(&input_bytes[..input_length])?)
}

#[cfg(target_arch = "wasm32")]
pub fn seal(lease_id: LeaseId, principal: AccountId, height: u64) -> Result<(), ProgramError> {
    let mut bytes = [0; ATTESTER_SET_CAPACITY];
    let length = read(POLICY_PREFIX, lease_id, None, &mut bytes)?;
    let mut policy = decode_policy(&bytes[..length])?;
    seal_inputs(&mut policy, principal, height)?;
    let mut output = [0; ATTESTER_SET_CAPACITY];
    let written = encode_policy(policy, &mut output)?;
    write(POLICY_PREFIX, lease_id, None, &output[..written])
}

#[cfg(target_arch = "wasm32")]
pub fn admit(attestation: Attestation<'_>, height: u64) -> Result<(), ProgramError> {
    let mut policy_bytes = [0; ATTESTER_SET_CAPACITY];
    let policy_length = read(POLICY_PREFIX, attestation.input.lease_id, None, &mut policy_bytes)?;
    let mut policy = decode_policy(&policy_bytes[..policy_length])?;
    let sealed_at = policy.sealed_at.ok_or_else(malformed)?;
    if attestation.observed_at < sealed_at
        || attestation.observed_at > height
        || !valid_name(attestation.attester_name)
    {
        return Err(malformed());
    }
    let mut input_bytes = [0; ATTESTED_INPUT_CAPACITY];
    let input_length = read(INPUT_PREFIX, attestation.input.lease_id, Some(attestation.input.input_id), &mut input_bytes)?;
    let committed = decode_input(&input_bytes[..input_length])?;
    if committed.commitment != attestation.input {
        return Err(malformed());
    }
    if committed.status == InputStatus::Attested {
        return Err(ProgramError::value(Field::CallInput, Reason::Duplicate));
    }
    if committed.predecessor_input_id != policy.last_admitted_input {
        return Err(malformed());
    }
    let mut policy_canonical = [0; ATTESTER_SET_CAPACITY];
    let canonical_length = encode_policy_commitment(policy, &mut policy_canonical)?;
    let policy_root = digest(&policy_canonical[..canonical_length])?;
    let mut statement = [0; ATTESTATION_STATEMENT_CAPACITY];
    let statement_length = statement_bytes(
        policy_root,
        policy.revision,
        attestation.input,
        attestation.observed_at,
        attestation.attester_name,
        &mut statement,
    )?;
    let statement_digest = digest(&statement[..statement_length])?;
    let name_digest = digest(attestation.attester_name)?;
    absent(REPLAY_PREFIX, attestation.input.lease_id, Some(statement_digest), &mut input_bytes)?;
    let key = policy.named_key(attestation.attester_name)?;
    crypto::ed25519_verify(Ed25519Message::new(&statement_digest)?, &key, &attestation.signature)?;
    let admitted = AttestedInput {
        commitment: attestation.input,
        predecessor_input_id: committed.predecessor_input_id,
        observed_at: attestation.observed_at,
        attester_name_digest: name_digest,
        statement_digest,
        evidence: EvidenceClass::Attested,
        status: InputStatus::Attested,
    };
    policy.admitted_inputs = policy.admitted_inputs.checked_add(1).ok_or_else(malformed)?;
    policy.last_admitted_input = admitted.commitment.input_id;
    policy.admitted_root = accumulate(policy.admitted_root, admitted_leaf(admitted)?)?;
    if policy.admitted_inputs == policy.committed_inputs {
        policy.settlement_commitment = Some(freeze_settlement_commitment(policy)?);
    }
    let input_written = encode_input(admitted, &mut input_bytes)?;
    let mut policy_output = [0; ATTESTER_SET_CAPACITY];
    let policy_written = encode_policy(policy, &mut policy_output)?;
    write(INPUT_PREFIX, attestation.input.lease_id, Some(attestation.input.input_id), &input_bytes[..input_written])?;
    write(REPLAY_PREFIX, attestation.input.lease_id, Some(statement_digest), &[VERSION])?;
    write(POLICY_PREFIX, attestation.input.lease_id, None, &policy_output[..policy_written])?;
    event::emit(EventTopic::new(TOPIC_INPUT)?, EventData::new(&input_bytes[..input_written])?)
}

#[cfg(target_arch = "wasm32")]
pub fn require_ready_commitment(lease_id: LeaseId) -> Result<[u8; 32], ProgramError> {
    let mut bytes = [0; ATTESTER_SET_CAPACITY];
    let length = read(POLICY_PREFIX, lease_id, None, &mut bytes)?;
    let policy = decode_policy(&bytes[..length])?;
    policy.settlement_input_commitment()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(value: u8) -> AccountId {
        AccountId::new([value; 32]).unwrap_or_else(|error| panic!("account: {error}"))
    }

    fn entries<'a>(name: &'a [u8], key: [u8; 32]) -> [Option<Attester<'a>>; MAX_ATTESTERS] {
        let mut entries = [None; MAX_ATTESTERS];
        entries[0] = Some(Attester { name, ed25519_key: key });
        entries
    }

    fn weather() -> InputCommitment {
        InputCommitment {
            lease_id: [7; 32],
            input_id: [8; 32],
            payload_digest: [9; 32],
            payload_length: 27,
            source: ExternalInputSource::HttpsApi,
            source_locator_digest: [10; 32],
        }
    }

    #[test]
    fn real_source_commitment_is_sealed_before_work_and_labeled_attested() {
        let tenant = account(3);
        let mut policy = AttesterSet::new([7; 32], tenant, 4, entries(b"weather-oracle.eu", [11; 32]))
            .unwrap_or_else(|error| panic!("policy: {error}"));
        let input = commit_input(&mut policy, weather(), tenant)
            .unwrap_or_else(|error| panic!("commit: {error}"));
        seal_inputs(&mut policy, tenant, 1_201).unwrap_or_else(|error| panic!("seal: {error}"));
        assert_eq!(input.evidence, EvidenceClass::Attested);
        assert_ne!(input.evidence, EvidenceClass::VerifiedExecution);
        assert!(commit_input(&mut policy, weather(), tenant).is_err());
        assert!(!policy.ready_for_settlement());
    }

    #[test]
    fn policy_and_hardware_input_have_canonical_bounded_round_trips() {
        let tenant = account(3);
        let mut policy = AttesterSet::new([7; 32], tenant, 9, entries(b"factory-meter-17", [12; 32]))
            .unwrap_or_else(|error| panic!("policy: {error}"));
        let mut commitment = weather();
        commitment.source = ExternalInputSource::HardwareSensor;
        let input = commit_input(&mut policy, commitment, tenant)
            .unwrap_or_else(|error| panic!("commit: {error}"));
        let mut policy_bytes = [0; ATTESTER_SET_CAPACITY];
        let length = encode_policy(policy, &mut policy_bytes).unwrap_or_else(|error| panic!("encode: {error}"));
        assert_eq!(decode_policy(&policy_bytes[..length]), Ok(policy));
        let mut input_bytes = [0; ATTESTED_INPUT_CAPACITY];
        let length = encode_input(input, &mut input_bytes).unwrap_or_else(|error| panic!("encode: {error}"));
        assert_eq!(decode_input(&input_bytes[..length]), Ok(input));
    }

    #[test]
    fn unnamed_attester_and_malformed_policy_are_refused() {
        let policy = AttesterSet::new([7; 32], account(3), 1, entries(b"enclave-cluster-3", [13; 32]))
            .unwrap_or_else(|error| panic!("policy: {error}"));
        assert!(policy.named_key(b"foreign-enclave").is_err());
        assert!(AttesterSet::new([7; 32], account(3), 1, entries(b"Not Canonical", [13; 32])).is_err());
    }

    #[test]
    fn runtime_authenticated_tenant_is_required_for_commit_and_seal() {
        let tenant = account(3);
        let mut policy = AttesterSet::new([7; 32], tenant, 1, entries(b"operator-review-board", [14; 32]))
            .unwrap_or_else(|error| panic!("policy: {error}"));
        assert!(commit_input(&mut policy, weather(), account(4)).is_err());
        commit_input(&mut policy, weather(), tenant)
            .unwrap_or_else(|error| panic!("commit: {error}"));
        assert!(seal_inputs(&mut policy, account(4), 1_201).is_err());
    }

    #[test]
    fn statement_binds_policy_source_payload_and_observation() {
        let mut first = [0; ATTESTATION_STATEMENT_CAPACITY];
        let first_length = statement_bytes([21; 32], 5, weather(), 1_205, b"weather-oracle.eu", &mut first)
            .unwrap_or_else(|error| panic!("statement: {error}"));
        let mut altered = weather();
        altered.source = ExternalInputSource::HumanOperator;
        let mut second = [0; ATTESTATION_STATEMENT_CAPACITY];
        let second_length = statement_bytes([21; 32], 5, altered, 1_205, b"weather-oracle.eu", &mut second)
            .unwrap_or_else(|error| panic!("statement: {error}"));
        assert_eq!(first_length, second_length);
        assert_ne!(&first[..first_length], &second[..second_length]);
    }

    #[test]
    fn production_attestation_wire_refuses_malformed_signature_framing() {
        let attestation = Attestation {
            input: weather(),
            observed_at: 1_205,
            attester_name: b"weather-oracle.eu",
            signature: [44; ATTESTATION_SIGNATURE_BYTES],
        };
        let mut wire = [0; ATTESTATION_REQUEST_CAPACITY];
        let length = encode_attestation(attestation, &mut wire)
            .unwrap_or_else(|error| panic!("attestation: {error}"));
        assert_eq!(decode_attestation(&wire[..length]), Ok(attestation));
        assert!(decode_attestation(&wire[..length - 1]).is_err());
    }

    #[test]
    fn input_order_replay_missing_and_exact_frozen_root_are_explicit() {
        let tenant = account(3);
        let mut policy = AttesterSet::new([7; 32], tenant, 11,
            entries(b"weather-oracle.eu", [11; 32]))
            .unwrap_or_else(|error| panic!("policy: {error}"));
        let first = weather();
        commit_input(&mut policy, first, tenant)
            .unwrap_or_else(|error| panic!("first: {error}"));
        assert!(commit_input(&mut policy, first, tenant).is_err());
        let mut earlier = first;
        earlier.input_id = [7; 32];
        assert!(commit_input(&mut policy, earlier, tenant).is_err());
        seal_inputs(&mut policy, tenant, 1_201)
            .unwrap_or_else(|error| panic!("seal: {error}"));
        assert!(policy.settlement_input_commitment().is_err());
        policy.admitted_inputs = policy.committed_inputs;
        policy.last_admitted_input = policy.last_committed_input;
        policy.committed_root = [21; 32];
        policy.admitted_root = [22; 32];
        policy.settlement_commitment = Some([23; 32]);
        assert_eq!(policy.settlement_input_commitment(), Ok([23; 32]));
        assert_ne!(policy.settlement_input_commitment(), Ok([24; 32]));
    }
}
