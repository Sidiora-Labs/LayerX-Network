//! Typed refusal taxonomy for the `CosmWasm` porting kit. Every refusal names
//! the contract construct that cannot be carried over unchanged, so a port
//! fails loudly at translation time instead of silently changing meaning once
//! it is deployed.

use core::fmt::{self, Display};

use layerx_programs::{ArchiveError, BuildRefusal, RegistryError};
use layerx_programs_runtime::{
    AbiError, EngineRefusal, ExecutionError, LifecycleRefusal, StorageError, TransferLawError,
    ValidationRefusal,
};

/// A refusal produced while translating or exercising a `CosmWasm` port.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PortRefusal {
    /// The address is empty or the reserved all-zero account.
    EmptyAddress,
    /// A `cw-storage-plus` namespace is empty or beyond the length its
    /// two-byte prefix can carry.
    InvalidNamespace,
    /// A composed raw key exceeds the namespaced-storage key bound.
    KeyTooLong,
    /// The state is shared: every account reads and writes the same cell. A
    /// `LayerX` namespace is `(program, principal)`, so there is no cell every
    /// account can reach.
    SharedState,
    /// The declared record or message schema is malformed, or the supplied
    /// values do not match it.
    SchemaMismatch,
    /// A `JSON` document is malformed, carries an unknown field, repeats a
    /// field, omits a declared field or exceeds the declared bounds.
    InvalidJson,
    /// The construct pays from a contract balance without supplying the
    /// corresponding registered derived-account context.
    ContractHeldBalance,
    /// The supplied owner, seed and source do not identify one derived account.
    InvalidProgramAccount,
    /// Burning would mutate supply rather than transfer conserved value.
    SupplyMutation,
    /// The construct spends another account's funds under an allowance the
    /// contract stored. Delegated spending is a capability grant.
    DelegatedSpend,
    /// The construct reads chain state or another chain: a `querier` round
    /// trip, a staking or distribution query, an `IBC` message.
    ChainQuery,
    /// A ported amount, bound or configuration value is outside its declared
    /// range.
    OutOfRange,
    /// Encoded event data exceeds the version-one event bound.
    EventDataTooLarge,
    /// An encoded event topic exceeds the version-one topic bound.
    TopicTooLarge,
    /// Encoded message data exceeds the version-one call-input bound.
    MessageTooLarge,
    /// The emitted module exceeds the runtime's declared module byte bound.
    ModuleTooLarge,
    /// The published port descriptor is malformed, incomplete or unpinned.
    InvalidDescriptor,
    /// Capability-ABI refusal.
    Abi(AbiError),
    /// Namespaced-storage refusal.
    Storage(StorageError),
    /// Deterministic-engine construction refusal.
    Engine(EngineRefusal),
    /// Deterministic-subset validation refusal.
    Validation(ValidationRefusal),
    /// Deployment or upgrade refusal.
    Lifecycle(LifecycleRefusal),
    /// Metered execution refusal.
    Execution(ExecutionError),
    /// Program registry or source-verification refusal.
    Registry(RegistryError),
    /// Canonical published-source archive refusal.
    Archive(ArchiveError),
    /// Reproducible-build pipeline refusal.
    Build(BuildRefusal),
    /// The rebuilt artifact does not reproduce the registered code hash.
    UnverifiedSource,
    /// 402LXP monetary-law refusal.
    TransferLaw(TransferLawError),
}

impl Display for PortRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyAddress => formatter.write_str("an empty address cannot be ported"),
            Self::InvalidNamespace => {
                formatter.write_str("storage namespace is empty or exceeds its bounds")
            }
            Self::KeyTooLong => formatter.write_str("composed key exceeds the storage key bound"),
            Self::SharedState => formatter.write_str("shared state has no namespaced equivalent"),
            Self::SchemaMismatch => formatter.write_str("values do not match the declared schema"),
            Self::InvalidJson => formatter.write_str("JSON document is malformed or unexpected"),
            Self::ContractHeldBalance => {
                formatter.write_str("contract-funded flow requires derived-account context")
            }
            Self::InvalidProgramAccount => {
                formatter.write_str("contract account does not match the program and seed")
            }
            Self::SupplyMutation => {
                formatter.write_str("BankMsg::Burn has no conserved 402LXP transfer equivalent")
            }
            Self::DelegatedSpend => {
                formatter.write_str("delegated spending is a 402LXP capability grant")
            }
            Self::ChainQuery => {
                formatter.write_str("chain and inter-chain queries have no ported equivalent")
            }
            Self::OutOfRange => formatter.write_str("ported value is outside its declared bound"),
            Self::EventDataTooLarge => formatter.write_str("event data exceeds the ABI bound"),
            Self::TopicTooLarge => formatter.write_str("event topic exceeds the ABI bound"),
            Self::MessageTooLarge => formatter.write_str("message data exceeds the ABI bound"),
            Self::ModuleTooLarge => formatter.write_str("ported module exceeds the byte bound"),
            Self::InvalidDescriptor => formatter.write_str("port descriptor is invalid"),
            Self::Abi(error) => write!(formatter, "ABI refusal: {error}"),
            Self::Storage(error) => write!(formatter, "storage refusal: {error}"),
            Self::Engine(error) => write!(formatter, "engine refusal: {error}"),
            Self::Validation(error) => write!(formatter, "validation refusal: {error}"),
            Self::Lifecycle(error) => write!(formatter, "lifecycle refusal: {error}"),
            Self::Execution(error) => write!(formatter, "execution refusal: {error}"),
            Self::Registry(error) => write!(formatter, "registry refusal: {error}"),
            Self::Archive(error) => write!(formatter, "published source refusal: {error}"),
            Self::Build(refusal) => write!(formatter, "reproducible build refusal: {refusal}"),
            Self::UnverifiedSource => {
                formatter.write_str("rebuilt artifact does not reproduce the registered code hash")
            }
            Self::TransferLaw(error) => write!(formatter, "monetary law refusal: {error}"),
        }
    }
}

impl std::error::Error for PortRefusal {}

impl From<AbiError> for PortRefusal {
    fn from(value: AbiError) -> Self {
        Self::Abi(value)
    }
}

impl From<StorageError> for PortRefusal {
    fn from(value: StorageError) -> Self {
        Self::Storage(value)
    }
}

impl From<EngineRefusal> for PortRefusal {
    fn from(value: EngineRefusal) -> Self {
        Self::Engine(value)
    }
}

impl From<ValidationRefusal> for PortRefusal {
    fn from(value: ValidationRefusal) -> Self {
        Self::Validation(value)
    }
}

impl From<LifecycleRefusal> for PortRefusal {
    fn from(value: LifecycleRefusal) -> Self {
        Self::Lifecycle(value)
    }
}

impl From<ExecutionError> for PortRefusal {
    fn from(value: ExecutionError) -> Self {
        Self::Execution(value)
    }
}

impl From<RegistryError> for PortRefusal {
    fn from(value: RegistryError) -> Self {
        Self::Registry(value)
    }
}

impl From<ArchiveError> for PortRefusal {
    fn from(value: ArchiveError) -> Self {
        Self::Archive(value)
    }
}

impl From<BuildRefusal> for PortRefusal {
    fn from(value: BuildRefusal) -> Self {
        Self::Build(value)
    }
}

impl From<TransferLawError> for PortRefusal {
    fn from(value: TransferLawError) -> Self {
        Self::TransferLaw(value)
    }
}
