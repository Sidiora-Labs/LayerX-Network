//! Atomic multi-scope budget reservations.

use std::collections::BTreeMap;
use std::sync::Mutex;

use sha2::{Digest as _, Sha256};

const RESERVATION_DIGEST_DOMAIN: &[u8] = b"layerx:budget-reservation:v1\0";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct LimitId(pub [u8; 16]);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LimitScope {
    Tenant([u8; 32]),
    Agent([u8; 32]),
    Session([u8; 32]),
    Capability([u8; 32]),
    Counterparty([u8; 32]),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LimitConfig {
    pub id: LimitId,
    pub name: String,
    pub scope: LimitScope,
    pub ceiling: u128,
    pub consumed: u128,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LimitState {
    config: LimitConfig,
    held: BTreeMap<[u8; 32], (u128, u64)>,
}

#[derive(Debug)]
pub struct BudgetLimiter {
    limits: Mutex<BTreeMap<LimitId, LimitState>>,
}

impl BudgetLimiter {
    /// Whether an exact durable approval reservation is present in any configured scope.
    pub fn has_reservation(&self, reservation_id: [u8; 32]) -> Result<bool, LimitRefusal> {
        let limits = self.limits.lock().map_err(|_| LimitRefusal::Poisoned)?;
        Ok(limits
            .values()
            .any(|limit| limit.held.contains_key(&reservation_id)))
    }
    /// Builds a limiter from a complete set of limit configurations.
    ///
    /// # Errors
    ///
    /// Refuses a zero ceiling, an already-consumed amount above its ceiling, and a repeated
    /// limit identifier.
    pub fn new(configs: Vec<LimitConfig>) -> Result<Self, LimitRefusal> {
        let mut limits = BTreeMap::new();
        for config in configs {
            if config.ceiling == 0 || config.consumed > config.ceiling {
                return Err(LimitRefusal::InvalidConfiguration);
            }
            if limits
                .insert(
                    config.id,
                    LimitState {
                        config,
                        held: BTreeMap::new(),
                    },
                )
                .is_some()
            {
                return Err(LimitRefusal::InvalidConfiguration);
            }
        }
        Ok(Self {
            limits: Mutex::new(limits),
        })
    }

    /// Counts the reservations currently held across every limit.
    ///
    /// # Errors
    ///
    /// Returns `Poisoned` when the limit state was left poisoned by a panicking holder.
    pub fn held_reservations(&self) -> Result<usize, LimitRefusal> {
        let limits = self.limits.lock().map_err(|_| LimitRefusal::Poisoned)?;
        Ok(limits.values().map(|limit| limit.held.len()).sum())
    }

    /// Returns the amount already consumed against one limit.
    ///
    /// # Errors
    ///
    /// Returns `UnknownLimit` for an unconfigured identifier, or `Poisoned` when the limit state
    /// was left poisoned by a panicking holder.
    pub fn consumed(&self, id: LimitId) -> Result<u128, LimitRefusal> {
        let limits = self.limits.lock().map_err(|_| LimitRefusal::Poisoned)?;
        limits
            .get(&id)
            .map(|limit| limit.config.consumed)
            .ok_or(LimitRefusal::UnknownLimit(id))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReservationRequest {
    pub id: [u8; 32],
    pub amount: u128,
    pub expiry_sequence: u64,
    pub current_sequence: u64,
    pub applicable_limits: Vec<LimitId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BudgetReservation {
    pub id: [u8; 32],
    pub amount: u128,
    pub applied_limits: Vec<LimitId>,
    pub durable: Vec<DurableBudgetReservation>,
}

/// Canonical restart record for one reservation applied to one verified limit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DurableBudgetReservation {
    pub reservation_id: [u8; 32],
    pub limit_id: LimitId,
    pub scope: LimitScope,
    pub amount: u128,
    pub ceiling: u128,
    pub expiry_sequence: u64,
    pub digest: [u8; 32],
}

impl DurableBudgetReservation {
    #[must_use]
    pub fn canonical_digest(&self) -> [u8; 32] {
        reservation_digest(
            self.reservation_id,
            self.limit_id,
            self.scope,
            self.amount,
            self.ceiling,
            self.expiry_sequence,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReleaseKind {
    Executed,
    Failed,
    Expired,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LimitRefusal {
    Exceeded {
        limit: LimitId,
        name: String,
        ceiling: u128,
        consumed: u128,
        held: u128,
        requested: u128,
    },
    UnknownLimit(LimitId),
    InvalidConfiguration,
    InvalidRequest,
    Arithmetic,
    Poisoned,
}

pub(crate) fn reserve_all(
    limiter: &BudgetLimiter,
    request: &ReservationRequest,
) -> Result<BudgetReservation, LimitRefusal> {
    if request.amount == 0
        || request.expiry_sequence <= request.current_sequence
        || request.applicable_limits.is_empty()
    {
        return Err(LimitRefusal::InvalidRequest);
    }
    let mut applicable = request.applicable_limits.clone();
    applicable.sort_unstable();
    applicable.dedup();
    let mut limits = limiter.limits.lock().map_err(|_| LimitRefusal::Poisoned)?;
    for id in &applicable {
        let limit = limits.get(id).ok_or(LimitRefusal::UnknownLimit(*id))?;
        if limit.held.contains_key(&request.id) {
            return Err(LimitRefusal::InvalidRequest);
        }
        let held = held_total(limit)?;
        let projected = limit
            .config
            .consumed
            .checked_add(held)
            .and_then(|value| value.checked_add(request.amount))
            .ok_or(LimitRefusal::Arithmetic)?;
        if projected > limit.config.ceiling {
            return Err(LimitRefusal::Exceeded {
                limit: *id,
                name: limit.config.name.clone(),
                ceiling: limit.config.ceiling,
                consumed: limit.config.consumed,
                held,
                requested: request.amount,
            });
        }
    }
    for id in &applicable {
        if let Some(limit) = limits.get_mut(id) {
            limit
                .held
                .insert(request.id, (request.amount, request.expiry_sequence));
        }
    }
    let durable = applicable
        .iter()
        .map(|id| {
            let limit = limits.get(id).ok_or(LimitRefusal::UnknownLimit(*id))?;
            let mut record = DurableBudgetReservation {
                reservation_id: request.id,
                limit_id: *id,
                scope: limit.config.scope,
                amount: request.amount,
                ceiling: limit.config.ceiling,
                expiry_sequence: request.expiry_sequence,
                digest: [0; 32],
            };
            record.digest = record.canonical_digest();
            Ok(record)
        })
        .collect::<Result<Vec<_>, LimitRefusal>>()?;
    Ok(BudgetReservation {
        id: request.id,
        amount: request.amount,
        applied_limits: applicable,
        durable,
    })
}

pub(crate) fn restore_all(
    limiter: &BudgetLimiter,
    records: &[DurableBudgetReservation],
) -> Result<(), LimitRefusal> {
    let mut limits = limiter.limits.lock().map_err(|_| LimitRefusal::Poisoned)?;
    let mut restored = limits.clone();
    for record in records {
        if record.reservation_id == [0; 32]
            || record.amount == 0
            || record.expiry_sequence == 0
            || record.digest != record.canonical_digest()
        {
            return Err(LimitRefusal::InvalidRequest);
        }
        let limit = restored
            .get_mut(&record.limit_id)
            .ok_or(LimitRefusal::UnknownLimit(record.limit_id))?;
        if limit.config.scope != record.scope
            || limit.config.ceiling != record.ceiling
            || limit.held.contains_key(&record.reservation_id)
        {
            return Err(LimitRefusal::InvalidConfiguration);
        }
        limit.held.insert(
            record.reservation_id,
            (record.amount, record.expiry_sequence),
        );
    }
    for limit in restored.values() {
        let held = held_total(limit)?;
        if limit
            .config
            .consumed
            .checked_add(held)
            .ok_or(LimitRefusal::Arithmetic)?
            > limit.config.ceiling
        {
            return Err(LimitRefusal::InvalidConfiguration);
        }
    }
    *limits = restored;
    Ok(())
}

pub(crate) fn release_all(
    limiter: &BudgetLimiter,
    reservation_id: [u8; 32],
    kind: ReleaseKind,
    current_sequence: u64,
) -> Result<bool, LimitRefusal> {
    if kind == ReleaseKind::Unknown {
        return Ok(false);
    }
    let mut limits = limiter.limits.lock().map_err(|_| LimitRefusal::Poisoned)?;
    let mut found = false;
    for limit in limits.values_mut() {
        let Some((amount, expiry)) = limit.held.get(&reservation_id).copied() else {
            continue;
        };
        if kind == ReleaseKind::Expired && current_sequence < expiry {
            continue;
        }
        limit.held.remove(&reservation_id);
        if kind == ReleaseKind::Executed {
            limit.config.consumed = limit
                .config
                .consumed
                .checked_add(amount)
                .ok_or(LimitRefusal::Arithmetic)?;
        }
        found = true;
    }
    Ok(found)
}

fn held_total(limit: &LimitState) -> Result<u128, LimitRefusal> {
    limit
        .held
        .values()
        .try_fold(0_u128, |total, (amount, _)| total.checked_add(*amount))
        .ok_or(LimitRefusal::Arithmetic)
}

fn reservation_digest(
    reservation_id: [u8; 32],
    limit_id: LimitId,
    scope: LimitScope,
    amount: u128,
    ceiling: u128,
    expiry_sequence: u64,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(RESERVATION_DIGEST_DOMAIN);
    hasher.update(reservation_id);
    hasher.update(limit_id.0);
    let (tag, identity) = match scope {
        LimitScope::Tenant(value) => (0_u8, value),
        LimitScope::Agent(value) => (1_u8, value),
        LimitScope::Session(value) => (2_u8, value),
        LimitScope::Capability(value) => (3_u8, value),
        LimitScope::Counterparty(value) => (4_u8, value),
    };
    hasher.update([tag]);
    hasher.update(identity);
    hasher.update(amount.to_be_bytes());
    hasher.update(ceiling.to_be_bytes());
    hasher.update(expiry_sequence.to_be_bytes());
    hasher.finalize().into()
}
