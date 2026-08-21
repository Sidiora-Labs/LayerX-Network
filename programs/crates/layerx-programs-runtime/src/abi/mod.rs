//! Version-one capability ABI. Every operation checks an explicit grant from
//! the invoking activity before touching namespaced storage or producing an
//! effect for the kernel to apply.

use core::fmt::{self, Display};
use std::collections::BTreeMap;

mod capability;
pub mod response;
mod storage_ops;

use capability::CapabilityKey;
pub use capability::{Capability, CapabilitySet};
pub use response::{CallResponse, ResponseRefusal, MAX_CALL_RESPONSE_BYTES};

use crate::execute::ABI_VERSION;
use crate::meter::MeterRefusal;
use crate::storage::{PrincipalId, ProgramId, Storage, StorageError, StorageNamespace};

pub const ABI_MODULE: &str = "layerx_v1";
pub const MAX_EVENT_TOPIC_BYTES: usize = 64;
pub const MAX_EVENT_DATA_BYTES: usize = 65_536;
pub const MAX_CALL_INPUT_BYTES: usize = 1_048_576;
pub const MAX_CAPABILITIES: usize = 256;

/// Frozen version-one host-function surface. Signatures use WebAssembly value
/// names, and all values are integer-only.
pub const ABI_MANIFEST: &str = "layerx_v1\0storage_read(i32,i32,i32,i32)->i32\0storage_write(i32,i32,i32,i32)->i32\0storage_delete(i32,i32)->i32\0event_emit(i32,i32,i32,i32)->i32\0program_call(i32,i32,i32,i32,i32,i32)->i32\0transfer_402(i64,i64,i32,i32,i32,i32)->i32\0receipt_read(i32,i32,i32,i32)->i32\0";

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
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorizationContext {
    principal: PrincipalId,
    capabilities: CapabilitySet,
}

impl AuthorizationContext {
    #[must_use]
    pub const fn new(principal: PrincipalId, capabilities: CapabilitySet) -> Self {
        Self {
            principal,
            capabilities,
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramEvent {
    pub program: ProgramId,
    pub principal: PrincipalId,
    pub topic: Vec<u8>,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramCall {
    pub caller: ProgramId,
    pub callee: ProgramId,
    pub principal: PrincipalId,
    pub input: Vec<u8>,
    pub capabilities: CapabilitySet,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransferRequest {
    pub program: ProgramId,
    pub principal: PrincipalId,
    pub asset: [u8; 32],
    pub to: [u8; 32],
    pub amount: u128,
}

/// Effects emitted by one successfully committed ABI transaction. Monetary
/// effects remain typed requests for the kernel's 402LXP transfer primitive;
/// no balance-writing handle exists here.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AbiEffects {
    pub events: Vec<ProgramEvent>,
    pub calls: Vec<ProgramCall>,
    pub transfers: Vec<TransferRequest>,
}

/// Successful atomic ABI state returned to the executor for durable commit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AbiCommit {
    pub storage: Storage,
    pub effects: AbiEffects,
}

/// Stable capability-ABI refusal taxonomy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
    InvalidEncoding,
    Storage(StorageError),
    Meter(MeterRefusal),
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
            Self::InvalidEncoding => formatter.write_str("program ABI input encoding is invalid"),
            Self::Storage(error) => write!(formatter, "storage refusal: {error}"),
            Self::Meter(error) => write!(formatter, "meter refusal: {error}"),
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
    namespace: StorageNamespace,
    storage: Storage,
    receipts: BTreeMap<[u8; 32], ReceiptView>,
    effects: AbiEffects,
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
        if version != ABI_VERSION {
            return Err(AbiError::WrongVersion);
        }
        let namespace = StorageNamespace::new(program, authorization.principal());
        let mut verified = BTreeMap::new();
        for digest in authorization.capabilities().receipt_digests() {
            let view = receipts.verified_receipt(digest)?;
            if view.receipt_digest != digest {
                return Err(AbiError::ReceiptMismatch);
            }
            verified.insert(digest, view);
        }
        Ok(Self {
            version,
            program,
            authorization,
            namespace,
            storage,
            receipts: verified,
            effects: AbiEffects::default(),
        })
    }

    pub(crate) fn nested(
        version: u16,
        program: ProgramId,
        authorization: AuthorizationContext,
        storage: Storage,
        receipts: BTreeMap<[u8; 32], ReceiptView>,
    ) -> Result<Self, AbiError> {
        if version != ABI_VERSION {
            return Err(AbiError::WrongVersion);
        }
        let namespace = StorageNamespace::new(program, authorization.principal());
        for digest in authorization.capabilities().receipt_digests() {
            if receipts.get(&digest).map(|view| view.receipt_digest) != Some(digest) {
                return Err(AbiError::ReceiptMismatch);
            }
        }
        Ok(Self {
            version,
            program,
            authorization,
            namespace,
            storage,
            receipts,
            effects: AbiEffects::default(),
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

    pub(crate) fn storage_snapshot(&self) -> Storage {
        self.storage.clone()
    }

    pub(crate) fn adopt_storage(&mut self, storage: Storage) {
        self.storage = storage;
    }

    pub(crate) fn verified_receipts(&self) -> BTreeMap<[u8; 32], ReceiptView> {
        self.receipts.clone()
    }

    pub(crate) fn absorb(&mut self, effects: AbiEffects) {
        self.effects.events.extend(effects.events);
        self.effects.calls.extend(effects.calls);
        self.effects.transfers.extend(effects.transfers);
    }

    #[must_use]
    pub const fn host_functions() -> &'static [HostFunction; 7] {
        &HOST_FUNCTIONS
    }

    #[must_use]
    pub const fn canonical_manifest() -> &'static [u8] {
        ABI_MANIFEST.as_bytes()
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
        self.stage_call(callee, input, requested.into_iter().collect())?;
        Ok(())
    }

    pub(crate) fn stage_call(
        &mut self,
        callee: ProgramId,
        input: &[u8],
        requested: Vec<Capability>,
    ) -> Result<CapabilitySet, AbiError> {
        self.authorization
            .capabilities()
            .grant(&CapabilityKey::Call(callee))?;
        if input.len() > MAX_CALL_INPUT_BYTES {
            return Err(AbiError::CallBounds);
        }
        let capabilities = self.authorization.capabilities().narrow(requested)?;
        self.effects.calls.push(ProgramCall {
            caller: self.program,
            callee,
            principal: self.authorization.principal(),
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
        self.effects.transfers.push(TransferRequest {
            program: self.program,
            principal: self.authorization.principal(),
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
