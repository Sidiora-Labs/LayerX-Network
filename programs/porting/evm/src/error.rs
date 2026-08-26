//! Typed refusal taxonomy for the EVM porting kit. Every refusal names the
//! Solidity construct that cannot be carried over unchanged, so a port fails
//! loudly at translation time instead of silently changing meaning on chain.

use core::fmt::{self, Display};

use layerx_programs::{ArchiveError, BuildRefusal, RegistryError};
use layerx_programs_runtime::{
    AbiError, EngineRefusal, ExecutionError, LifecycleRefusal, StorageError, TransferLawError,
    ValidationRefusal,
};

/// A refusal produced while translating or exercising an EVM port.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PortRefusal {
    /// The 20-byte address is the reserved zero address.
    ZeroAddress,
    /// A `uint256` word carries a value wider than the ported integer width.
    WordTooWide,
    /// A canonical ABI signature is malformed or exceeds the topic bound.
    InvalidSignature,
    /// The supplied argument count does not match the declared signature.
    ArgumentCountMismatch,
    /// Encoded event data exceeds the version-one event bound.
    EventDataTooLarge,
    /// Encoded calldata exceeds the version-one call-input bound.
    CalldataTooLarge,
    /// The construct pays from a contract balance without supplying the
    /// corresponding registered derived-account context.
    ContractHeldBalance,
    /// The supplied owner, seed and source do not identify one derived account.
    InvalidProgramAccount,
    /// A whole-balance sweep has no exact bounded amount to authorize.
    UnboundedBalanceSweep,
    /// The construct spends a third party's balance from an allowance the
    /// contract stores. Delegated spending is a capability grant.
    DelegatedSpend,
    /// A ported amount, period count or immutable is outside its declared bound.
    OutOfRange,
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
            Self::ZeroAddress => formatter.write_str("the zero address cannot be ported"),
            Self::WordTooWide => formatter.write_str("uint256 word exceeds the ported width"),
            Self::InvalidSignature => formatter.write_str("canonical ABI signature is invalid"),
            Self::ArgumentCountMismatch => {
                formatter.write_str("argument count does not match the signature")
            }
            Self::EventDataTooLarge => formatter.write_str("event data exceeds the ABI bound"),
            Self::CalldataTooLarge => formatter.write_str("calldata exceeds the ABI bound"),
            Self::ContractHeldBalance => {
                formatter.write_str("contract-funded flow requires derived-account context")
            }
            Self::InvalidProgramAccount => {
                formatter.write_str("contract account does not match the program and seed")
            }
            Self::UnboundedBalanceSweep => {
                formatter.write_str("selfdestruct sweep has no bounded program-spend amount")
            }
            Self::DelegatedSpend => {
                formatter.write_str("delegated spending is a 402LXP capability grant")
            }
            Self::OutOfRange => formatter.write_str("ported value is outside its declared bound"),
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
