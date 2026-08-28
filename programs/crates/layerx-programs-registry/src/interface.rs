use core::fmt::{self, Display};
use std::collections::BTreeSet;

use layerx_programs_runtime::abi::{EncodingConvention, TypeTag};
use layerx_programs_runtime::{ProgramId, WasmEngine, ABI_V1_VERSION, ABI_V2_VERSION};

use crate::account_state::{verify_state_membership, StateProof};
use crate::hash::sha256;
use crate::{VerifiedDeploymentEvidence, VerifiedProgramHead};

const DOMAIN: &[u8] = b"LayerX/program-interface/v1\0";
const STATE_PREFIX: &[u8] = b"interface\0";
const MAX_INTERFACE_BYTES: usize = 952;
const MAX_ENTRIES: usize = 256;
const MAX_FIELDS: usize = 256;
const MAX_DEPTH: usize = 16;
const MAX_NAME: usize = 128;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct InterfaceDigest([u8; 32]);

impl InterfaceDigest {
    #[must_use]
    pub const fn new(bytes: [u8; 32]) -> Self { Self(bytes) }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] { &self.0 }

    #[must_use]
    pub const fn into_bytes(self) -> [u8; 32] { self.0 }
}

impl From<[u8; 32]> for InterfaceDigest {
    fn from(bytes: [u8; 32]) -> Self { Self::new(bytes) }
}

impl From<InterfaceDigest> for [u8; 32] {
    fn from(digest: InterfaceDigest) -> Self { digest.into_bytes() }
}

impl AsRef<[u8]> for InterfaceDigest {
    fn as_ref(&self) -> &[u8] { self.as_bytes() }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum InterfaceCapability {
    StorageRead,
    StorageWrite,
    SharedStorageRead,
    SharedStorageWrite,
    EmitEvent,
    Call { program: ProgramId },
    Transfer402 { asset: [u8; 32], to: [u8; 32], maximum_amount: u128 },
    ProgramSpend { owner_program: ProgramId, seed: Vec<u8>, source_account: [u8; 32], asset: [u8; 32], to: [u8; 32], maximum_amount: u128 },
    ReceiptRead { receipt_digest: [u8; 32] },
    BalanceView { account: [u8; 32], asset: [u8; 32], receipt_digest: [u8; 32] },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValueSchema {
    pub convention: EncodingConvention,
    pub value: Option<ValueType>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValueType {
    U8,
    U16,
    U32,
    U64,
    U128,
    U256,
    I8,
    I16,
    I32,
    I64,
    I128,
    Bytes { max_len: u32 },
    FixedArray { item: Box<Self>, length: u32 },
    VariableArray { item: Box<Self>, max_items: u32 },
    Option(Box<Self>),
    Union(Vec<SchemaVariant>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaVariant {
    pub tag: u32,
    pub value: ValueType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypedFailure {
    pub code: u32,
    pub name: String,
    pub detail: ValueSchema,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InterfaceEntryPoint {
    pub name: String,
    pub discriminator: [u8; 4],
    pub calldata: ValueSchema,
    pub response: ValueSchema,
    pub capabilities: Vec<InterfaceCapability>,
    pub event_topics: Vec<[u8; 32]>,
    pub failures: Vec<TypedFailure>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramInterface {
    code_hash: [u8; 32],
    abi_version: u16,
    entries: Vec<InterfaceEntryPoint>,
    encoding: Vec<u8>,
    digest: InterfaceDigest,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedInterfaceRead {
    pub interface: ProgramInterface,
    pub program: ProgramId,
    pub version: u32,
    pub receipt_digest: [u8; 32],
    pub state_root: [u8; 32],
    pub freshness: crate::ReadFreshness,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InterfaceStateWitness {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
    pub proof: StateProof,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InterfaceRefusal {
    Invalid,
    NonCanonical,
    CodeHashMismatch,
    UnsupportedAbi,
    ModuleRejected,
    MissingExport,
    StateProof,
    ProgramMismatch,
    VersionMismatch,
    InterfaceAbsent,
    NarrowingUpgrade,
}

impl Display for InterfaceRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Invalid => "program interface is invalid",
            Self::NonCanonical => "program interface encoding is not canonical",
            Self::CodeHashMismatch => "program interface is bound to a different code hash",
            Self::UnsupportedAbi => "program interface uses an unsupported ABI",
            Self::ModuleRejected => "program module was rejected before interface binding",
            Self::MissingExport => "declared interface entry point is not exported by the module",
            Self::StateProof => "program interface is not proven in the receipt state root",
            Self::ProgramMismatch => "program interface proof names a different program",
            Self::VersionMismatch => "program interface proof names a different version",
            Self::InterfaceAbsent => "deployment evidence does not publish an interface",
            Self::NarrowingUpgrade => "interface upgrade narrows the published contract",
        })
    }
}

impl std::error::Error for InterfaceRefusal {}

impl ProgramInterface {
    pub fn bind_deployment(
        deployment: &VerifiedDeploymentEvidence,
        entries: Vec<InterfaceEntryPoint>,
    ) -> Result<Self, InterfaceRefusal> {
        if !deployment.interface_present() {
            return Err(InterfaceRefusal::InterfaceAbsent);
        }
        let interface = Self::bind(deployment.module(), deployment.abi_version(), entries)?;
        if interface.code_hash != deployment.code_hash() {
            return Err(InterfaceRefusal::CodeHashMismatch);
        }
        Ok(interface)
    }

    pub fn bind_verified_upgrade(
        deployment: &VerifiedDeploymentEvidence,
        entries: Vec<InterfaceEntryPoint>,
        prior: &Self,
        breaking: bool,
    ) -> Result<Self, InterfaceRefusal> {
        let interface = Self::bind_deployment(deployment, entries)?;
        interface.authorize_upgrade(prior, breaking)?;
        Ok(interface)
    }

    pub fn bind(
        module: &[u8],
        abi_version: u16,
        entries: Vec<InterfaceEntryPoint>,
    ) -> Result<Self, InterfaceRefusal> {
        let code_hash = sha256(module);
        let engine = WasmEngine::declared().map_err(|_| InterfaceRefusal::ModuleRejected)?;
        let validated = match abi_version {
            ABI_V1_VERSION => engine.validate(module),
            ABI_V2_VERSION => engine.validate_v2(module),
            _ => return Err(InterfaceRefusal::UnsupportedAbi),
        }
        .map_err(|_| InterfaceRefusal::ModuleRejected)?;
        validate_entries(&entries)?;
        if entries
            .iter()
            .any(|entry| !validated.supports_interface_entrypoint(&entry.name))
        {
            return Err(InterfaceRefusal::MissingExport);
        }
        if entries.iter().any(|entry| !validated.interface_capability_mask_matches(
            &entry.name,
            capability_mask(&entry.capabilities),
        )) {
            return Err(InterfaceRefusal::Invalid);
        }
        Self::from_parts(code_hash, abi_version, entries)
    }

    pub fn bind_upgrade(
        module: &[u8],
        abi_version: u16,
        entries: Vec<InterfaceEntryPoint>,
        prior: &Self,
        breaking: bool,
    ) -> Result<Self, InterfaceRefusal> {
        let interface = Self::bind(module, abi_version, entries)?;
        interface.authorize_upgrade(prior, breaking)?;
        Ok(interface)
    }

    fn from_parts(
        code_hash: [u8; 32],
        abi_version: u16,
        entries: Vec<InterfaceEntryPoint>,
    ) -> Result<Self, InterfaceRefusal> {
        if code_hash == [0; 32] || !matches!(abi_version, ABI_V1_VERSION | ABI_V2_VERSION) {
            return Err(InterfaceRefusal::Invalid);
        }
        validate_entries(&entries)?;
        let encoding = encode_interface(code_hash, abi_version, &entries);
        if encoding.len() > MAX_INTERFACE_BYTES {
            return Err(InterfaceRefusal::Invalid);
        }
        let digest = InterfaceDigest::new(sha256(&encoding));
        Ok(Self { code_hash, abi_version, entries, encoding, digest })
    }

    #[must_use]
    pub const fn code_hash(&self) -> [u8; 32] { self.code_hash }
    #[must_use]
    pub const fn abi_version(&self) -> u16 { self.abi_version }
    #[must_use]
    pub fn entries(&self) -> &[InterfaceEntryPoint] { &self.entries }
    #[must_use]
    pub fn canonical_encoding(&self) -> &[u8] { &self.encoding }
    #[must_use]
    pub const fn digest(&self) -> InterfaceDigest { self.digest }

    pub fn encode_call(&self, entry_name: &str, payload: &[u8]) -> Result<Vec<u8>, InterfaceRefusal> {
        let entry = self.entries.iter().find(|entry| entry.name == entry_name)
            .ok_or(InterfaceRefusal::Invalid)?;
        let mut calldata = Vec::with_capacity(4 + payload.len());
        calldata.extend_from_slice(&entry.discriminator);
        calldata.extend_from_slice(payload);
        Ok(calldata)
    }

    pub fn decode_call<'a>(&self, calldata: &'a [u8]) -> Result<(&InterfaceEntryPoint, &'a [u8]), InterfaceRefusal> {
        let discriminator: [u8; 4] = calldata.get(..4).ok_or(InterfaceRefusal::Invalid)?
            .try_into().map_err(|_| InterfaceRefusal::Invalid)?;
        let entry = self.entries.iter().find(|entry| entry.discriminator == discriminator)
            .ok_or(InterfaceRefusal::Invalid)?;
        Ok((entry, &calldata[4..]))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, InterfaceRefusal> {
        if bytes.len() > MAX_INTERFACE_BYTES || bytes.get(..DOMAIN.len()) != Some(DOMAIN) {
            return Err(InterfaceRefusal::NonCanonical);
        }
        let mut cursor = DOMAIN.len();
        let code_hash = take::<32>(bytes, &mut cursor)?;
        let abi_version = u16::from_be_bytes(take::<2>(bytes, &mut cursor)?);
        let count = usize::try_from(u16::from_be_bytes(take::<2>(bytes, &mut cursor)?))
            .map_err(|_| InterfaceRefusal::NonCanonical)?;
        if count == 0 || count > MAX_ENTRIES { return Err(InterfaceRefusal::Invalid); }
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count { entries.push(decode_entry(bytes, &mut cursor)?); }
        if cursor != bytes.len() { return Err(InterfaceRefusal::NonCanonical); }
        let result = Self::from_parts(code_hash, abi_version, entries)?;
        if result.encoding != bytes { return Err(InterfaceRefusal::NonCanonical); }
        Ok(result)
    }

    #[must_use]
    pub fn is_widening_of(&self, prior: &Self) -> bool {
        self.abi_version == prior.abi_version && prior.entries.iter().all(|old| {
            self.entries.iter().find(|new| new.name == old.name).is_some_and(|new| {
                new.discriminator == old.discriminator
                    && new.calldata.accepts_all(&old.calldata)
                    && old.response.accepts_all(&new.response)
                    && new.capabilities.iter().all(|cap| old.capabilities.contains(cap))
                    && old.event_topics.iter().all(|topic| new.event_topics.contains(topic))
                    && old.failures.iter().all(|failure| new.failures.contains(failure))
            })
        })
    }

    pub fn authorize_upgrade(&self, prior: &Self, breaking: bool) -> Result<(), InterfaceRefusal> {
        if self.is_widening_of(prior) || breaking { Ok(()) } else { Err(InterfaceRefusal::NarrowingUpgrade) }
    }
}

impl ValueSchema {
    #[must_use]
    pub const fn layerx(value: ValueType) -> Self {
        Self { convention: EncodingConvention::LayerX, value: Some(value) }
    }

    #[must_use]
    pub const fn evm_head_only() -> Self {
        Self { convention: EncodingConvention::EvmHeadOnly, value: None }
    }

    fn accepts_all(&self, prior: &Self) -> bool {
        self.convention == prior.convention && match (&self.value, &prior.value) {
            (None, None) => true,
            (Some(new), Some(old)) => new.accepts_all(old),
            _ => false,
        }
    }
}

impl ValueType {
    fn accepts_all(&self, prior: &Self) -> bool {
        match (self, prior) {
            (Self::U8, Self::U8) | (Self::U16, Self::U16) | (Self::U32, Self::U32)
            | (Self::U64, Self::U64) | (Self::U128, Self::U128) | (Self::U256, Self::U256)
            | (Self::I8, Self::I8) | (Self::I16, Self::I16) | (Self::I32, Self::I32)
            | (Self::I64, Self::I64) | (Self::I128, Self::I128) => true,
            (Self::Bytes { max_len: a }, Self::Bytes { max_len: b }) => a >= b,
            (Self::FixedArray { item: a, length: al }, Self::FixedArray { item: b, length: bl }) => al == bl && a.accepts_all(b),
            (Self::VariableArray { item: a, max_items: am }, Self::VariableArray { item: b, max_items: bm }) => am >= bm && a.accepts_all(b),
            (Self::Option(a), Self::Option(b)) => a.accepts_all(b),
            (Self::Union(a), Self::Union(b)) => b.iter().all(|old| a.iter().any(|new| new.tag == old.tag && new.value.accepts_all(&old.value))),
            _ => false,
        }
    }
}

pub fn interface_state_key(program: ProgramId) -> Vec<u8> {
    let mut key = Vec::with_capacity(STATE_PREFIX.len() + 32);
    key.extend_from_slice(STATE_PREFIX);
    key.extend_from_slice(&program.bytes());
    key
}

pub fn interface_state_value(program: ProgramId, version: u32, interface: &ProgramInterface) -> Result<Vec<u8>, InterfaceRefusal> {
    if version == 0 {
        return Err(InterfaceRefusal::VersionMismatch);
    }
    let mut value = Vec::with_capacity(interface.encoding.len() + 72);
    value.extend_from_slice(&program.bytes());
    value.extend_from_slice(&version.to_be_bytes());
    value.extend_from_slice(interface.digest.as_bytes());
    put_bytes(&mut value, &interface.encoding);
    Ok(value)
}

pub fn verify_interface_read(
    head: &VerifiedProgramHead,
    witness: &InterfaceStateWitness,
) -> Result<VerifiedInterfaceRead, InterfaceRefusal> {
    if witness.key != interface_state_key(head.program()) {
        return Err(InterfaceRefusal::ProgramMismatch);
    }
    verify_state_membership(&witness.key, &witness.value, &witness.proof, head.programs_root())
        .map_err(|_| InterfaceRefusal::StateProof)?;
    let mut cursor = 0;
    let program = ProgramId::new(take::<32>(&witness.value, &mut cursor)?)
        .map_err(|_| InterfaceRefusal::ProgramMismatch)?;
    let version = u32::from_be_bytes(take::<4>(&witness.value, &mut cursor)?);
    let digest = InterfaceDigest::new(take::<32>(&witness.value, &mut cursor)?);
    let encoded = take_bytes(&witness.value, &mut cursor)?;
    if cursor != witness.value.len() || program != head.program() { return Err(InterfaceRefusal::ProgramMismatch); }
    if version != head.version() { return Err(InterfaceRefusal::VersionMismatch); }
    let interface = ProgramInterface::decode(encoded)?;
    if interface.digest != digest || interface.code_hash != head.code_hash() || interface.abi_version != head.abi_version() {
        return Err(InterfaceRefusal::CodeHashMismatch);
    }
    Ok(VerifiedInterfaceRead { interface, program, version, receipt_digest: head.receipt_digest(), state_root: head.state_root(), freshness: head.freshness() })
}

fn validate_entries(entries: &[InterfaceEntryPoint]) -> Result<(), InterfaceRefusal> {
    if entries.is_empty() || entries.len() > MAX_ENTRIES { return Err(InterfaceRefusal::Invalid); }
    let mut names = BTreeSet::new();
    let mut discriminators = BTreeSet::new();
    for entry in entries {
        validate_name(&entry.name)?;
        if !names.insert(entry.name.as_str()) || !discriminators.insert(entry.discriminator) { return Err(InterfaceRefusal::Invalid); }
        validate_schema(&entry.calldata, 0)?;
        validate_schema(&entry.response, 0)?;
        if entry.capabilities.len() > MAX_FIELDS || entry.event_topics.len() > MAX_FIELDS || entry.failures.len() > MAX_FIELDS
            || !capabilities_canonically_sorted(&entry.capabilities) || !strictly_sorted(&entry.event_topics) { return Err(InterfaceRefusal::Invalid); }
        if entry.capabilities.iter().any(|capability| !capability_valid(capability)) { return Err(InterfaceRefusal::Invalid); }
        let mut codes = None;
        for failure in &entry.failures {
            validate_name(&failure.name)?; validate_schema(&failure.detail, 0)?;
            if codes.is_some_and(|code| code >= failure.code) { return Err(InterfaceRefusal::Invalid); }
            codes = Some(failure.code);
        }
    }
    if !entries.windows(2).all(|pair| pair[0].name < pair[1].name) { return Err(InterfaceRefusal::Invalid); }
    Ok(())
}

fn capability_valid(capability: &InterfaceCapability) -> bool {
    match capability {
        InterfaceCapability::Transfer402 { asset, to, maximum_amount } => asset != &[0;32] && to != &[0;32] && *maximum_amount != 0,
        InterfaceCapability::ProgramSpend { seed, source_account, asset, to, maximum_amount, .. } => !seed.is_empty() && seed.len() <= 256 && source_account != &[0;32] && asset != &[0;32] && to != &[0;32] && *maximum_amount != 0,
        InterfaceCapability::ReceiptRead { receipt_digest } => receipt_digest != &[0;32],
        InterfaceCapability::BalanceView { account, asset, receipt_digest } => account != &[0;32] && asset != &[0;32] && receipt_digest != &[0;32],
        _ => true,
    }
}

fn capability_mask(capabilities: &[InterfaceCapability]) -> u16 {
    capabilities.iter().fold(0_u16, |mask, capability| mask | (1 << match capability {
        InterfaceCapability::StorageRead => 0, InterfaceCapability::StorageWrite => 1,
        InterfaceCapability::SharedStorageRead => 2, InterfaceCapability::SharedStorageWrite => 3,
        InterfaceCapability::EmitEvent => 4, InterfaceCapability::Call { .. } => 5,
        InterfaceCapability::Transfer402 { .. } => 6, InterfaceCapability::ProgramSpend { .. } => 7,
        InterfaceCapability::ReceiptRead { .. } => 8, InterfaceCapability::BalanceView { .. } => 9,
    }))
}

fn validate_schema(schema: &ValueSchema, depth: usize) -> Result<(), InterfaceRefusal> {
    match (schema.convention, &schema.value) {
        (EncodingConvention::LayerX, Some(value)) => validate_value_type(value, depth),
        (EncodingConvention::EvmHeadOnly, None) => Ok(()),
        _ => Err(InterfaceRefusal::Invalid),
    }
}

fn validate_value_type(value: &ValueType, depth: usize) -> Result<(), InterfaceRefusal> {
    if depth > MAX_DEPTH { return Err(InterfaceRefusal::Invalid); }
    match value {
        ValueType::Bytes { max_len } if *max_len == 0 => Err(InterfaceRefusal::Invalid),
        ValueType::FixedArray { item, length } => { if *length == 0 { return Err(InterfaceRefusal::Invalid); } validate_value_type(item, depth + 1) }
        ValueType::VariableArray { item, max_items } => { if *max_items == 0 { return Err(InterfaceRefusal::Invalid); } validate_value_type(item, depth + 1) }
        ValueType::Option(item) => validate_value_type(item, depth + 1),
        ValueType::Union(variants) => {
            if variants.is_empty() || variants.len() > MAX_FIELDS || !variants.windows(2).all(|p| p[0].tag < p[1].tag) { return Err(InterfaceRefusal::Invalid); }
            for variant in variants { validate_value_type(&variant.value, depth + 1)?; }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn validate_name(name: &str) -> Result<(), InterfaceRefusal> {
    if name.is_empty() || name.len() > MAX_NAME || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') { Err(InterfaceRefusal::Invalid) } else { Ok(()) }
}

fn strictly_sorted<T: Ord>(items: &[T]) -> bool { items.windows(2).all(|p| p[0] < p[1]) }
fn capabilities_canonically_sorted(items: &[InterfaceCapability]) -> bool {
    items.windows(2).all(|pair| {
        let mut left = Vec::new();
        let mut right = Vec::new();
        encode_capability(&mut left, &pair[0]);
        encode_capability(&mut right, &pair[1]);
        left < right
    })
}

fn encode_interface(code_hash: [u8; 32], abi: u16, entries: &[InterfaceEntryPoint]) -> Vec<u8> {
    let mut out = Vec::new(); out.extend_from_slice(DOMAIN); out.extend_from_slice(&code_hash); out.extend_from_slice(&abi.to_be_bytes()); out.extend_from_slice(&(entries.len() as u16).to_be_bytes());
    for entry in entries { put_text(&mut out, &entry.name); out.extend_from_slice(&entry.discriminator); encode_schema(&mut out, &entry.calldata); encode_schema(&mut out, &entry.response); out.extend_from_slice(&(entry.capabilities.len() as u16).to_be_bytes()); for cap in &entry.capabilities { encode_capability(&mut out, cap); } out.extend_from_slice(&(entry.event_topics.len() as u16).to_be_bytes()); for topic in &entry.event_topics { out.extend_from_slice(topic); } out.extend_from_slice(&(entry.failures.len() as u16).to_be_bytes()); for failure in &entry.failures { out.extend_from_slice(&failure.code.to_be_bytes()); put_text(&mut out, &failure.name); encode_schema(&mut out, &failure.detail); } }
    out
}

fn encode_schema(out: &mut Vec<u8>, schema: &ValueSchema) {
    out.push(schema.convention.tag());
    if let Some(value) = &schema.value { encode_value_type(out, value); }
}

fn encode_value_type(out: &mut Vec<u8>, value: &ValueType) {
    match value {
        ValueType::U8 => out.push(TypeTag::U8 as u8), ValueType::U16 => out.push(TypeTag::U16 as u8),
        ValueType::U32 => out.push(TypeTag::U32 as u8), ValueType::U64 => out.push(TypeTag::U64 as u8),
        ValueType::U128 => out.push(TypeTag::U128 as u8), ValueType::U256 => out.push(TypeTag::U256 as u8),
        ValueType::I8 => out.push(TypeTag::I8 as u8), ValueType::I16 => out.push(TypeTag::I16 as u8),
        ValueType::I32 => out.push(TypeTag::I32 as u8), ValueType::I64 => out.push(TypeTag::I64 as u8),
        ValueType::I128 => out.push(TypeTag::I128 as u8),
        ValueType::Bytes { max_len } => { out.push(TypeTag::Bytes as u8); out.extend_from_slice(&max_len.to_be_bytes()); }
        ValueType::FixedArray { item, length } => { out.push(TypeTag::FixedArray as u8); out.extend_from_slice(&length.to_be_bytes()); encode_value_type(out, item); }
        ValueType::VariableArray { item, max_items } => { out.push(TypeTag::VariableArray as u8); out.extend_from_slice(&max_items.to_be_bytes()); encode_value_type(out, item); }
        ValueType::Option(item) => { out.push(TypeTag::Option as u8); encode_value_type(out, item); }
        ValueType::Union(variants) => { out.push(TypeTag::Union as u8); out.extend_from_slice(&(variants.len() as u16).to_be_bytes()); for variant in variants { out.extend_from_slice(&variant.tag.to_be_bytes()); encode_value_type(out, &variant.value); } }
    }
}

fn decode_entry(bytes: &[u8], cursor: &mut usize) -> Result<InterfaceEntryPoint, InterfaceRefusal> {
    let name = take_text(bytes, cursor)?; let discriminator = take::<4>(bytes, cursor)?; let calldata = decode_schema(bytes, cursor, 0)?; let response = decode_schema(bytes, cursor, 0)?;
    let capabilities = take_count(bytes, cursor, decode_capability)?;
    let event_topics = take_count(bytes, cursor, |bytes, cursor| take::<32>(bytes, cursor))?;
    let failures = take_count(bytes, cursor, |bytes, cursor| Ok(TypedFailure { code: u32::from_be_bytes(take::<4>(bytes, cursor)?), name: take_text(bytes, cursor)?, detail: decode_schema(bytes, cursor, 0)? }))?;
    Ok(InterfaceEntryPoint { name, discriminator, calldata, response, capabilities, event_topics, failures })
}

fn decode_schema(bytes: &[u8], cursor: &mut usize, depth: usize) -> Result<ValueSchema, InterfaceRefusal> {
    if depth > MAX_DEPTH { return Err(InterfaceRefusal::Invalid); }
    let convention = EncodingConvention::from_tag(take::<1>(bytes, cursor)?[0]).map_err(|_| InterfaceRefusal::NonCanonical)?;
    let value = match convention { EncodingConvention::LayerX => Some(decode_value_type(bytes, cursor, depth)?), EncodingConvention::EvmHeadOnly => None };
    Ok(ValueSchema { convention, value })
}

fn decode_value_type(bytes: &[u8], cursor: &mut usize, depth: usize) -> Result<ValueType, InterfaceRefusal> {
    if depth > MAX_DEPTH { return Err(InterfaceRefusal::Invalid); }
    Ok(match TypeTag::from_byte(take::<1>(bytes, cursor)?[0]).map_err(|_| InterfaceRefusal::NonCanonical)? {
        TypeTag::U8 => ValueType::U8, TypeTag::U16 => ValueType::U16, TypeTag::U32 => ValueType::U32,
        TypeTag::U64 => ValueType::U64, TypeTag::U128 => ValueType::U128, TypeTag::U256 => ValueType::U256,
        TypeTag::I8 => ValueType::I8, TypeTag::I16 => ValueType::I16, TypeTag::I32 => ValueType::I32,
        TypeTag::I64 => ValueType::I64, TypeTag::I128 => ValueType::I128,
        TypeTag::Bytes => ValueType::Bytes { max_len: u32::from_be_bytes(take::<4>(bytes, cursor)?) },
        TypeTag::FixedArray => ValueType::FixedArray { length: u32::from_be_bytes(take::<4>(bytes, cursor)?), item: Box::new(decode_value_type(bytes, cursor, depth + 1)?) },
        TypeTag::VariableArray => ValueType::VariableArray { max_items: u32::from_be_bytes(take::<4>(bytes, cursor)?), item: Box::new(decode_value_type(bytes, cursor, depth + 1)?) },
        TypeTag::Option => ValueType::Option(Box::new(decode_value_type(bytes, cursor, depth + 1)?)),
        TypeTag::Union => ValueType::Union(take_count(bytes, cursor, |bytes, cursor| Ok(SchemaVariant { tag: u32::from_be_bytes(take::<4>(bytes, cursor)?), value: decode_value_type(bytes, cursor, depth + 1)? }))?),
    })
}

fn encode_capability(out: &mut Vec<u8>, capability: &InterfaceCapability) {
    match capability {
        InterfaceCapability::StorageRead => out.push(0), InterfaceCapability::StorageWrite => out.push(1),
        InterfaceCapability::SharedStorageRead => out.push(2), InterfaceCapability::SharedStorageWrite => out.push(3), InterfaceCapability::EmitEvent => out.push(4),
        InterfaceCapability::Call { program } => { out.push(5); out.extend_from_slice(&program.bytes()); }
        InterfaceCapability::Transfer402 { asset, to, maximum_amount } => { out.push(6); out.extend_from_slice(asset); out.extend_from_slice(to); out.extend_from_slice(&maximum_amount.to_be_bytes()); }
        InterfaceCapability::ProgramSpend { owner_program, seed, source_account, asset, to, maximum_amount } => { out.push(7); out.extend_from_slice(&owner_program.bytes()); put_bytes16(out, seed); out.extend_from_slice(source_account); out.extend_from_slice(asset); out.extend_from_slice(to); out.extend_from_slice(&maximum_amount.to_be_bytes()); }
        InterfaceCapability::ReceiptRead { receipt_digest } => { out.push(8); out.extend_from_slice(receipt_digest); }
        InterfaceCapability::BalanceView { account, asset, receipt_digest } => { out.push(9); out.extend_from_slice(account); out.extend_from_slice(asset); out.extend_from_slice(receipt_digest); }
    }
}

fn decode_capability(bytes: &[u8], cursor: &mut usize) -> Result<InterfaceCapability, InterfaceRefusal> {
    Ok(match take::<1>(bytes, cursor)?[0] {
        0 => InterfaceCapability::StorageRead, 1 => InterfaceCapability::StorageWrite, 2 => InterfaceCapability::SharedStorageRead, 3 => InterfaceCapability::SharedStorageWrite, 4 => InterfaceCapability::EmitEvent,
        5 => InterfaceCapability::Call { program: ProgramId::new(take::<32>(bytes, cursor)?).map_err(|_| InterfaceRefusal::Invalid)? },
        6 => InterfaceCapability::Transfer402 { asset: take::<32>(bytes, cursor)?, to: take::<32>(bytes, cursor)?, maximum_amount: u128::from_be_bytes(take::<16>(bytes, cursor)?) },
        7 => { let owner_program=ProgramId::new(take::<32>(bytes,cursor)?).map_err(|_| InterfaceRefusal::Invalid)?; let seed=take_bytes16(bytes,cursor)?; InterfaceCapability::ProgramSpend { owner_program, seed, source_account:take::<32>(bytes,cursor)?, asset:take::<32>(bytes,cursor)?, to:take::<32>(bytes,cursor)?, maximum_amount:u128::from_be_bytes(take::<16>(bytes,cursor)?) } },
        8 => InterfaceCapability::ReceiptRead { receipt_digest: take::<32>(bytes,cursor)? },
        9 => InterfaceCapability::BalanceView { account:take::<32>(bytes,cursor)?, asset:take::<32>(bytes,cursor)?, receipt_digest:take::<32>(bytes,cursor)? },
        _ => return Err(InterfaceRefusal::NonCanonical),
    })
}

fn put_text(out: &mut Vec<u8>, text: &str) { out.extend_from_slice(&(text.len() as u16).to_be_bytes()); out.extend_from_slice(text.as_bytes()); }
fn put_bytes(out: &mut Vec<u8>, bytes: &[u8]) { out.extend_from_slice(&(bytes.len() as u32).to_be_bytes()); out.extend_from_slice(bytes); }
fn put_bytes16(out: &mut Vec<u8>, bytes: &[u8]) { out.extend_from_slice(&(bytes.len() as u16).to_be_bytes()); out.extend_from_slice(bytes); }
fn take<const N: usize>(bytes: &[u8], cursor: &mut usize) -> Result<[u8; N], InterfaceRefusal> { let end = cursor.checked_add(N).ok_or(InterfaceRefusal::NonCanonical)?; let value = bytes.get(*cursor..end).ok_or(InterfaceRefusal::NonCanonical)?.try_into().map_err(|_| InterfaceRefusal::NonCanonical)?; *cursor = end; Ok(value) }
fn take_text(bytes: &[u8], cursor: &mut usize) -> Result<String, InterfaceRefusal> { let len = usize::from(u16::from_be_bytes(take::<2>(bytes, cursor)?)); let end = cursor.checked_add(len).ok_or(InterfaceRefusal::NonCanonical)?; let value = core::str::from_utf8(bytes.get(*cursor..end).ok_or(InterfaceRefusal::NonCanonical)?).map_err(|_| InterfaceRefusal::NonCanonical)?.to_owned(); *cursor = end; Ok(value) }
fn take_bytes<'a>(bytes: &'a [u8], cursor: &mut usize) -> Result<&'a [u8], InterfaceRefusal> { let len = usize::try_from(u32::from_be_bytes(take::<4>(bytes, cursor)?)).map_err(|_| InterfaceRefusal::NonCanonical)?; let end = cursor.checked_add(len).ok_or(InterfaceRefusal::NonCanonical)?; let value = bytes.get(*cursor..end).ok_or(InterfaceRefusal::NonCanonical)?; *cursor = end; Ok(value) }
fn take_bytes16(bytes: &[u8], cursor: &mut usize) -> Result<Vec<u8>, InterfaceRefusal> { let len=usize::from(u16::from_be_bytes(take::<2>(bytes,cursor)?)); let end=cursor.checked_add(len).ok_or(InterfaceRefusal::NonCanonical)?; let value=bytes.get(*cursor..end).ok_or(InterfaceRefusal::NonCanonical)?.to_vec(); *cursor=end; Ok(value) }
fn take_count<T>(bytes: &[u8], cursor: &mut usize, mut decode: impl FnMut(&[u8], &mut usize) -> Result<T, InterfaceRefusal>) -> Result<Vec<T>, InterfaceRefusal> { let count = usize::from(u16::from_be_bytes(take::<2>(bytes, cursor)?)); if count > MAX_FIELDS { return Err(InterfaceRefusal::Invalid); } let mut values = Vec::with_capacity(count); for _ in 0..count { values.push(decode(bytes, cursor)?); } Ok(values) }

#[cfg(test)]
mod conformance_vectors {
    use super::*;

    const CALLABLE_MODULE: &[u8] = &[
        0,97,115,109,1,0,0,0,1,12,2,96,2,127,127,1,127,96,1,127,1,127,
        3,3,2,0,1,5,3,1,0,1,7,34,3,4,b'c',b'a',b'l',b'l',0,0,
        14,b'l',b'a',b'y',b'e',b'r',b'x',b'_',b'r',b'e',b's',b'e',b'r',b'v',b'e',0,1,
        6,b'm',b'e',b'm',b'o',b'r',b'y',2,0,10,11,2,4,0,65,0,11,4,0,65,0,11,
    ];
    const CALL_ONLY_MODULE: &[u8] = &[
        0,97,115,109,1,0,0,0,1,7,1,96,2,127,127,1,127,3,2,1,0,
        7,8,1,4,b'c',b'a',b'l',b'l',0,0,10,6,1,4,0,65,0,11,
    ];

    fn entry(max: u32) -> InterfaceEntryPoint {
        InterfaceEntryPoint {
            name: "call".to_owned(),
            discriminator: [0x10, 0x20, 0x30, 0x40],
            calldata: ValueSchema::layerx(ValueType::Bytes { max_len: max }),
            response: ValueSchema::layerx(ValueType::U8),
            capabilities: vec![InterfaceCapability::StorageRead],
            event_topics: vec![[0x44; 32]],
            failures: vec![TypedFailure {
                code: 7,
                name: "denied".to_owned(),
                detail: ValueSchema::layerx(ValueType::Bytes { max_len: 64 }),
            }],
        }
    }

    #[test]
    fn canonical_interface_vector_round_trips_and_binds_real_export() {
        let interface = ProgramInterface::bind(CALLABLE_MODULE, ABI_V1_VERSION, vec![entry(64)])
            .unwrap_or_else(|error| panic!("bind interface vector: {error}"));
        let decoded = ProgramInterface::decode(interface.canonical_encoding())
            .unwrap_or_else(|error| panic!("decode interface vector: {error}"));
        assert_eq!(decoded, interface);
        assert_eq!(decoded.digest().into_bytes(), sha256(decoded.canonical_encoding()));
        assert_eq!(
            InterfaceDigest::new(decoded.digest().into_bytes()).as_bytes(),
            decoded.digest().as_bytes(),
        );
        let calldata = decoded.encode_call("call", &[1, 2, 3])
            .unwrap_or_else(|error| panic!("encode call vector: {error}"));
        assert_eq!(calldata, [0x10, 0x20, 0x30, 0x40, 1, 2, 3]);
        let (selected, payload) = decoded.decode_call(&calldata)
            .unwrap_or_else(|error| panic!("decode call vector: {error}"));
        assert_eq!(selected.name, "call");
        assert_eq!(payload, [1, 2, 3]);
        let mut reordered = decoded.canonical_encoding().to_vec();
        reordered.push(0);
        assert_eq!(ProgramInterface::decode(&reordered), Err(InterfaceRefusal::NonCanonical));
    }

    #[test]
    fn binding_refuses_a_declared_entry_absent_from_the_real_module() {
        let mut missing = entry(64);
        missing.name = "missing".to_owned();
        assert_eq!(
            ProgramInterface::bind(CALLABLE_MODULE, ABI_V1_VERSION, vec![missing]),
            Err(InterfaceRefusal::MissingExport),
        );
    }

    #[test]
    fn binding_refuses_entry_that_cannot_accept_discriminator_calldata() {
        assert_eq!(
            ProgramInterface::bind(CALL_ONLY_MODULE, ABI_V1_VERSION, vec![entry(64)]),
            Err(InterfaceRefusal::MissingExport),
        );
    }

    #[test]
    fn upgrade_widening_and_explicit_breaking_declaration_are_distinct() {
        let prior = ProgramInterface::bind(CALLABLE_MODULE, ABI_V1_VERSION, vec![entry(64)])
            .unwrap_or_else(|error| panic!("bind prior: {error}"));
        let wider = ProgramInterface::bind(CALLABLE_MODULE, ABI_V1_VERSION, vec![entry(128)])
            .unwrap_or_else(|error| panic!("bind wider: {error}"));
        let narrower = ProgramInterface::bind(CALLABLE_MODULE, ABI_V1_VERSION, vec![entry(32)])
            .unwrap_or_else(|error| panic!("bind narrower: {error}"));
        assert_eq!(wider.authorize_upgrade(&prior, false), Ok(()));
        assert_eq!(narrower.authorize_upgrade(&prior, false), Err(InterfaceRefusal::NarrowingUpgrade));
        assert_eq!(narrower.authorize_upgrade(&prior, true), Ok(()));
    }

    #[test]
    fn schema_vector_uses_the_frozen_calldata_conventions_and_type_tags() {
        let variants = vec![
            SchemaVariant { tag: 0, value: ValueType::U8 },
            SchemaVariant { tag: 1, value: ValueType::U16 },
            SchemaVariant { tag: 2, value: ValueType::U32 },
            SchemaVariant { tag: 3, value: ValueType::U64 },
            SchemaVariant { tag: 4, value: ValueType::U128 },
            SchemaVariant { tag: 5, value: ValueType::U256 },
            SchemaVariant { tag: 6, value: ValueType::I8 },
            SchemaVariant { tag: 7, value: ValueType::I16 },
            SchemaVariant { tag: 8, value: ValueType::I32 },
            SchemaVariant { tag: 9, value: ValueType::I64 },
            SchemaVariant { tag: 10, value: ValueType::I128 },
            SchemaVariant { tag: 11, value: ValueType::Bytes { max_len: 64 } },
            SchemaVariant { tag: 12, value: ValueType::FixedArray { length: 4, item: Box::new(ValueType::U8) } },
            SchemaVariant { tag: 13, value: ValueType::VariableArray { max_items: 8, item: Box::new(ValueType::U16) } },
            SchemaVariant { tag: 14, value: ValueType::Option(Box::new(ValueType::U32)) },
        ];
        let mut typed = entry(64);
        typed.calldata = ValueSchema::layerx(ValueType::Union(variants));
        typed.response = ValueSchema::evm_head_only();
        let interface = ProgramInterface::bind(CALLABLE_MODULE, ABI_V1_VERSION, vec![typed])
            .unwrap_or_else(|error| panic!("bind frozen schema vector: {error}"));
        let decoded = ProgramInterface::decode(interface.canonical_encoding())
            .unwrap_or_else(|error| panic!("decode frozen schema vector: {error}"));
        assert_eq!(decoded, interface);
        assert_eq!(decoded.entries()[0].calldata.convention, EncodingConvention::LayerX);
        assert_eq!(decoded.entries()[0].response, ValueSchema::evm_head_only());
    }
}
