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
    /// The complete, closed protocol module set in canonical identifier order.
    pub const ALL: [Self; 9] = [
        Self::Asset,
        Self::Escrow,
        Self::Budget,
        Self::Stream,
        Self::Service,
        Self::Perps,
        Self::Governance,
        Self::Bridge,
        Self::Programs,
    ];

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

/// The kernel spot module identifier, outside the negotiated agent module set.
pub const SPOT_MODULE_ID: u16 = 10;

const PERPS_MAX_ORACLE_KEYS: usize = 8;
const PERPS_ADL_CAPACITY: usize = 128;
const PERPS_MARKET_BYTES: usize = 622;
const BASIS_POINTS_ONE: u32 = 10_000;

/// Failure to encode or decode a canonical perps or spot activity payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TradingPayloadError {
    /// The activity type is not a declared perps or spot activity.
    UnknownActivity(u32),
    /// The payload length does not match the activity layout.
    Length(usize),
    /// A field is zero, out of its enumeration, or not canonically encoded.
    NonCanonical,
    /// A market parameter is outside its protocol bounds.
    ParameterBounds,
    /// An identifier list is not strictly increasing.
    UnsortedSequence,
    /// The negotiated registry refused the encoded activity.
    Payload(PayloadError),
}

/// The side of a perps or spot order or position.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum TradeSide {
    /// Perps buy, spot bid.
    Buy = 1,
    /// Perps sell, spot ask.
    Sell = 2,
}

impl TradeSide {
    const fn from_byte(value: u8) -> Result<Self, TradingPayloadError> {
        match value {
            1 => Ok(Self::Buy),
            2 => Ok(Self::Sell),
            _ => Err(TradingPayloadError::NonCanonical),
        }
    }
}

/// A spot order's execution kind.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum SpotOrderKind {
    /// A priced order.
    Limit = 1,
    /// An unpriced immediate order.
    Market = 2,
}

/// A spot order's time in force.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum SpotTimeInForce {
    /// Rests on the book until filled or cancelled.
    GoodTilCancelled = 1,
    /// Fills what it can immediately and cancels the rest.
    ImmediateOrCancel = 2,
}

/// The full parameter set of a perps market creation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PerpsMarket {
    pub market_id: [u8; 32],
    pub quote_asset: [u8; 32],
    pub administrator: [u8; 32],
    pub liquidity_account_id: [u8; 32],
    pub long_funding_account_id: [u8; 32],
    pub short_funding_account_id: [u8; 32],
    pub insurance_account_id: [u8; 32],
    pub contract_size: u128,
    pub tick_size: u128,
    pub lot_size: u128,
    pub price_scale: u128,
    pub initial_margin_ratio_bps: u32,
    pub maintenance_margin_ratio_bps: u32,
    pub liquidation_fee_bps: u32,
    pub liquidator_share_bps: u32,
    pub maximum_funding_rate_bps: u32,
    pub maximum_deviation_basis_points: u32,
    pub funding_interval_ms: u64,
    pub maximum_oracle_staleness_ms: u64,
    pub minimum_price: u128,
    pub maximum_price: u128,
    pub permitted_oracle_keys: Vec<[u8; 32]>,
    pub parameter_version: u32,
    pub halted: bool,
}

impl PerpsMarket {
    fn validate(&self) -> Result<(), TradingPayloadError> {
        let accounts = [
            self.liquidity_account_id,
            self.long_funding_account_id,
            self.short_funding_account_id,
            self.insurance_account_id,
        ];
        let accounts_canonical = !zero(&self.administrator)
            && accounts
                .iter()
                .enumerate()
                .all(|(index, account)| !zero(account) && !accounts[..index].contains(account));
        let keys = &self.permitted_oracle_keys;
        let keys_canonical = !keys.is_empty()
            && keys.len() <= PERPS_MAX_ORACLE_KEYS
            && keys.iter().all(|key| !zero(key))
            && keys.windows(2).all(|pair| pair[0] < pair[1]);
        let bounded = !zero(&self.market_id)
            && !zero(&self.quote_asset)
            && self.contract_size != 0
            && self.tick_size != 0
            && self.lot_size != 0
            && self.price_scale != 0
            && self.maintenance_margin_ratio_bps != 0
            && self.initial_margin_ratio_bps > self.maintenance_margin_ratio_bps
            && self.initial_margin_ratio_bps <= BASIS_POINTS_ONE
            && self.liquidation_fee_bps <= BASIS_POINTS_ONE
            && self.liquidator_share_bps <= BASIS_POINTS_ONE
            && self.maximum_funding_rate_bps != 0
            && self.maximum_funding_rate_bps <= BASIS_POINTS_ONE
            && self.maximum_deviation_basis_points != 0
            && self.maximum_deviation_basis_points <= BASIS_POINTS_ONE
            && self.funding_interval_ms != 0
            && self.maximum_oracle_staleness_ms != 0
            && self.minimum_price != 0
            && self.minimum_price < self.maximum_price
            && self.parameter_version != 0;
        if bounded && accounts_canonical && keys_canonical {
            Ok(())
        } else {
            Err(TradingPayloadError::ParameterBounds)
        }
    }

    fn encode(&self, out: &mut Vec<u8>) -> Result<(), TradingPayloadError> {
        self.validate()?;
        for id in [
            &self.market_id,
            &self.quote_asset,
            &self.administrator,
            &self.liquidity_account_id,
            &self.long_funding_account_id,
            &self.short_funding_account_id,
            &self.insurance_account_id,
        ] {
            out.extend_from_slice(id);
        }
        for value in [
            self.contract_size,
            self.tick_size,
            self.lot_size,
            self.price_scale,
        ] {
            out.extend_from_slice(&value.to_be_bytes());
        }
        for value in [
            self.initial_margin_ratio_bps,
            self.maintenance_margin_ratio_bps,
            self.liquidation_fee_bps,
            self.liquidator_share_bps,
            self.maximum_funding_rate_bps,
            self.maximum_deviation_basis_points,
        ] {
            out.extend_from_slice(&value.to_be_bytes());
        }
        out.extend_from_slice(&self.funding_interval_ms.to_be_bytes());
        out.extend_from_slice(&self.maximum_oracle_staleness_ms.to_be_bytes());
        out.extend_from_slice(&self.minimum_price.to_be_bytes());
        out.extend_from_slice(&self.maximum_price.to_be_bytes());
        let count = u8::try_from(self.permitted_oracle_keys.len())
            .map_err(|_| TradingPayloadError::ParameterBounds)?;
        out.push(count);
        for key in &self.permitted_oracle_keys {
            out.extend_from_slice(key);
        }
        out.resize(
            out.len() + (PERPS_MAX_ORACLE_KEYS - self.permitted_oracle_keys.len()) * 32,
            0,
        );
        out.extend_from_slice(&self.parameter_version.to_be_bytes());
        out.push(u8::from(self.halted));
        Ok(())
    }

    fn decode(reader: &mut Reader<'_>) -> Result<Self, TradingPayloadError> {
        let market_id = reader.id()?;
        let quote_asset = reader.id()?;
        let administrator = reader.id()?;
        let liquidity_account_id = reader.id()?;
        let long_funding_account_id = reader.id()?;
        let short_funding_account_id = reader.id()?;
        let insurance_account_id = reader.id()?;
        let contract_size = reader.u128()?;
        let tick_size = reader.u128()?;
        let lot_size = reader.u128()?;
        let price_scale = reader.u128()?;
        let initial_margin_ratio_bps = reader.u32()?;
        let maintenance_margin_ratio_bps = reader.u32()?;
        let liquidation_fee_bps = reader.u32()?;
        let liquidator_share_bps = reader.u32()?;
        let maximum_funding_rate_bps = reader.u32()?;
        let maximum_deviation_basis_points = reader.u32()?;
        let funding_interval_ms = reader.u64()?;
        let maximum_oracle_staleness_ms = reader.u64()?;
        let minimum_price = reader.u128()?;
        let maximum_price = reader.u128()?;
        let count = usize::from(reader.u8()?);
        if count > PERPS_MAX_ORACLE_KEYS {
            return Err(TradingPayloadError::NonCanonical);
        }
        let mut permitted_oracle_keys = Vec::with_capacity(count);
        for index in 0..PERPS_MAX_ORACLE_KEYS {
            let key = reader.id()?;
            if index < count {
                permitted_oracle_keys.push(key);
            } else if !zero(&key) {
                return Err(TradingPayloadError::NonCanonical);
            }
        }
        let parameter_version = reader.u32()?;
        let halted = reader.flag()?;
        let market = Self {
            market_id,
            quote_asset,
            administrator,
            liquidity_account_id,
            long_funding_account_id,
            short_funding_account_id,
            insurance_account_id,
            contract_size,
            tick_size,
            lot_size,
            price_scale,
            initial_margin_ratio_bps,
            maintenance_margin_ratio_bps,
            liquidation_fee_bps,
            liquidator_share_bps,
            maximum_funding_rate_bps,
            maximum_deviation_basis_points,
            funding_interval_ms,
            maximum_oracle_staleness_ms,
            minimum_price,
            maximum_price,
            permitted_oracle_keys,
            parameter_version,
            halted,
        };
        market.validate()?;
        Ok(market)
    }
}

/// A typed perps module activity with its exact kernel payload layout.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PerpsPayload {
    /// `0x00060001`: create a market.
    MarketCreate(PerpsMarket),
    /// `0x00060002`: halt or resume a market.
    MarketHalt { market_id: [u8; 32], halted: bool },
    /// `0x00060003`: push an oracle observation.
    OraclePush {
        market_id: [u8; 32],
        observation_sequence: u64,
        price: u128,
        observed_at: u64,
        source_identifier: u64,
    },
    /// `0x00060004`: place an order.
    OrderPlace {
        market_id: [u8; 32],
        order_id: [u8; 32],
        owner_account_id: [u8; 32],
        side: TradeSide,
        price: u128,
        quantity: u128,
    },
    /// `0x00060005`: cancel an order.
    OrderCancel {
        market_id: [u8; 32],
        order_id: [u8; 32],
    },
    /// `0x00060006`: open a position.
    PositionOpen {
        market_id: [u8; 32],
        position_id: [u8; 32],
        margin_account_id: [u8; 32],
        side: TradeSide,
        size: u128,
        entry_notional: u128,
        margin_amount: u128,
    },
    /// `0x00060007`: increase a position.
    PositionIncrease {
        market_id: [u8; 32],
        position_id: [u8; 32],
        size_delta: u128,
        notional_delta: u128,
        margin_amount: u128,
    },
    /// `0x00060008`: close a position.
    PositionClose {
        market_id: [u8; 32],
        position_id: [u8; 32],
    },
    /// `0x00060009`: settle a funding interval.
    FundingTick { market_id: [u8; 32] },
    /// `0x0006000a`: liquidate a position.
    Liquidate {
        market_id: [u8; 32],
        position_id: [u8; 32],
        liquidator_account_id: [u8; 32],
    },
    /// `0x0006000b`: auto-deleverage sorted positions.
    Adl {
        market_id: [u8; 32],
        position_ids: Vec<[u8; 32]>,
    },
}

impl PerpsPayload {
    /// Returns the activity ordinal within the perps module.
    #[must_use]
    pub const fn ordinal(&self) -> u16 {
        match self {
            Self::MarketCreate(_) => 1,
            Self::MarketHalt { .. } => 2,
            Self::OraclePush { .. } => 3,
            Self::OrderPlace { .. } => 4,
            Self::OrderCancel { .. } => 5,
            Self::PositionOpen { .. } => 6,
            Self::PositionIncrease { .. } => 7,
            Self::PositionClose { .. } => 8,
            Self::FundingTick { .. } => 9,
            Self::Liquidate { .. } => 10,
            Self::Adl { .. } => 11,
        }
    }

    /// Returns the packed perps activity type.
    #[must_use]
    pub const fn activity_type(&self) -> ActivityType {
        ActivityType(((ModuleId::Perps as u32) << 16) | self.ordinal() as u32)
    }

    /// Encodes the exact kernel payload bytes.
    ///
    /// # Errors
    ///
    /// Returns the kernel's refusal for a zero, unbounded or unsorted field.
    pub fn encode(&self) -> Result<Vec<u8>, TradingPayloadError> {
        let mut out = Vec::new();
        match self {
            Self::MarketCreate(market) => market.encode(&mut out)?,
            Self::MarketHalt { market_id, halted } => {
                ids(&mut out, &[market_id])?;
                out.push(u8::from(*halted));
            }
            Self::OraclePush {
                market_id,
                observation_sequence,
                price,
                observed_at,
                source_identifier,
            } => {
                if *observation_sequence == 0
                    || *price == 0
                    || *observed_at == 0
                    || *source_identifier == 0
                {
                    return Err(TradingPayloadError::NonCanonical);
                }
                ids(&mut out, &[market_id])?;
                out.extend_from_slice(&observation_sequence.to_be_bytes());
                out.extend_from_slice(&price.to_be_bytes());
                out.extend_from_slice(&observed_at.to_be_bytes());
                out.extend_from_slice(&source_identifier.to_be_bytes());
            }
            Self::OrderPlace {
                market_id,
                order_id,
                owner_account_id,
                side,
                price,
                quantity,
            } => {
                nonzero(&[*price, *quantity])?;
                ids(&mut out, &[market_id, order_id, owner_account_id])?;
                out.push(*side as u8);
                out.extend_from_slice(&price.to_be_bytes());
                out.extend_from_slice(&quantity.to_be_bytes());
            }
            Self::OrderCancel {
                market_id,
                order_id,
            } => ids(&mut out, &[market_id, order_id])?,
            Self::PositionOpen {
                market_id,
                position_id,
                margin_account_id,
                side,
                size,
                entry_notional,
                margin_amount,
            } => {
                nonzero(&[*size, *margin_amount])?;
                if *entry_notional != 0 {
                    return Err(TradingPayloadError::NonCanonical);
                }
                ids(&mut out, &[market_id, position_id, margin_account_id])?;
                out.push(*side as u8);
                for value in [size, entry_notional, margin_amount] {
                    out.extend_from_slice(&value.to_be_bytes());
                }
            }
            Self::PositionIncrease {
                market_id,
                position_id,
                size_delta,
                notional_delta,
                margin_amount,
            } => {
                nonzero(&[*size_delta, *margin_amount])?;
                if *notional_delta != 0 {
                    return Err(TradingPayloadError::NonCanonical);
                }
                ids(&mut out, &[market_id, position_id])?;
                for value in [size_delta, notional_delta, margin_amount] {
                    out.extend_from_slice(&value.to_be_bytes());
                }
            }
            Self::PositionClose {
                market_id,
                position_id,
            } => ids(&mut out, &[market_id, position_id])?,
            Self::FundingTick { market_id } => ids(&mut out, &[market_id])?,
            Self::Liquidate {
                market_id,
                position_id,
                liquidator_account_id,
            } => ids(&mut out, &[market_id, position_id, liquidator_account_id])?,
            Self::Adl {
                market_id,
                position_ids,
            } => adl_encode(&mut out, market_id, position_ids)?,
        }
        Ok(out)
    }

    /// Decodes and validates exact kernel payload bytes for a perps activity.
    ///
    /// # Errors
    ///
    /// Returns a typed refusal for an undeclared activity, a wrong length, or
    /// a payload the kernel decoder would reject.
    pub fn decode(activity_type: ActivityType, bytes: &[u8]) -> Result<Self, TradingPayloadError> {
        if activity_type.module() != ModuleId::Perps {
            return Err(TradingPayloadError::UnknownActivity(activity_type.value()));
        }
        let expected = match activity_type.ordinal() {
            1 => PERPS_MARKET_BYTES,
            2 => 33,
            3 => 72,
            4 => 129,
            5 | 8 => 64,
            6 => 145,
            7 => 112,
            9 => 32,
            10 => 96,
            11 => return adl_decode(bytes),
            _ => return Err(TradingPayloadError::UnknownActivity(activity_type.value())),
        };
        if bytes.len() != expected {
            return Err(TradingPayloadError::Length(bytes.len()));
        }
        let mut reader = Reader(bytes);
        let decoded = match activity_type.ordinal() {
            1 => Self::MarketCreate(PerpsMarket::decode(&mut reader)?),
            2 => {
                let market_id = reader.nonzero_id()?;
                Self::MarketHalt {
                    market_id,
                    halted: reader.flag()?,
                }
            }
            3 => Self::OraclePush {
                market_id: reader.id()?,
                observation_sequence: reader.u64()?,
                price: reader.u128()?,
                observed_at: reader.u64()?,
                source_identifier: reader.u64()?,
            },
            4 => Self::OrderPlace {
                market_id: reader.id()?,
                order_id: reader.id()?,
                owner_account_id: reader.id()?,
                side: TradeSide::from_byte(reader.u8()?)?,
                price: reader.u128()?,
                quantity: reader.u128()?,
            },
            5 => Self::OrderCancel {
                market_id: reader.id()?,
                order_id: reader.id()?,
            },
            6 => Self::PositionOpen {
                market_id: reader.id()?,
                position_id: reader.id()?,
                margin_account_id: reader.id()?,
                side: TradeSide::from_byte(reader.u8()?)?,
                size: reader.u128()?,
                entry_notional: reader.u128()?,
                margin_amount: reader.u128()?,
            },
            7 => Self::PositionIncrease {
                market_id: reader.id()?,
                position_id: reader.id()?,
                size_delta: reader.u128()?,
                notional_delta: reader.u128()?,
                margin_amount: reader.u128()?,
            },
            8 => Self::PositionClose {
                market_id: reader.id()?,
                position_id: reader.id()?,
            },
            9 => Self::FundingTick {
                market_id: reader.id()?,
            },
            _ => Self::Liquidate {
                market_id: reader.id()?,
                position_id: reader.id()?,
                liquidator_account_id: reader.id()?,
            },
        };
        if decoded.encode()? != bytes {
            return Err(TradingPayloadError::NonCanonical);
        }
        Ok(decoded)
    }

    /// Wraps the encoded activity as a payload declared by `registry`.
    ///
    /// # Errors
    ///
    /// Returns an encoding refusal or the registry's payload refusal.
    pub fn payload(&self, registry: &ModuleRegistry) -> Result<Payload, TradingPayloadError> {
        Payload::new(registry, self.activity_type(), &self.encode()?)
            .map_err(TradingPayloadError::Payload)
    }

    /// Decodes a perps module payload back into its typed activity.
    ///
    /// # Errors
    ///
    /// Returns a typed refusal for a non-perps or non-canonical payload.
    pub fn from_payload(payload: &Payload) -> Result<Self, TradingPayloadError> {
        Self::decode(payload.activity_type(), payload.as_bytes())
    }
}

/// A typed spot module activity with its exact kernel payload layout.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SpotPayload {
    /// `0x000a0001`: create a market.
    MarketCreate {
        market_id: [u8; 32],
        base_asset: [u8; 32],
        quote_asset: [u8; 32],
        tick_size: u128,
        lot_size: u128,
        administrator: [u8; 32],
    },
    /// `0x000a0002`: place an order.
    OrderPlace {
        market_id: [u8; 32],
        order_id: [u8; 32],
        base_account_id: [u8; 32],
        quote_account_id: [u8; 32],
        side: TradeSide,
        kind: SpotOrderKind,
        time_in_force: SpotTimeInForce,
        price: u128,
        quantity: u128,
    },
    /// `0x000a0003`: cancel an order.
    OrderCancel {
        market_id: [u8; 32],
        order_id: [u8; 32],
    },
    /// `0x000a0004`: halt a market.
    MarketHalt { market_id: [u8; 32] },
    /// `0x000a0005`: resume a market.
    MarketResume { market_id: [u8; 32] },
}

impl SpotPayload {
    /// Returns the activity ordinal within the spot module.
    #[must_use]
    pub const fn ordinal(&self) -> u16 {
        match self {
            Self::MarketCreate { .. } => 1,
            Self::OrderPlace { .. } => 2,
            Self::OrderCancel { .. } => 3,
            Self::MarketHalt { .. } => 4,
            Self::MarketResume { .. } => 5,
        }
    }

    /// Returns the packed kernel activity type `0x000axxxx`.
    #[must_use]
    pub const fn activity_value(&self) -> u32 {
        ((SPOT_MODULE_ID as u32) << 16) | self.ordinal() as u32
    }

    /// Encodes the exact kernel payload bytes.
    ///
    /// # Errors
    ///
    /// Returns the kernel's refusal for a zero, equal or inconsistent field.
    pub fn encode(&self) -> Result<Vec<u8>, TradingPayloadError> {
        let mut out = Vec::new();
        match self {
            Self::MarketCreate {
                market_id,
                base_asset,
                quote_asset,
                tick_size,
                lot_size,
                administrator,
            } => {
                nonzero(&[*tick_size, *lot_size])?;
                if base_asset == quote_asset || zero(administrator) {
                    return Err(TradingPayloadError::NonCanonical);
                }
                ids(&mut out, &[market_id, base_asset, quote_asset])?;
                out.extend_from_slice(&tick_size.to_be_bytes());
                out.extend_from_slice(&lot_size.to_be_bytes());
                out.extend_from_slice(administrator);
            }
            Self::OrderPlace {
                market_id,
                order_id,
                base_account_id,
                quote_account_id,
                side,
                kind,
                time_in_force,
                price,
                quantity,
            } => {
                let priced = match kind {
                    SpotOrderKind::Limit => *price != 0,
                    SpotOrderKind::Market => {
                        *price == 0 && *time_in_force == SpotTimeInForce::ImmediateOrCancel
                    }
                };
                if !priced || *quantity == 0 || base_account_id == quote_account_id {
                    return Err(TradingPayloadError::NonCanonical);
                }
                ids(
                    &mut out,
                    &[market_id, order_id, base_account_id, quote_account_id],
                )?;
                out.extend_from_slice(&[*side as u8, *kind as u8, *time_in_force as u8]);
                out.extend_from_slice(&price.to_be_bytes());
                out.extend_from_slice(&quantity.to_be_bytes());
            }
            Self::OrderCancel {
                market_id,
                order_id,
            } => ids(&mut out, &[market_id, order_id])?,
            Self::MarketHalt { market_id } | Self::MarketResume { market_id } => {
                ids(&mut out, &[market_id])?;
            }
        }
        Ok(out)
    }

    /// Decodes and validates exact kernel payload bytes for a spot activity.
    ///
    /// # Errors
    ///
    /// Returns a typed refusal for an undeclared activity, a wrong length, or
    /// a payload the kernel decoder would reject.
    pub fn decode(activity_value: u32, bytes: &[u8]) -> Result<Self, TradingPayloadError> {
        let module = activity_value >> 16;
        let expected = match (module, activity_value & 0xffff) {
            (10, 1) => 160,
            (10, 2) => 163,
            (10, 3) => 64,
            (10, 4 | 5) => 32,
            _ => return Err(TradingPayloadError::UnknownActivity(activity_value)),
        };
        if bytes.len() != expected {
            return Err(TradingPayloadError::Length(bytes.len()));
        }
        let mut reader = Reader(bytes);
        let decoded = match activity_value & 0xffff {
            1 => Self::MarketCreate {
                market_id: reader.id()?,
                base_asset: reader.id()?,
                quote_asset: reader.id()?,
                tick_size: reader.u128()?,
                lot_size: reader.u128()?,
                administrator: reader.id()?,
            },
            2 => Self::OrderPlace {
                market_id: reader.id()?,
                order_id: reader.id()?,
                base_account_id: reader.id()?,
                quote_account_id: reader.id()?,
                side: TradeSide::from_byte(reader.u8()?)?,
                kind: match reader.u8()? {
                    1 => SpotOrderKind::Limit,
                    2 => SpotOrderKind::Market,
                    _ => return Err(TradingPayloadError::NonCanonical),
                },
                time_in_force: match reader.u8()? {
                    1 => SpotTimeInForce::GoodTilCancelled,
                    2 => SpotTimeInForce::ImmediateOrCancel,
                    _ => return Err(TradingPayloadError::NonCanonical),
                },
                price: reader.u128()?,
                quantity: reader.u128()?,
            },
            3 => Self::OrderCancel {
                market_id: reader.id()?,
                order_id: reader.id()?,
            },
            4 => Self::MarketHalt {
                market_id: reader.id()?,
            },
            _ => Self::MarketResume {
                market_id: reader.id()?,
            },
        };
        if decoded.encode()? != bytes {
            return Err(TradingPayloadError::NonCanonical);
        }
        Ok(decoded)
    }
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], TradingPayloadError> {
        let (head, rest) = self
            .0
            .split_first_chunk::<N>()
            .ok_or(TradingPayloadError::Length(self.0.len()))?;
        self.0 = rest;
        Ok(*head)
    }

    fn id(&mut self) -> Result<[u8; 32], TradingPayloadError> {
        self.take::<32>()
    }

    fn nonzero_id(&mut self) -> Result<[u8; 32], TradingPayloadError> {
        let id = self.id()?;
        if zero(&id) {
            return Err(TradingPayloadError::NonCanonical);
        }
        Ok(id)
    }

    fn u8(&mut self) -> Result<u8, TradingPayloadError> {
        Ok(self.take::<1>()?[0])
    }

    fn flag(&mut self) -> Result<bool, TradingPayloadError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(TradingPayloadError::NonCanonical),
        }
    }

    fn u32(&mut self) -> Result<u32, TradingPayloadError> {
        Ok(u32::from_be_bytes(self.take::<4>()?))
    }

    fn u64(&mut self) -> Result<u64, TradingPayloadError> {
        Ok(u64::from_be_bytes(self.take::<8>()?))
    }

    fn u128(&mut self) -> Result<u128, TradingPayloadError> {
        Ok(u128::from_be_bytes(self.take::<16>()?))
    }
}

fn zero(id: &[u8; 32]) -> bool {
    id.iter().all(|byte| *byte == 0)
}

fn ids(out: &mut Vec<u8>, values: &[&[u8; 32]]) -> Result<(), TradingPayloadError> {
    if values.iter().any(|id| zero(id)) {
        return Err(TradingPayloadError::NonCanonical);
    }
    for id in values {
        out.extend_from_slice(*id);
    }
    Ok(())
}

fn nonzero(values: &[u128]) -> Result<(), TradingPayloadError> {
    if values.contains(&0) {
        Err(TradingPayloadError::NonCanonical)
    } else {
        Ok(())
    }
}

fn adl_encode(
    out: &mut Vec<u8>,
    market_id: &[u8; 32],
    position_ids: &[[u8; 32]],
) -> Result<(), TradingPayloadError> {
    if position_ids.is_empty() || position_ids.len() > PERPS_ADL_CAPACITY {
        return Err(TradingPayloadError::NonCanonical);
    }
    if position_ids.iter().any(zero) {
        return Err(TradingPayloadError::NonCanonical);
    }
    if position_ids.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(TradingPayloadError::UnsortedSequence);
    }
    ids(out, &[market_id])?;
    out.push(u8::try_from(position_ids.len()).map_err(|_| TradingPayloadError::NonCanonical)?);
    for id in position_ids {
        out.extend_from_slice(id);
    }
    Ok(())
}

fn adl_decode(bytes: &[u8]) -> Result<PerpsPayload, TradingPayloadError> {
    if bytes.len() < 65 || bytes.len() > 33 + 32 * PERPS_ADL_CAPACITY {
        return Err(TradingPayloadError::Length(bytes.len()));
    }
    let mut reader = Reader(bytes);
    let market_id = reader.id()?;
    let count = usize::from(reader.u8()?);
    if count == 0 || count > PERPS_ADL_CAPACITY || bytes.len() != 33 + 32 * count {
        return Err(TradingPayloadError::NonCanonical);
    }
    if zero(&market_id) {
        return Err(TradingPayloadError::NonCanonical);
    }
    let mut position_ids = Vec::with_capacity(count);
    for _ in 0..count {
        position_ids.push(reader.id()?);
    }
    let decoded = PerpsPayload::Adl {
        market_id,
        position_ids,
    };
    decoded.encode()?;
    Ok(decoded)
}
