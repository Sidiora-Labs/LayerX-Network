//! Typed refusal taxonomy for the Solana porting kit. Every refusal names the
//! account-model or Anchor construct that cannot be carried over unchanged, so
//! a port fails loudly at translation time instead of silently changing meaning
//! once it is deployed.

use core::fmt::{self, Display};

use layerx_programs::{ArchiveError, BuildRefusal, RegistryError};
use layerx_programs_runtime::{
    AbiError, EngineRefusal, ExecutionError, LifecycleRefusal, StorageError, TransferLawError,
    ValidationRefusal,
};

/// A refusal produced while translating or exercising a Solana port.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PortRefusal {
    /// The public key is the reserved all-zero key.
    ZeroPubkey,
    /// A seed path is empty, too long, or carries an oversized seed.
    InvalidSeeds,
    /// The published bump does not reproduce the published address.
    DerivationMismatch,
    /// Stored account data does not begin with the account's discriminator,
    /// exactly the check Anchor performs on every account load.
    DiscriminatorMismatch,
    /// The account is larger than the namespaced-storage value bound, or its
    /// encoding does not match its declared space.
    AccountBounds,
    /// The supplied field values do not match the declared account schema.
    SchemaMismatch,
    /// The construct writes lamports directly. No program holds balance-writing
    /// authority on `LayerX`.
    LamportMutation,
    /// The PDA-shaped authority does not match the LayerX program and seed.
    InvalidProgramAccount,
    /// Rent sweeping has no exact bounded amount in the source instruction.
    UnboundedRentSweep,
    /// The construct signs for a third party's funds with a program-derived
    /// authority. Delegated spending is a capability grant.
    DelegatedSpend,
    /// A ported amount, limit or immutable is outside its declared bound.
    OutOfRange,
    /// Encoded event data exceeds the version-one event bound.
    EventDataTooLarge,
    /// Encoded instruction data exceeds the version-one call-input bound.
    InstructionDataTooLarge,
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
            Self::ZeroPubkey => formatter.write_str("the all-zero public key cannot be ported"),
            Self::InvalidSeeds => formatter.write_str("seed path is empty or exceeds its bounds"),
            Self::DerivationMismatch => {
                formatter.write_str("bump does not derive the published address")
            }
            Self::DiscriminatorMismatch => {
                formatter.write_str("account data does not carry the expected discriminator")
            }
            Self::AccountBounds => formatter.write_str("account exceeds its declared space"),
            Self::SchemaMismatch => {
                formatter.write_str("field values do not match the account schema")
            }
            Self::LamportMutation => {
                formatter.write_str("direct lamport mutation has no transfer-law equivalent")
            }
            Self::InvalidProgramAccount => {
                formatter.write_str("program account does not match the program and seed")
            }
            Self::UnboundedRentSweep => {
                formatter.write_str("account close has no bounded program-spend amount")
            }
            Self::DelegatedSpend => {
                formatter.write_str("delegated spending is a 402LXP capability grant")
            }
            Self::OutOfRange => formatter.write_str("ported value is outside its declared bound"),
            Self::EventDataTooLarge => formatter.write_str("event data exceeds the ABI bound"),
            Self::InstructionDataTooLarge => {
                formatter.write_str("instruction data exceeds the ABI bound")
            }
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
