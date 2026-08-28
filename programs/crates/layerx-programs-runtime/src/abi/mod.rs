//! Version-one capability ABI. Every operation checks an explicit grant from
//! the invoking activity before touching namespaced storage or producing an
//! effect for the kernel to apply.

use core::fmt::{self, Display};
use std::collections::BTreeMap;

pub mod balance;
pub(crate) mod capability;
pub mod codec;
pub mod context;
pub mod manifest;
pub mod response;
mod storage_ops;

pub use balance::{BalanceView, MAX_BALANCE_VIEW_GRANTS};
use capability::CapabilityKey;
pub use capability::{Capability, CapabilitySet};
pub use codec::{
    Calldata, CodecError, EncodingConvention, TypeTag, DECODED_SIZE_LIMIT, MAX_CALLDATA_BYTES,
    MAX_NESTING_DEPTH,
};
pub use response::{CallResponse, ResponseRefusal, MAX_CALL_RESPONSE_BYTES};
pub use storage_ops::StorageSelector;

use crate::meter::MeterRefusal;
use crate::storage::{
    NamespaceDrop, PrincipalId, ProgramId, Storage, StorageError, StorageNamespace,
};
use crate::transfer::{ProgramAuthority, ProgramFundingBinding, TransferLawError, TransferSource};

pub use crate::ABI_MANIFEST;
pub const ABI_MODULE: &str = manifest::ABI_V1_MODULE;
pub const ABI_V2_MODULE: &str = manifest::ABI_V2_MODULE;
pub const MAX_EVENT_TOPIC_BYTES: usize = 64;
pub const MAX_EVENT_DATA_BYTES: usize = 65_536;
pub const MAX_CALL_INPUT_BYTES: usize = 1_048_576;
pub const MAX_CAPABILITIES: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostFunction {
    pub name: &'static str,
    pub signature: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AbiValueType {
    I32,
    I64,
}

#[derive(Clone, Copy)]
pub(crate) struct HostFunctionType {
    pub params: &'static [AbiValueType],
    pub results: &'static [AbiValueType],
}

const I32_2: &[AbiValueType] = &[AbiValueType::I32, AbiValueType::I32];
const I32_4: &[AbiValueType] = &[AbiValueType::I32; 4];
const I32_6: &[AbiValueType] = &[AbiValueType::I32; 6];
const TRANSFER_PARAMS: &[AbiValueType] = &[
    AbiValueType::I64,
    AbiValueType::I64,
    AbiValueType::I32,
    AbiValueType::I32,
    AbiValueType::I32,
    AbiValueType::I32,
];
const I32_RESULT: &[AbiValueType] = &[AbiValueType::I32];

pub(crate) const HOST_FUNCTION_TYPES: [HostFunctionType; 7] = [
    HostFunctionType {
        params: I32_4,
        results: I32_RESULT,
    },
    HostFunctionType {
        params: I32_4,
        results: I32_RESULT,
    },
    HostFunctionType {
        params: I32_2,
        results: I32_RESULT,
    },
    HostFunctionType {
        params: I32_4,
        results: I32_RESULT,
    },
    HostFunctionType {
        params: I32_6,
        results: I32_RESULT,
    },
    HostFunctionType {
        params: TRANSFER_PARAMS,
        results: I32_RESULT,
    },
    HostFunctionType {
        params: I32_4,
        results: I32_RESULT,
    },
];

pub const HOST_FUNCTIONS: [HostFunction; 7] = [
    HostFunction {
        name: "storage_read",
        signature: "(i32,i32,i32,i32)->i32",
    },
    HostFunction {
        name: "storage_write",
        signature: "(i32,i32,i32,i32)->i32",
    },
    HostFunction {
        name: "storage_delete",
        signature: "(i32,i32)->i32",
    },
    HostFunction {
        name: "event_emit",
        signature: "(i32,i32,i32,i32)->i32",
    },
    HostFunction {
        name: "program_call",
        signature: "(i32,i32,i32,i32,i32,i32)->i32",
    },
    HostFunction {
        name: "transfer_402",
        signature: "(i64,i64,i32,i32,i32,i32)->i32",
    },
    HostFunction {
        name: "receipt_read",
        signature: "(i32,i32,i32,i32)->i32",
    },
];

/// Exact authority fixed for one invocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthorizationContext {
    principal: PrincipalId,
    capabilities: CapabilitySet,
    frame: CallFrameId,
}

impl AuthorizationContext {
    #[must_use]
    pub const fn new(principal: PrincipalId, capabilities: CapabilitySet) -> Self {
        Self {
            principal,
            capabilities,
            frame: CallFrameId::root(),
        }
    }

    #[must_use]
    pub const fn principal(&self) -> PrincipalId {
        self.principal
    }

    #[must_use]
    pub const fn capabilities(&self) -> &CapabilitySet {
        &self.capabilities
    }

    /// Returns the host-fixed frame that owns this authority. Guests never
    /// supply or alter this value.
    #[must_use]
    pub const fn frame(&self) -> CallFrameId {
        self.frame
    }

    pub(crate) const fn nested(
        principal: PrincipalId,
        capabilities: CapabilitySet,
        frame: CallFrameId,
    ) -> Self {
        Self {
            principal,
            capabilities,
            frame,
        }
    }
}

/// Opaque, host-assigned identity for one frame of an activity call graph.
/// The eight-byte path admits the declared maximum nesting depth and fan-out;
/// there is no guest-facing constructor.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CallFrameId {
    path: [u8; 8],
    depth: u8,
}

impl CallFrameId {
    #[must_use]
    pub const fn root() -> Self {
        Self {
            path: [0; 8],
            depth: 0,
        }
    }

    /// Derives a child frame for host-side call orchestration. This is never
    /// exposed through the guest ABI.
    pub fn child(self, ordinal: u32) -> Result<Self, AbiError> {
        let depth = usize::from(self.depth);
        let slot = self.path.get(depth).ok_or(AbiError::CallBounds)?;
        if ordinal == 0 || ordinal > u32::from(u8::MAX) || *slot != 0 {
            return Err(AbiError::CallBounds);
        }
        let mut path = self.path;
        path[depth] = ordinal as u8;
        Ok(Self {
            path,
            depth: self.depth.saturating_add(1),
        })
    }

    #[must_use]
    pub const fn canonical_bytes(&self) -> ([u8; 8], u8) {
        (self.path, self.depth)
    }

    /// Rebuilds a host-frame identifier from its canonical artifact form.
    #[cfg(test)]
    pub(crate) fn from_canonical(path: [u8; 8], depth: u8) -> Result<Self, AbiError> {
        let depth = usize::from(depth);
        if depth > path.len() || path[depth..].iter().any(|byte| *byte != 0) {
            return Err(AbiError::InvalidEncoding);
        }
        if path[..depth].iter().any(|byte| *byte == 0) {
            return Err(AbiError::InvalidEncoding);
        }
        Ok(Self {
            path,
            depth: depth as u8,
        })
    }
}

/// Verified receipt facts exposed without raw kernel state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReceiptView {
    pub receipt_digest: [u8; 32],
    pub result_code: i32,
    pub asset: [u8; 32],
    pub amount: u128,
    pub state_root: [u8; 32],
}

/// Core-owned boundary supplying only locally verified receipt facts.
pub trait ReceiptOracle {
    /// Returns facts only after canonical receipt and authority verification.
    ///
    /// # Errors
    ///
    /// Returns an evidence refusal when the named digest is absent or invalid.
    fn verified_receipt(&self, receipt_digest: [u8; 32]) -> Result<ReceiptView, AbiError>;

    /// Returns a balance only after Core has verified the named receipt and
    /// the account proof against its resulting state root.
    fn verified_balance(
        &self,
        _account: [u8; 32],
        _asset: [u8; 32],
        _receipt_digest: [u8; 32],
    ) -> Result<BalanceView, AbiError> {
        Err(AbiError::BalanceEvidenceUnavailable)
    }
}

/// Fail-closed production receipt boundary for executions whose declared
/// capability set contains no receipt or balance read authority.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UnavailableReceiptOracle;

impl ReceiptOracle for UnavailableReceiptOracle {
    fn verified_receipt(&self, _: [u8; 32]) -> Result<ReceiptView, AbiError> {
        Err(AbiError::ReceiptMismatch)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramEvent {
    pub program: ProgramId,
    pub principal: PrincipalId,
    pub frame: CallFrameId,
    pub topic: Vec<u8>,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramCall {
    pub caller: ProgramId,
    pub callee: ProgramId,
    pub principal: PrincipalId,
    pub caller_frame: CallFrameId,
    pub callee_frame: CallFrameId,
    pub input: Vec<u8>,
    pub capabilities: CapabilitySet,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransferRequest {
    pub program: ProgramId,
    pub principal: PrincipalId,
    pub frame: CallFrameId,
    pub source: TransferSource,
    pub asset: [u8; 32],
    pub to: [u8; 32],
    pub amount: u128,
}

impl TransferRequest {
    #[must_use]
    pub const fn program(&self) -> ProgramId {
        self.program
    }

    #[must_use]
    pub const fn frame(&self) -> CallFrameId {
        self.frame
    }

    #[must_use]
    pub const fn source(&self) -> &TransferSource {
        &self.source
    }
}

/// Effects emitted by one successfully committed ABI transaction. Monetary
/// effects remain typed requests for the kernel's 402LXP transfer primitive;
/// no balance-writing handle exists here.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AbiEffects {
    pub events: Vec<ProgramEvent>,
    pub calls: Vec<ProgramCall>,
    pub transfers: Vec<TransferRequest>,
    /// Provisional exact released-occupancy facts from committed drops. Task
    /// 29.5 owns durable occupancy accounting and nets these facts against the
    /// activity's final namespace occupancy.
    pub namespace_drops: Vec<NamespaceDrop>,
}

impl AbiEffects {
    /// Canonically commits every program event to its producing frame, so an
    /// activity receipt cannot replay an event under another program frame.
    pub fn canonical_program_event_envelope(&self) -> Result<Vec<u8>, AbiError> {
        if self.events.len() > crate::DEFAULT_MAX_CALL_GRAPH_EDGES as usize {
            return Err(AbiError::EventBounds);
        }
        let mut encoded = b"LayerX/programs/events/v1\0".to_vec();
        let event_count = u32::try_from(self.events.len()).map_err(|_| AbiError::EventBounds)?;
        encoded.extend_from_slice(&event_count.to_be_bytes());
        for event in &self.events {
            if event.topic.len() > MAX_EVENT_TOPIC_BYTES || event.data.len() > MAX_EVENT_DATA_BYTES
            {
                return Err(AbiError::EventBounds);
            }
            encoded.extend_from_slice(&event.program.bytes());
            encoded.extend_from_slice(&event.principal.bytes());
            let (path, depth) = event.frame.canonical_bytes();
            encoded.extend_from_slice(&path);
            encoded.push(depth);
            let topic_length =
                u32::try_from(event.topic.len()).map_err(|_| AbiError::EventBounds)?;
            encoded.extend_from_slice(&topic_length.to_be_bytes());
            encoded.extend_from_slice(&event.topic);
            let data_length = u32::try_from(event.data.len()).map_err(|_| AbiError::EventBounds)?;
            encoded.extend_from_slice(&data_length.to_be_bytes());
            encoded.extend_from_slice(&event.data);
        }
        Ok(encoded)
    }
}

/// Successful atomic ABI state returned to the executor for durable commit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AbiCommit {
    pub storage: Storage,
    pub effects: AbiEffects,
}

/// Stable capability-ABI refusal taxonomy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AbiError {
    WrongVersion,
    InvalidCapability,
    DuplicateCapability,
    CapabilityDenied,
    CapabilityEscalation,
    EventBounds,
    CallBounds,
    AmountBounds,
    ReceiptMismatch,
    BalanceAbsent,
    BalanceEvidenceUnavailable,
    InvalidEncoding,
    Storage(StorageError),
    Meter(MeterRefusal),
    AccessDeclaration,
}

impl Display for AbiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongVersion => formatter.write_str("program ABI version is unsupported"),
            Self::InvalidCapability => formatter.write_str("capability declaration is invalid"),
            Self::DuplicateCapability => formatter.write_str("capability is declared twice"),
            Self::CapabilityDenied => formatter.write_str("capability was not granted"),
            Self::CapabilityEscalation => formatter.write_str("capability narrowing escalates"),
            Self::EventBounds => formatter.write_str("program event exceeds ABI bounds"),
            Self::CallBounds => formatter.write_str("program call exceeds ABI bounds"),
            Self::AmountBounds => formatter.write_str("402LXP transfer amount is invalid"),
            Self::ReceiptMismatch => formatter.write_str("verified receipt facts do not match"),
            Self::BalanceAbsent => formatter.write_str("verified account or asset is absent"),
            Self::BalanceEvidenceUnavailable => {
                formatter.write_str("verified balance evidence is unavailable")
            }
            Self::InvalidEncoding => formatter.write_str("program ABI input encoding is invalid"),
            Self::Storage(error) => write!(formatter, "storage refusal: {error}"),
            Self::Meter(error) => write!(formatter, "meter refusal: {error}"),
            Self::AccessDeclaration => formatter.write_str("access falls outside the activity declaration"),
        }
    }
}

impl std::error::Error for AbiError {}

impl From<StorageError> for AbiError {
    fn from(value: StorageError) -> Self {
        Self::Storage(value)
    }
}

impl From<MeterRefusal> for AbiError {
    fn from(value: MeterRefusal) -> Self {
        Self::Meter(value)
    }
}

/// One atomic, explicitly authorised ABI transaction. Storage is an owned
/// snapshot so a trap or refusal discards every write and emitted effect.
#[derive(Clone, Debug)]
pub struct Abi {
    version: u16,
    program: ProgramId,
    authorization: AuthorizationContext,
    principal_namespace: StorageNamespace,
    shared_namespace: StorageNamespace,
    storage: Storage,
    receipts: BTreeMap<[u8; 32], ReceiptView>,
    balances: BTreeMap<([u8; 32], [u8; 32]), Result<BalanceView, AbiError>>,
    effects: AbiEffects,
    access_declaration: crate::AccessDeclaration,
}

impl Abi {
    /// Opens the version-one ABI over an atomic namespaced transaction.
    ///
    /// # Errors
    ///
    /// Refuses an ABI version other than the runtime's declared version.
    pub fn new(
        version: u16,
        program: ProgramId,
        authorization: AuthorizationContext,
        storage: Storage,
        receipts: &dyn ReceiptOracle,
    ) -> Result<Self, AbiError> {
        if manifest::manifest(version).is_none() {
            return Err(AbiError::WrongVersion);
        }
        if version == manifest::ABI_V1_VERSION
            && authorization.capabilities().has_v2_only_grant()
        {
            return Err(AbiError::InvalidCapability);
        }
        if !authorization
            .capabilities()
            .root_program_spend_is_owned_by(program)
        {
            return Err(AbiError::InvalidCapability);
        }
        let principal_namespace = StorageNamespace::principal(program, authorization.principal());
        let shared_namespace = StorageNamespace::shared(program);
        let mut verified = BTreeMap::new();
        for digest in authorization.capabilities().receipt_digests() {
            let view = receipts.verified_receipt(digest)?;
            if view.receipt_digest != digest {
                return Err(AbiError::ReceiptMismatch);
            }
            verified.insert(digest, view);
        }
        let mut balances = BTreeMap::new();
        for (account, asset, receipt_digest) in authorization.capabilities().balance_grants() {
            let evidence = receipts
                .verified_balance(account, asset, receipt_digest)
                .and_then(|view| {
                    if view.account != account
                        || view.asset != asset
                        || view.receipt_digest != receipt_digest
                        || view.state_root == [0; 32]
                        || view.observed_sequence == 0
                    {
                        Err(AbiError::ReceiptMismatch)
                    } else {
                        Ok(view)
                    }
                });
            balances.insert((account, asset), evidence);
        }
        Ok(Self {
            version,
            program,
            authorization,
            principal_namespace,
            shared_namespace,
            storage,
            receipts: verified,
            balances,
            effects: AbiEffects::default(),
            access_declaration: crate::AccessDeclaration::absent(),
        })
    }

    pub(crate) fn nested(
        version: u16,
        program: ProgramId,
        authorization: AuthorizationContext,
        storage: Storage,
        receipts: BTreeMap<[u8; 32], ReceiptView>,
        balances: BTreeMap<([u8; 32], [u8; 32]), Result<BalanceView, AbiError>>,
    ) -> Result<Self, AbiError> {
        if manifest::manifest(version).is_none() {
            return Err(AbiError::WrongVersion);
        }
        if version == manifest::ABI_V1_VERSION
            && authorization.capabilities().has_v2_only_grant()
        {
            return Err(AbiError::InvalidCapability);
        }
        let principal_namespace = StorageNamespace::principal(program, authorization.principal());
        let shared_namespace = StorageNamespace::shared(program);
        for digest in authorization.capabilities().receipt_digests() {
            if receipts.get(&digest).map(|view| view.receipt_digest) != Some(digest) {
                return Err(AbiError::ReceiptMismatch);
            }
        }
        for (account, asset, receipt_digest) in authorization.capabilities().balance_grants() {
            let Some(view) = balances.get(&(account, asset)) else {
                return Err(AbiError::BalanceEvidenceUnavailable);
            };
            if matches!(view, Ok(view) if view.receipt_digest != receipt_digest) {
                return Err(AbiError::ReceiptMismatch);
            }
        }
        Ok(Self {
            version,
            program,
            authorization,
            principal_namespace,
            shared_namespace,
            storage,
            receipts,
            balances,
            effects: AbiEffects::default(),
            access_declaration: crate::AccessDeclaration::absent(),
        })
    }

    #[must_use]
    pub const fn version(&self) -> u16 {
        self.version
    }

    pub(crate) const fn principal(&self) -> PrincipalId {
        self.authorization.principal()
    }

    pub(crate) const fn program(&self) -> ProgramId {
        self.program
    }

    pub(crate) const fn frame(&self) -> CallFrameId {
        self.authorization.frame()
    }

    /// Returns the principal-scoped namespace fixed before guest entry.
    #[must_use]
    pub const fn principal_namespace(&self) -> StorageNamespace {
        self.principal_namespace
    }

    /// Returns the program-shared namespace fixed before guest entry.
    #[must_use]
    pub const fn shared_namespace(&self) -> StorageNamespace {
        self.shared_namespace
    }

    pub(crate) fn storage_snapshot(&self) -> Storage {
        self.storage.clone()
    }

    pub(crate) fn adopt_storage(&mut self, storage: Storage) {
        self.storage = storage;
    }

    pub(crate) fn verified_receipts(&self) -> BTreeMap<[u8; 32], ReceiptView> {
        self.receipts.clone()
    }

    pub(crate) fn verified_balances(
        &self,
    ) -> BTreeMap<([u8; 32], [u8; 32]), Result<BalanceView, AbiError>> {
        self.balances.clone()
    }

    pub(crate) fn absorb(&mut self, effects: AbiEffects) {
        self.effects.events.extend(effects.events);
        self.effects.calls.extend(effects.calls);
        self.effects.transfers.extend(effects.transfers);
        self.effects.namespace_drops.extend(effects.namespace_drops);
    }

    pub(crate) fn set_access_declaration(&mut self, declaration: crate::AccessDeclaration) {
        self.access_declaration = declaration;
    }

    pub(crate) const fn access_declaration(&self) -> &crate::AccessDeclaration {
        &self.access_declaration
    }

    #[must_use]
    pub const fn host_functions() -> &'static [HostFunction; 7] {
        &HOST_FUNCTIONS
    }

    #[must_use]
    pub const fn canonical_manifest() -> &'static [u8] {
        manifest::ABI_V1_MANIFEST.as_bytes()
    }

    /// Emits one event under the current program namespace.
    ///
    /// # Errors
    ///
    /// Refuses missing authority and invalid topic/data bounds.
    pub fn emit_event(&mut self, topic: &[u8], data: &[u8]) -> Result<(), AbiError> {
        self.authorization
            .capabilities()
            .grant(&CapabilityKey::EmitEvent)?;
        if topic.is_empty()
            || topic.len() > MAX_EVENT_TOPIC_BYTES
            || data.len() > MAX_EVENT_DATA_BYTES
        {
            return Err(AbiError::EventBounds);
        }
        self.effects.events.push(ProgramEvent {
            program: self.program,
            principal: self.authorization.principal(),
            frame: self.authorization.frame(),
            topic: topic.to_vec(),
            data: data.to_vec(),
        });
        Ok(())
    }

    /// Requests a program call with an explicitly narrowed capability set.
    ///
    /// # Errors
    ///
    /// Refuses missing call authority, oversized input, or any escalation.
    pub fn call_program(
        &mut self,
        callee: ProgramId,
        input: &[u8],
        requested: impl IntoIterator<Item = Capability>,
    ) -> Result<(), AbiError> {
        let ordinal = u32::try_from(self.effects.calls.len())
            .unwrap_or(u32::MAX)
            .saturating_add(1);
        let frame = self.authorization.frame().child(ordinal)?;
        self.stage_call(callee, input, requested.into_iter().collect(), frame)?;
        Ok(())
    }

    pub(crate) fn stage_call(
        &mut self,
        callee: ProgramId,
        input: &[u8],
        requested: Vec<Capability>,
        callee_frame: CallFrameId,
    ) -> Result<CapabilitySet, AbiError> {
        self.authorization
            .capabilities()
            .grant(&CapabilityKey::Call(callee))?;
        self.access_declaration
            .enforce_call(callee)
            .map_err(|_| AbiError::AccessDeclaration)?;
        if input.len() > MAX_CALL_INPUT_BYTES {
            return Err(AbiError::CallBounds);
        }
        let capabilities = self
            .authorization
            .capabilities()
            .narrow_for_program_edge(self.program, requested)?;
        self.effects.calls.push(ProgramCall {
            caller: self.program,
            callee,
            principal: self.authorization.principal(),
            caller_frame: self.authorization.frame(),
            callee_frame,
            input: input.to_vec(),
            capabilities: capabilities.clone(),
        });
        Ok(capabilities)
    }

    /// Requests an authenticated 402LXP transfer for the kernel to apply.
    ///
    /// # Errors
    ///
    /// Refuses missing authority, a zero amount, or a grant-limit excess.
    pub fn request_transfer(
        &mut self,
        asset: [u8; 32],
        to: [u8; 32],
        amount: u128,
    ) -> Result<(), AbiError> {
        if amount == 0 {
            return Err(AbiError::AmountBounds);
        }
        let grant = self
            .authorization
            .capabilities()
            .grant(&CapabilityKey::Transfer { asset, to })?;
        let Capability::Transfer402 { maximum_amount, .. } = grant else {
            return Err(AbiError::CapabilityDenied);
        };
        if amount > *maximum_amount {
            return Err(AbiError::CapabilityEscalation);
        }
        self.access_declaration
            .enforce_account(self.authorization.principal().bytes(), asset, crate::AccessMode::Write)
            .and_then(|()| self.access_declaration.enforce_account(to, asset, crate::AccessMode::Write))
            .map_err(|_| AbiError::AccessDeclaration)?;
        self.effects.transfers.push(TransferRequest {
            program: self.program,
            principal: self.authorization.principal(),
            frame: self.authorization.frame(),
            source: TransferSource::Principal(self.authorization.principal()),
            asset,
            to,
            amount,
        });
        Ok(())
    }

    /// Requests a principal-funded transfer into the exact account derived by
    /// the currently executing program. Registry and asset binding are checked
    /// again by the kernel before any transfer or guest state commits.
    pub(crate) fn request_program_funding(
        &mut self,
        seed: &[u8],
        destination_account: [u8; 32],
        asset: [u8; 32],
        amount: u128,
    ) -> Result<(), AbiError> {
        if amount == 0 {
            return Err(AbiError::AmountBounds);
        }
        let grant = self
            .authorization
            .capabilities()
            .grant(&CapabilityKey::Transfer {
                asset,
                to: destination_account,
            })?;
        let Capability::Transfer402 { maximum_amount, .. } = grant else {
            return Err(AbiError::CapabilityDenied);
        };
        if amount > *maximum_amount {
            return Err(AbiError::CapabilityEscalation);
        }
        let binding = ProgramFundingBinding::issue(self.program, seed, destination_account, asset)
            .map_err(|_| AbiError::CapabilityDenied)?;
        self.access_declaration
            .enforce_account(self.authorization.principal().bytes(), asset, crate::AccessMode::Write)
            .and_then(|()| self.access_declaration.enforce_account(destination_account, asset, crate::AccessMode::Write))
            .map_err(|_| AbiError::AccessDeclaration)?;
        self.effects.transfers.push(TransferRequest {
            program: self.program,
            principal: self.authorization.principal(),
            frame: self.authorization.frame(),
            source: TransferSource::ProgramFunding {
                principal: self.authorization.principal(),
                binding,
            },
            asset,
            to: destination_account,
            amount,
        });
        Ok(())
    }

    /// Requests a candidate-v2 402LXP transfer from an account derived by the
    /// currently executing program. The opaque authority token is issued only
    /// after the exact source derivation and cumulative ProgramSpend grant are
    /// checked at this host-fixed frame.
    pub(crate) fn request_program_transfer(
        &mut self,
        seed: &[u8],
        source_account: [u8; 32],
        asset: [u8; 32],
        to: [u8; 32],
        amount: u128,
    ) -> Result<(), AbiError> {
        if amount == 0 {
            return Err(AbiError::AmountBounds);
        }
        let cumulative = self
            .effects
            .transfers
            .iter()
            .filter_map(|request| match &request.source {
                TransferSource::Program(authority)
                    if authority.owner_program() == self.program
                        && authority.seed() == seed
                        && authority.source_account() == source_account
                        && authority.asset() == asset
                        && authority.to() == to =>
                {
                    Some(request.amount)
                }
                _ => None,
            })
            .try_fold(amount, |total, prior| total.checked_add(prior))
            .ok_or(AbiError::AmountBounds)?;
        if !self.authorization.capabilities().permits_program_spend(
            capability::ProgramSpendAuthorization {
                staging_program: self.program,
                owner_program: self.program,
                seed,
                source_account,
                asset,
                to,
                amount: cumulative,
            },
        ) {
            return Err(AbiError::CapabilityEscalation);
        }
        let authority = ProgramAuthority::issue(
            self.program,
            seed,
            source_account,
            self.authorization.frame(),
            asset,
            to,
            amount,
        )
        .map_err(|error| match error {
            TransferLawError::InvalidProgramAuthority => AbiError::CapabilityDenied,
            _ => AbiError::InvalidEncoding,
        })?;
        self.access_declaration
            .enforce_account(source_account, asset, crate::AccessMode::Write)
            .and_then(|()| self.access_declaration.enforce_account(to, asset, crate::AccessMode::Write))
            .map_err(|_| AbiError::AccessDeclaration)?;
        self.effects.transfers.push(TransferRequest {
            program: self.program,
            principal: self.authorization.principal(),
            frame: self.authorization.frame(),
            source: TransferSource::Program(authority),
            asset,
            to,
            amount,
        });
        Ok(())
    }

    /// Reads facts through the core's receipt-verification authority.
    ///
    /// # Errors
    ///
    /// Refuses missing digest authority and all absent or mismatched evidence.
    pub fn receipt_read(&self, receipt_digest: [u8; 32]) -> Result<ReceiptView, AbiError> {
        self.authorization
            .capabilities()
            .grant(&CapabilityKey::ReceiptRead(receipt_digest))?;
        self.receipts
            .get(&receipt_digest)
            .cloned()
            .ok_or(AbiError::ReceiptMismatch)
    }

    /// Reads one proof-bound balance. The sight grant is structurally absent
    /// from both transfer-authority paths.
    pub fn balance_read(
        &self,
        account: [u8; 32],
        asset: [u8; 32],
    ) -> Result<BalanceView, AbiError> {
        self.authorization
            .capabilities()
            .grant(&CapabilityKey::BalanceView { account, asset })?;
        self.access_declaration
            .enforce_account(account, asset, crate::AccessMode::Read)
            .map_err(|_| AbiError::AccessDeclaration)?;
        self.balances
            .get(&(account, asset))
            .cloned()
            .ok_or(AbiError::BalanceAbsent)?
    }

    /// Atomically commits storage and returns effects for the kernel. Dropping
    /// an ABI transaction instead rolls all storage writes and effects back.
    #[must_use]
    pub fn commit(self) -> AbiCommit {
        AbiCommit {
            storage: self.storage,
            effects: self.effects,
        }
    }
}
