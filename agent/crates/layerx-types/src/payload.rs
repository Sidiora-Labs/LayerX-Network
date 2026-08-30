//! Registered module activity types and their bounded canonical payload bytes.

use crate::limits::{MAX_MODULE_ACTIVITY_TYPES, MAX_PAYLOAD_BYTES};

/// The protocol module identifiers accepted by the agent boundary.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum ModuleId {
    /// Asset issuance and transfer module.
    Asset = 1,
    /// Escrow lifecycle module.
    Escrow = 2,
    /// Budget and allowance module.
    Budget = 3,
    /// Streaming payment module.
    Stream = 4,
    /// Service commerce module.
    Service = 5,
    /// Perpetual market module.
    Perps = 6,
    /// Protocol governance module.
    Governance = 7,
    /// Settlement bridge module.
    Bridge = 8,
    /// Deterministic programs module.
    Programs = 9,
}

impl ModuleId {
    /// Decodes a protocol module identifier without accepting extensions.
    ///
    /// # Errors
    ///
    /// Returns [`PayloadError::UnknownModule`] for an undeclared module.
    pub const fn from_u16(value: u16) -> Result<Self, PayloadError> {
        match value {
            1 => Ok(Self::Asset),
            2 => Ok(Self::Escrow),
            3 => Ok(Self::Budget),
            4 => Ok(Self::Stream),
            5 => Ok(Self::Service),
            6 => Ok(Self::Perps),
            7 => Ok(Self::Governance),
            8 => Ok(Self::Bridge),
            9 => Ok(Self::Programs),
            _ => Err(PayloadError::UnknownModule(value)),
        }
    }
}

/// A protocol activity type split into its module and non-zero ordinal.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ActivityType(u32);

impl ActivityType {
    /// Constructs an activity type from a closed module and non-zero ordinal.
    ///
    /// # Errors
    ///
    /// Returns [`PayloadError::ZeroOrdinal`] when `ordinal` is zero.
    pub const fn new(module: ModuleId, ordinal: u16) -> Result<Self, PayloadError> {
        if ordinal == 0 {
            return Err(PayloadError::ZeroOrdinal);
        }
        Ok(Self(((module as u32) << 16) | (ordinal as u32)))
    }

    /// Decodes the protocol's module/ordinal representation.
    ///
    /// # Errors
    ///
    /// Returns a typed error for an unknown module or zero ordinal.
    pub const fn from_u32(value: u32) -> Result<Self, PayloadError> {
        let bytes = value.to_be_bytes();
        let module = match ModuleId::from_u16(u16::from_be_bytes([bytes[0], bytes[1]])) {
            Ok(module) => module,
            Err(error) => return Err(error),
        };
        Self::new(module, u16::from_be_bytes([bytes[2], bytes[3]]))
    }

    /// Returns the canonical packed representation.
    #[must_use]
    pub const fn value(self) -> u32 {
        self.0
    }

    /// Returns the declared module component.
    #[must_use]
    pub const fn module(self) -> ModuleId {
        match ModuleId::from_u16((self.0 >> 16) as u16) {
            Ok(module) => module,
            Err(_) => unreachable!(),
        }
    }

    /// Returns the non-zero activity ordinal.
    #[must_use]
    pub const fn ordinal(self) -> u16 {
        let bytes = self.0.to_be_bytes();
        u16::from_be_bytes([bytes[2], bytes[3]])
    }
}

/// One core module's exact declared activity set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleRegistration {
    module: ModuleId,
    activity_types: Vec<ActivityType>,
}

impl ModuleRegistration {
    /// Constructs a sorted, unique registration for one module.
    ///
    /// # Errors
    ///
    /// Returns a typed error for an empty, oversized, mismatched, duplicated,
    /// or unsorted activity declaration.
    pub fn new(module: ModuleId, activity_types: &[ActivityType]) -> Result<Self, PayloadError> {
        if activity_types.is_empty() || activity_types.len() > MAX_MODULE_ACTIVITY_TYPES {
            return Err(PayloadError::RegistrationLength(activity_types.len()));
        }
        let mut previous = None;
        for activity_type in activity_types {
            if activity_type.module() != module {
                return Err(PayloadError::ModuleMismatch);
            }
            if previous.is_some_and(|value| value >= *activity_type) {
                return Err(PayloadError::UnsortedRegistration);
            }
            previous = Some(*activity_type);
        }
        Ok(Self {
            module,
            activity_types: activity_types.to_vec(),
        })
    }

    /// Returns the registered module.
    #[must_use]
    pub const fn module(&self) -> ModuleId {
        self.module
    }

    /// Returns the exact sorted activity set negotiated for this module.
    #[must_use]
    pub fn activity_types(&self) -> &[ActivityType] {
        &self.activity_types
    }

    fn declares(&self, activity_type: ActivityType) -> bool {
        self.activity_types.binary_search(&activity_type).is_ok()
    }
}

/// The module registrations negotiated from a core boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleRegistry(Vec<ModuleRegistration>);

impl ModuleRegistry {
    /// Constructs a registry with no duplicate module declarations.
    ///
    /// # Errors
    ///
    /// Returns [`PayloadError::DuplicateModule`] for a repeated module.
    pub fn new(registrations: &[ModuleRegistration]) -> Result<Self, PayloadError> {
        for (index, registration) in registrations.iter().enumerate() {
            if registrations[..index]
                .iter()
                .any(|candidate| candidate.module == registration.module)
            {
                return Err(PayloadError::DuplicateModule(registration.module));
            }
        }
        Ok(Self(registrations.to_vec()))
    }

    /// Returns the exact sorted module registrations negotiated from core.
    #[must_use]
    pub fn registrations(&self) -> &[ModuleRegistration] {
        &self.0
    }

    /// Reports whether core registered this exact module activity type.
    #[must_use]
    pub fn declares(&self, activity_type: ActivityType) -> bool {
        self.0.iter().any(|registration| {
            registration.module == activity_type.module() && registration.declares(activity_type)
        })
    }
}

/// A canonical module payload tagged by an activity declared by core.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Payload {
    /// Asset module payload.
    Asset(ActivityType, Box<[u8]>),
    /// Escrow module payload.
    Escrow(ActivityType, Box<[u8]>),
    /// Budget module payload.
    Budget(ActivityType, Box<[u8]>),
    /// Stream module payload.
    Stream(ActivityType, Box<[u8]>),
    /// Service module payload.
    Service(ActivityType, Box<[u8]>),
    /// Perpetuals module payload.
    Perps(ActivityType, Box<[u8]>),
    /// Governance module payload.
    Governance(ActivityType, Box<[u8]>),
    /// Bridge module payload.
    Bridge(ActivityType, Box<[u8]>),
    /// Programs module payload.
    Programs(ActivityType, Box<[u8]>),
}

impl Payload {
    /// Constructs a bounded payload only for an activity declared by a
    /// registered module.
    ///
    /// # Errors
    ///
    /// Returns a typed error before allocation when the byte bound is exceeded,
    /// or when no registration declares the activity.
    pub fn new(
        registry: &ModuleRegistry,
        activity_type: ActivityType,
        bytes: &[u8],
    ) -> Result<Self, PayloadError> {
        if bytes.len() > MAX_PAYLOAD_BYTES {
            return Err(PayloadError::PayloadLength(bytes.len()));
        }
        if !registry.declares(activity_type) {
            return Err(PayloadError::UndeclaredActivity(activity_type.value()));
        }
        let bytes = Box::<[u8]>::from(bytes);
        Ok(match activity_type.module() {
            ModuleId::Asset => Self::Asset(activity_type, bytes),
            ModuleId::Escrow => Self::Escrow(activity_type, bytes),
            ModuleId::Budget => Self::Budget(activity_type, bytes),
            ModuleId::Stream => Self::Stream(activity_type, bytes),
            ModuleId::Service => Self::Service(activity_type, bytes),
            ModuleId::Perps => Self::Perps(activity_type, bytes),
            ModuleId::Governance => Self::Governance(activity_type, bytes),
            ModuleId::Bridge => Self::Bridge(activity_type, bytes),
            ModuleId::Programs => Self::Programs(activity_type, bytes),
        })
    }

    /// Returns the declared activity tag.
    #[must_use]
    pub const fn activity_type(&self) -> ActivityType {
        match self {
            Self::Asset(activity_type, _)
            | Self::Escrow(activity_type, _)
            | Self::Budget(activity_type, _)
            | Self::Stream(activity_type, _)
            | Self::Service(activity_type, _)
            | Self::Perps(activity_type, _)
            | Self::Governance(activity_type, _)
            | Self::Bridge(activity_type, _)
            | Self::Programs(activity_type, _) => *activity_type,
        }
    }

    /// Borrows the exact canonical payload bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Asset(_, bytes)
            | Self::Escrow(_, bytes)
            | Self::Budget(_, bytes)
            | Self::Stream(_, bytes)
            | Self::Service(_, bytes)
            | Self::Perps(_, bytes)
            | Self::Governance(_, bytes)
            | Self::Bridge(_, bytes)
            | Self::Programs(_, bytes) => bytes,
        }
    }
}

/// Failure to construct a declared, bounded module payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PayloadError {
    /// The packed activity named a module outside the closed set.
    UnknownModule(u16),
    /// Activity ordinal zero is reserved and cannot be registered.
    ZeroOrdinal,
    /// A module registered no activities or exceeded the protocol maximum.
    RegistrationLength(usize),
    /// An activity belongs to a different module than its registration.
    ModuleMismatch,
    /// A registration was not strictly increasing and unique.
    UnsortedRegistration,
    /// A module appeared more than once in the registry.
    DuplicateModule(ModuleId),
    /// No registered module declared the activity.
    UndeclaredActivity(u32),
    /// Payload bytes exceeded the protocol maximum.
    PayloadLength(usize),
}
