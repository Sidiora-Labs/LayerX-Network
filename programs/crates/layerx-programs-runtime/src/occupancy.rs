//! Deterministic storage occupancy accounting across protocol batches.

use core::fmt::{self, Display};
use std::collections::{BTreeMap, BTreeSet};

use crate::budget::AdmittedBudget;
use crate::meter::FeeSchedule;
use crate::storage::{PrincipalId, ProgramId, Storage, StorageError, StorageNamespace};

const EVIDENCE_DOMAIN: &[u8] = b"LXP/storage-occupancy-settlement/v1\0";
const LEDGER_DOMAIN: &[u8] = b"LXP/storage-occupancy-ledger/v1\0";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OccupancyResponsibility {
    namespace: StorageNamespace,
    payer: PrincipalId,
}

impl OccupancyResponsibility {
    pub(crate) fn from_admitted(
        executing_program: ProgramId,
        namespace: StorageNamespace,
        admitted: &AdmittedBudget,
    ) -> Result<Self, OccupancyError> {
        let payer = admitted.payer();
        if namespace.program() != executing_program
            || namespace
                .principal_scope()
                .is_some_and(|principal| principal != payer)
        {
            return Err(OccupancyError::AuthorityMismatch { namespace });
        }
        Ok(Self { namespace, payer })
    }
    #[must_use]
    pub const fn namespace(self) -> StorageNamespace {
        self.namespace
    }
    #[must_use]
    pub const fn payer(self) -> PrincipalId {
        self.payer
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct OccupancyPosition {
    payer: PrincipalId,
    bytes: u64,
    batch: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OccupancyUsage {
    /// Exact metered occupancy usage in namespace byte-batches.
    pub byte_batches: u128,
    /// Exact fee charged for the metered occupancy usage.
    pub fee_units: u128,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OccupancyCharge {
    namespace: StorageNamespace,
    payer: PrincipalId,
    from_batch: u64,
    to_batch: u64,
    recorded_bytes: u64,
    final_bytes: u64,
    byte_batches: u128,
    price: u64,
    fee_units: u128,
}
impl OccupancyCharge {
    #[must_use]
    pub const fn namespace(self) -> StorageNamespace {
        self.namespace
    }
    #[must_use]
    pub const fn payer(self) -> PrincipalId {
        self.payer
    }
    #[must_use]
    pub const fn from_batch(self) -> u64 {
        self.from_batch
    }
    #[must_use]
    pub const fn to_batch(self) -> u64 {
        self.to_batch
    }
    #[must_use]
    pub const fn recorded_bytes(self) -> u64 {
        self.recorded_bytes
    }
    #[must_use]
    pub const fn final_bytes(self) -> u64 {
        self.final_bytes
    }
    #[must_use]
    pub const fn byte_batches(self) -> u128 {
        self.byte_batches
    }
    #[must_use]
    pub const fn price(self) -> u64 {
        self.price
    }
    #[must_use]
    pub const fn fee_units(self) -> u128 {
        self.fee_units
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OccupancySettlement {
    batch: u64,
    usage: OccupancyUsage,
    fee_schedule: FeeSchedule,
    charges: Vec<OccupancyCharge>,
}
impl OccupancySettlement {
    #[must_use]
    pub const fn batch(&self) -> u64 {
        self.batch
    }
    #[must_use]
    pub const fn usage(&self) -> OccupancyUsage {
        self.usage
    }
    #[must_use]
    pub const fn fee_schedule(&self) -> FeeSchedule {
        self.fee_schedule
    }
    #[must_use]
    pub fn charges(&self) -> &[OccupancyCharge] {
        &self.charges
    }

    #[must_use]
    pub fn canonical_evidence(&self) -> Vec<u8> {
        let mut out = EVIDENCE_DOMAIN.to_vec();
        out.extend_from_slice(&self.batch.to_be_bytes());
        encode_schedule(&mut out, self.fee_schedule);
        out.extend_from_slice(&self.usage.byte_batches.to_be_bytes());
        out.extend_from_slice(&self.usage.fee_units.to_be_bytes());
        out.extend_from_slice(&(self.charges.len() as u64).to_be_bytes());
        for charge in &self.charges {
            encode_namespace(&mut out, charge.namespace);
            out.extend_from_slice(&charge.payer.bytes());
            out.extend_from_slice(&charge.from_batch.to_be_bytes());
            out.extend_from_slice(&charge.to_batch.to_be_bytes());
            out.extend_from_slice(&charge.recorded_bytes.to_be_bytes());
            out.extend_from_slice(&charge.final_bytes.to_be_bytes());
            out.extend_from_slice(&charge.byte_batches.to_be_bytes());
            out.extend_from_slice(&charge.price.to_be_bytes());
            out.extend_from_slice(&charge.fee_units.to_be_bytes());
        }
        out
    }

    pub fn canonical_decode(encoded: &[u8]) -> Result<Self, OccupancyError> {
        let mut cursor = Cursor::new(encoded);
        if cursor.take(EVIDENCE_DOMAIN.len())? != EVIDENCE_DOMAIN {
            return Err(OccupancyError::MalformedEvidence);
        }
        let batch = cursor.u64()?;
        let fee_schedule = decode_schedule(&mut cursor)?;
        let declared_units = cursor.u128()?;
        let declared_fee = cursor.u128()?;
        let count =
            usize::try_from(cursor.u64()?).map_err(|_| OccupancyError::MalformedEvidence)?;
        const MIN_CHARGE_BYTES: usize = 1 + 33 + 32 + 8 + 8 + 8 + 8 + 16 + 8 + 16;
        if count > cursor.remaining.len() / MIN_CHARGE_BYTES {
            return Err(OccupancyError::MalformedEvidence);
        }
        let mut charges = Vec::with_capacity(count);
        let mut prior = None;
        let mut total_units = 0u128;
        let mut total_fee = 0u128;
        for _ in 0..count {
            let namespace = decode_namespace(&mut cursor)?;
            if prior.is_some_and(|value| value >= namespace) {
                return Err(OccupancyError::MalformedEvidence);
            }
            prior = Some(namespace);
            let payer =
                PrincipalId::new(cursor.array()?).map_err(|_| OccupancyError::MalformedEvidence)?;
            if namespace
                .principal_scope()
                .is_some_and(|principal| principal != payer)
            {
                return Err(OccupancyError::MalformedEvidence);
            }
            let from_batch = cursor.u64()?;
            let to_batch = cursor.u64()?;
            let recorded_bytes = cursor.u64()?;
            let final_bytes = cursor.u64()?;
            let byte_batches = cursor.u128()?;
            let price = cursor.u64()?;
            let fee_units = cursor.u128()?;
            let intervals = to_batch
                .checked_sub(from_batch)
                .ok_or(OccupancyError::MalformedEvidence)?;
            let computed_units = u128::from(recorded_bytes)
                .checked_mul(u128::from(intervals))
                .ok_or(OccupancyError::ArithmeticOverflow)?;
            let expected_price = fee_schedule.occupancy_byte_batch_price();
            let computed_fee = computed_units
                .checked_mul(u128::from(expected_price))
                .ok_or(OccupancyError::ArithmeticOverflow)?;
            if to_batch != batch
                || byte_batches != computed_units
                || price != expected_price
                || fee_units != computed_fee
            {
                return Err(OccupancyError::MalformedEvidence);
            }
            total_units = total_units
                .checked_add(byte_batches)
                .ok_or(OccupancyError::ArithmeticOverflow)?;
            total_fee = total_fee
                .checked_add(fee_units)
                .ok_or(OccupancyError::ArithmeticOverflow)?;
            charges.push(OccupancyCharge {
                namespace,
                payer,
                from_batch,
                to_batch,
                recorded_bytes,
                final_bytes,
                byte_batches,
                price,
                fee_units,
            });
        }
        if !cursor.is_empty() || total_units != declared_units || total_fee != declared_fee {
            return Err(OccupancyError::MalformedEvidence);
        }
        Ok(Self {
            batch,
            usage: OccupancyUsage {
                byte_batches: total_units,
                fee_units: total_fee,
            },
            fee_schedule,
            charges,
        })
    }
}

#[derive(Clone, Debug)]
pub struct PreparedOccupancySettlement {
    settlement: OccupancySettlement,
    prior_state: Vec<u8>,
    final_storage_sizes: Vec<u8>,
    next_positions: BTreeMap<StorageNamespace, OccupancyPosition>,
}
impl PreparedOccupancySettlement {
    #[must_use]
    pub const fn settlement(&self) -> &OccupancySettlement {
        &self.settlement
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OccupancyError {
    AuthorityMismatch { namespace: StorageNamespace },
    ResponsibilityMismatch { namespace: StorageNamespace },
    DuplicateResponsibility { namespace: StorageNamespace },
    MissingResponsibility { namespace: StorageNamespace },
    BatchRegression { previous: u64, attempted: u64 },
    StalePreparation,
    ArithmeticOverflow,
    MalformedEvidence,
    Storage(StorageError),
}
impl Display for OccupancyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AuthorityMismatch { .. } => {
                f.write_str("occupancy responsibility does not match authenticated authority")
            }
            Self::ResponsibilityMismatch { .. } => {
                f.write_str("namespace occupancy payer cannot be rebound")
            }
            Self::DuplicateResponsibility { .. } => {
                f.write_str("namespace occupancy responsibility is duplicated")
            }
            Self::MissingResponsibility { .. } => {
                f.write_str("occupied namespace has no responsible account")
            }
            Self::BatchRegression {
                previous,
                attempted,
            } => write!(
                f,
                "occupancy batch {attempted} precedes recorded batch {previous}"
            ),
            Self::StalePreparation => {
                f.write_str("occupancy preparation does not match current ledger state")
            }
            Self::ArithmeticOverflow => f.write_str("storage occupancy arithmetic overflowed"),
            Self::MalformedEvidence => f.write_str("storage occupancy evidence is malformed"),
            Self::Storage(error) => write!(f, "storage occupancy refusal: {error}"),
        }
    }
}
impl std::error::Error for OccupancyError {}
impl From<StorageError> for OccupancyError {
    fn from(value: StorageError) -> Self {
        Self::Storage(value)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OccupancyLedger {
    positions: BTreeMap<StorageNamespace, OccupancyPosition>,
}
impl OccupancyLedger {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            positions: BTreeMap::new(),
        }
    }

    pub fn prepare_batch(
        &self,
        batch: u64,
        storage: &Storage,
        responsibilities: impl IntoIterator<Item = OccupancyResponsibility>,
        schedule: FeeSchedule,
    ) -> Result<PreparedOccupancySettlement, OccupancyError> {
        let mut next = self.positions.clone();
        let mut declared = BTreeSet::new();
        for responsibility in responsibilities {
            if !declared.insert(responsibility.namespace) {
                return Err(OccupancyError::DuplicateResponsibility {
                    namespace: responsibility.namespace,
                });
            }
            match next.get(&responsibility.namespace) {
                Some(position) if position.payer != responsibility.payer => {
                    return Err(OccupancyError::ResponsibilityMismatch {
                        namespace: responsibility.namespace,
                    })
                }
                Some(_) => {}
                None => {
                    next.insert(
                        responsibility.namespace,
                        OccupancyPosition {
                            payer: responsibility.payer,
                            bytes: 0,
                            batch,
                        },
                    );
                }
            }
        }
        let sizes: BTreeMap<_, _> = storage.namespace_sizes()?.into_iter().collect();
        for namespace in sizes.keys() {
            if !next.contains_key(namespace) {
                return Err(OccupancyError::MissingResponsibility {
                    namespace: *namespace,
                });
            }
        }
        let price = schedule.occupancy_byte_batch_price();
        let mut total_units = 0u128;
        let mut total_fee = 0u128;
        let mut charges = Vec::with_capacity(next.len());
        let mut tombstones = Vec::new();
        for (namespace, position) in &mut next {
            let intervals =
                batch
                    .checked_sub(position.batch)
                    .ok_or(OccupancyError::BatchRegression {
                        previous: position.batch,
                        attempted: batch,
                    })?;
            let units = u128::from(position.bytes)
                .checked_mul(u128::from(intervals))
                .ok_or(OccupancyError::ArithmeticOverflow)?;
            let fee = units
                .checked_mul(u128::from(price))
                .ok_or(OccupancyError::ArithmeticOverflow)?;
            total_units = total_units
                .checked_add(units)
                .ok_or(OccupancyError::ArithmeticOverflow)?;
            total_fee = total_fee
                .checked_add(fee)
                .ok_or(OccupancyError::ArithmeticOverflow)?;
            let final_bytes = sizes.get(namespace).copied().unwrap_or(0);
            charges.push(OccupancyCharge {
                namespace: *namespace,
                payer: position.payer,
                from_batch: position.batch,
                to_batch: batch,
                recorded_bytes: position.bytes,
                final_bytes,
                byte_batches: units,
                price,
                fee_units: fee,
            });
            position.bytes = final_bytes;
            position.batch = batch;
            if final_bytes == 0 {
                tombstones.push(*namespace);
            }
        }
        for namespace in tombstones {
            next.remove(&namespace);
        }
        Ok(PreparedOccupancySettlement {
            settlement: OccupancySettlement {
                batch,
                usage: OccupancyUsage {
                    byte_batches: total_units,
                    fee_units: total_fee,
                },
                fee_schedule: schedule,
                charges,
            },
            prior_state: self.canonical_state(),
            final_storage_sizes: canonical_storage_sizes(storage)?,
            next_positions: next,
        })
    }

    /// Applies a prepared transition after the kernel has accepted every exact
    /// debit. Kept crate-private until task 28.7 supplies that atomic bridge.
    pub(crate) fn commit_after_debits(
        &mut self,
        prepared: PreparedOccupancySettlement,
        current_storage: &Storage,
    ) -> Result<OccupancySettlement, OccupancyError> {
        if self.canonical_state() != prepared.prior_state
            || canonical_storage_sizes(current_storage)? != prepared.final_storage_sizes
        {
            return Err(OccupancyError::StalePreparation);
        }
        self.positions = prepared.next_positions;
        Ok(prepared.settlement)
    }

    /// Replays settlement evidence against the exact prior ledger and final
    /// committed storage, rejecting evidence that is internally consistent but
    /// does not describe this state transition.
    pub fn replay_evidence(
        &self,
        evidence: &[u8],
        final_storage: &Storage,
        responsibilities: impl IntoIterator<Item = OccupancyResponsibility>,
    ) -> Result<OccupancySettlement, OccupancyError> {
        let recorded = OccupancySettlement::canonical_decode(evidence)?;
        let prepared = self.prepare_batch(
            recorded.batch(),
            final_storage,
            responsibilities,
            recorded.fee_schedule(),
        )?;
        if prepared.settlement != recorded {
            return Err(OccupancyError::MalformedEvidence);
        }
        Ok(recorded)
    }

    #[must_use]
    pub fn canonical_state(&self) -> Vec<u8> {
        let mut out = LEDGER_DOMAIN.to_vec();
        out.extend_from_slice(&(self.positions.len() as u64).to_be_bytes());
        for (namespace, position) in &self.positions {
            encode_namespace(&mut out, *namespace);
            out.extend_from_slice(&position.payer.bytes());
            out.extend_from_slice(&position.bytes.to_be_bytes());
            out.extend_from_slice(&position.batch.to_be_bytes());
        }
        out
    }

    pub fn canonical_decode(encoded: &[u8]) -> Result<Self, OccupancyError> {
        let mut cursor = Cursor::new(encoded);
        if cursor.take(LEDGER_DOMAIN.len())? != LEDGER_DOMAIN {
            return Err(OccupancyError::MalformedEvidence);
        }
        let count =
            usize::try_from(cursor.u64()?).map_err(|_| OccupancyError::MalformedEvidence)?;
        const MIN_POSITION_BYTES: usize = 1 + 33 + 32 + 8 + 8;
        if count > cursor.remaining.len() / MIN_POSITION_BYTES {
            return Err(OccupancyError::MalformedEvidence);
        }
        let mut positions = BTreeMap::new();
        let mut prior = None;
        for _ in 0..count {
            let namespace = decode_namespace(&mut cursor)?;
            if prior.is_some_and(|value| value >= namespace) {
                return Err(OccupancyError::MalformedEvidence);
            }
            prior = Some(namespace);
            let payer =
                PrincipalId::new(cursor.array()?).map_err(|_| OccupancyError::MalformedEvidence)?;
            if namespace
                .principal_scope()
                .is_some_and(|principal| principal != payer)
            {
                return Err(OccupancyError::MalformedEvidence);
            }
            let bytes = cursor.u64()?;
            if bytes == 0 {
                return Err(OccupancyError::MalformedEvidence);
            }
            let batch = cursor.u64()?;
            positions.insert(
                namespace,
                OccupancyPosition {
                    payer,
                    bytes,
                    batch,
                },
            );
        }
        if !cursor.is_empty() {
            return Err(OccupancyError::MalformedEvidence);
        }
        Ok(Self { positions })
    }
    #[must_use]
    pub fn recorded_bytes(&self, namespace: StorageNamespace) -> Option<u64> {
        self.positions
            .get(&namespace)
            .map(|position| position.bytes)
    }
}

fn encode_namespace(out: &mut Vec<u8>, namespace: StorageNamespace) {
    let bytes = namespace.canonical_bytes();
    out.push(u8::try_from(bytes.len()).unwrap_or(u8::MAX));
    out.extend_from_slice(&bytes);
}

fn canonical_storage_sizes(storage: &Storage) -> Result<Vec<u8>, OccupancyError> {
    let sizes = storage.namespace_sizes()?;
    let mut out = Vec::new();
    out.extend_from_slice(&(sizes.len() as u64).to_be_bytes());
    for (namespace, bytes) in sizes {
        encode_namespace(&mut out, namespace);
        out.extend_from_slice(&bytes.to_be_bytes());
    }
    Ok(out)
}
fn decode_namespace(cursor: &mut Cursor<'_>) -> Result<StorageNamespace, OccupancyError> {
    let length = usize::from(cursor.byte()?);
    let bytes = cursor.take(length)?;
    if length != 33 && length != 65 {
        return Err(OccupancyError::MalformedEvidence);
    }
    let program = ProgramId::new(
        bytes[0..32]
            .try_into()
            .map_err(|_| OccupancyError::MalformedEvidence)?,
    )
    .map_err(|_| OccupancyError::MalformedEvidence)?;
    match (bytes[32], length) {
        (0, 65) => Ok(StorageNamespace::principal(
            program,
            PrincipalId::new(
                bytes[33..65]
                    .try_into()
                    .map_err(|_| OccupancyError::MalformedEvidence)?,
            )
            .map_err(|_| OccupancyError::MalformedEvidence)?,
        )),
        (1, 33) => Ok(StorageNamespace::shared(program)),
        _ => Err(OccupancyError::MalformedEvidence),
    }
}
fn encode_schedule(out: &mut Vec<u8>, schedule: FeeSchedule) {
    for price in [
        schedule.cpu_price(),
        schedule.memory_byte_price(),
        schedule.storage_read_byte_price(),
        schedule.storage_write_byte_price(),
        schedule.output_value_price(),
        schedule.output_byte_price(),
        schedule.occupancy_byte_batch_price(),
    ] {
        out.extend_from_slice(&price.to_be_bytes());
    }
}
fn decode_schedule(cursor: &mut Cursor<'_>) -> Result<FeeSchedule, OccupancyError> {
    Ok(FeeSchedule::new(
        cursor.u64()?,
        cursor.u64()?,
        cursor.u64()?,
        cursor.u64()?,
        cursor.u64()?,
    )
    .with_output_byte_price(cursor.u64()?)
    .with_occupancy_byte_batch_price(cursor.u64()?))
}

struct Cursor<'a> {
    remaining: &'a [u8],
}
impl<'a> Cursor<'a> {
    const fn new(remaining: &'a [u8]) -> Self {
        Self { remaining }
    }
    fn take(&mut self, length: usize) -> Result<&'a [u8], OccupancyError> {
        let (value, rest) = self
            .remaining
            .split_at_checked(length)
            .ok_or(OccupancyError::MalformedEvidence)?;
        self.remaining = rest;
        Ok(value)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], OccupancyError> {
        self.take(N)?
            .try_into()
            .map_err(|_| OccupancyError::MalformedEvidence)
    }
    fn byte(&mut self) -> Result<u8, OccupancyError> {
        Ok(self.take(1)?[0])
    }
    fn u64(&mut self) -> Result<u64, OccupancyError> {
        Ok(u64::from_be_bytes(self.array()?))
    }
    fn u128(&mut self) -> Result<u128, OccupancyError> {
        Ok(u128::from_be_bytes(self.array()?))
    }
    const fn is_empty(&self) -> bool {
        self.remaining.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::{ActivityBudgetBinding, AdmittedBudget};
    use crate::meter::ResourceBudget;
    fn program(value: u8) -> ProgramId {
        ProgramId::new([value; 32]).unwrap_or_else(|error| panic!("program: {error}"))
    }
    fn principal(value: u8) -> PrincipalId {
        PrincipalId::new([value; 32]).unwrap_or_else(|error| panic!("principal: {error}"))
    }
    fn token(payer: PrincipalId) -> AdmittedBudget {
        AdmittedBudget::new(
            ResourceBudget::declared(),
            payer,
            ActivityBudgetBinding::new([9; 32]).unwrap_or_else(|error| panic!("binding: {error}")),
            u128::MAX,
            FeeSchedule::declared(),
            ResourceBudget::declared(),
        )
    }
    fn responsibility(namespace: StorageNamespace, payer: PrincipalId) -> OccupancyResponsibility {
        OccupancyResponsibility::from_admitted(namespace.program(), namespace, &token(payer))
            .unwrap_or_else(|error| panic!("responsibility: {error}"))
    }
    fn seed(storage: &mut Storage, namespace: StorageNamespace, key: &[u8], value: &[u8]) {
        let mut tx = storage.transaction(namespace);
        tx.write(key, value)
            .unwrap_or_else(|error| panic!("write: {error}"));
        assert_eq!(tx.commit(), 1);
    }

    #[test]
    fn staged_settlement_is_monotone_and_commits_only_after_debit_acceptance() {
        for bytes in 1usize..=24 {
            for batches in 1u64..=12 {
                let namespace = StorageNamespace::shared(program(1));
                let mut storage = Storage::new();
                seed(&mut storage, namespace, b"k", &vec![1; bytes]);
                let ledger = OccupancyLedger::new();
                let prepared = ledger
                    .prepare_batch(
                        10,
                        &storage,
                        [responsibility(namespace, principal(1))],
                        FeeSchedule::declared(),
                    )
                    .unwrap_or_else(|error| panic!("prepare: {error}"));
                assert_eq!(ledger.recorded_bytes(namespace), None);
                let mut committed = ledger;
                committed
                    .commit_after_debits(prepared, &storage)
                    .unwrap_or_else(|error| panic!("commit: {error}"));
                let held = committed
                    .prepare_batch(10 + batches, &storage, [], FeeSchedule::declared())
                    .unwrap_or_else(|error| panic!("held: {error}"));
                assert_eq!(
                    held.settlement().usage().byte_batches,
                    u128::try_from(bytes + 1).unwrap_or_else(|_| panic!("size"))
                        * u128::from(batches)
                );
            }
        }
    }

    #[test]
    fn drop_and_same_batch_rewrite_use_only_final_committed_size() {
        let namespace = StorageNamespace::principal(program(1), principal(1));
        let mut storage = Storage::new();
        seed(&mut storage, namespace, b"old", b"12345");
        let mut ledger = OccupancyLedger::new();
        let initial = ledger
            .prepare_batch(
                4,
                &storage,
                [responsibility(namespace, principal(1))],
                FeeSchedule::declared(),
            )
            .unwrap_or_else(|error| panic!("initial: {error}"));
        ledger
            .commit_after_debits(initial, &storage)
            .unwrap_or_else(|error| panic!("commit: {error}"));
        let drop = storage
            .namespace_drop_preview(namespace)
            .unwrap_or_else(|error| panic!("drop: {error}"));
        storage.reclaim_namespace(drop);
        seed(&mut storage, namespace, b"n", b"v");
        let prepared = ledger
            .prepare_batch(7, &storage, [], FeeSchedule::declared())
            .unwrap_or_else(|error| panic!("prepare: {error}"));
        assert_eq!(prepared.settlement().charges()[0].byte_batches(), 24);
        assert_eq!(prepared.settlement().charges()[0].final_bytes(), 2);
        {
            let _discarded = prepared.clone();
        }
        assert_eq!(ledger.recorded_bytes(namespace), Some(8));
        ledger
            .commit_after_debits(prepared, &storage)
            .unwrap_or_else(|error| panic!("commit: {error}"));
        let drop = storage
            .namespace_drop_preview(namespace)
            .unwrap_or_else(|error| panic!("drop: {error}"));
        storage.reclaim_namespace(drop);
        let prepared = ledger
            .prepare_batch(8, &storage, [], FeeSchedule::declared())
            .unwrap_or_else(|error| panic!("prepare: {error}"));
        ledger
            .commit_after_debits(prepared, &storage)
            .unwrap_or_else(|error| panic!("commit: {error}"));
        assert_eq!(ledger.recorded_bytes(namespace), None);
        assert_eq!(
            ledger
                .prepare_batch(20, &storage, [], FeeSchedule::declared())
                .unwrap_or_else(|error| panic!("post-drop: {error}"))
                .settlement()
                .usage()
                .byte_batches,
            0
        );
        assert!(ledger
            .prepare_batch(20, &storage, [], FeeSchedule::declared())
            .unwrap_or_else(|error| panic!("post-drop evidence: {error}"))
            .settlement()
            .charges()
            .is_empty());
        seed(&mut storage, namespace, b"again", b"state");
        assert_eq!(
            ledger
                .prepare_batch(21, &storage, [], FeeSchedule::declared())
                .map(|_| ()),
            Err(OccupancyError::MissingResponsibility { namespace })
        );
        let recreated = ledger
            .prepare_batch(
                21,
                &storage,
                [responsibility(namespace, principal(1))],
                FeeSchedule::declared(),
            )
            .unwrap_or_else(|error| panic!("recreate: {error}"));
        ledger
            .commit_after_debits(recreated, &storage)
            .unwrap_or_else(|error| panic!("recreate commit: {error}"));
        assert_eq!(ledger.recorded_bytes(namespace), Some(10));
    }

    #[test]
    fn coverage_authority_duplicates_and_stale_preparations_refuse_without_mutation() {
        let namespace = StorageNamespace::shared(program(1));
        let mut storage = Storage::new();
        seed(&mut storage, namespace, b"k", b"v");
        let mut ledger = OccupancyLedger::new();
        assert_eq!(
            ledger
                .prepare_batch(1, &storage, [], FeeSchedule::declared())
                .map(|_| ()),
            Err(OccupancyError::MissingResponsibility { namespace })
        );
        let declaration = responsibility(namespace, principal(1));
        assert_eq!(
            ledger
                .prepare_batch(
                    1,
                    &storage,
                    [declaration, declaration],
                    FeeSchedule::declared()
                )
                .map(|_| ()),
            Err(OccupancyError::DuplicateResponsibility { namespace })
        );
        assert_eq!(
            OccupancyResponsibility::from_admitted(program(2), namespace, &token(principal(1))),
            Err(OccupancyError::AuthorityMismatch { namespace })
        );
        let principal_namespace = StorageNamespace::principal(program(1), principal(2));
        assert_eq!(
            OccupancyResponsibility::from_admitted(
                program(1),
                principal_namespace,
                &token(principal(1)),
            ),
            Err(OccupancyError::AuthorityMismatch {
                namespace: principal_namespace,
            })
        );
        let first = ledger
            .prepare_batch(1, &storage, [declaration], FeeSchedule::declared())
            .unwrap_or_else(|error| panic!("first: {error}"));
        let stale = first.clone();
        seed(&mut storage, namespace, b"changed", b"after-prepare");
        assert_eq!(
            ledger.commit_after_debits(first.clone(), &storage),
            Err(OccupancyError::StalePreparation)
        );
        let mut restore = storage.transaction(namespace);
        restore
            .delete(b"changed")
            .unwrap_or_else(|error| panic!("restore: {error}"));
        assert_eq!(restore.commit(), 1);
        ledger
            .commit_after_debits(first, &storage)
            .unwrap_or_else(|error| panic!("commit: {error}"));
        assert_eq!(
            ledger
                .prepare_batch(
                    1,
                    &storage,
                    [responsibility(namespace, principal(2))],
                    FeeSchedule::declared(),
                )
                .map(|_| ()),
            Err(OccupancyError::ResponsibilityMismatch { namespace })
        );
        assert_eq!(
            ledger.commit_after_debits(stale, &storage),
            Err(OccupancyError::StalePreparation)
        );
    }

    #[test]
    fn evidence_and_restart_state_are_strict_canonical_and_replayable() {
        let left = StorageNamespace::shared(program(1));
        let right = StorageNamespace::shared(program(2));
        let mut storage = Storage::new();
        seed(&mut storage, right, b"z", b"right");
        seed(&mut storage, left, b"a", b"left");
        let mut ledger = OccupancyLedger::new();
        let schedule = FeeSchedule::declared().with_occupancy_byte_batch_price(7);
        let prepared = ledger
            .prepare_batch(
                3,
                &storage,
                [
                    responsibility(right, principal(2)),
                    responsibility(left, principal(1)),
                ],
                schedule,
            )
            .unwrap_or_else(|error| panic!("prepare: {error}"));
        let reversed = ledger
            .prepare_batch(
                3,
                &storage,
                [
                    responsibility(left, principal(1)),
                    responsibility(right, principal(2)),
                ],
                schedule,
            )
            .unwrap_or_else(|error| panic!("reverse: {error}"));
        assert_eq!(prepared.settlement(), reversed.settlement());
        ledger
            .commit_after_debits(prepared, &storage)
            .unwrap_or_else(|error| panic!("initial commit: {error}"));
        let prior = ledger.clone();
        let prepared = ledger
            .prepare_batch(5, &storage, [], schedule)
            .unwrap_or_else(|error| panic!("held prepare: {error}"));
        assert_eq!(prepared.settlement().usage().byte_batches, 22);
        assert_eq!(prepared.settlement().usage().fee_units, 154);
        assert_eq!(prepared.settlement().charges()[0].payer(), principal(1));
        assert_eq!(prepared.settlement().charges()[0].fee_units(), 70);
        assert_eq!(prepared.settlement().charges()[1].payer(), principal(2));
        assert_eq!(prepared.settlement().charges()[1].fee_units(), 84);
        let evidence = prepared.settlement().canonical_evidence();
        let decoded = OccupancySettlement::canonical_decode(&evidence)
            .unwrap_or_else(|error| panic!("decode: {error}"));
        assert_eq!(decoded.canonical_evidence(), evidence);
        for cut in 0..evidence.len() {
            assert_eq!(
                OccupancySettlement::canonical_decode(&evidence[..cut]),
                Err(OccupancyError::MalformedEvidence)
            );
        }
        let mut trailing = evidence.clone();
        trailing.push(0);
        assert_eq!(
            OccupancySettlement::canonical_decode(&trailing),
            Err(OccupancyError::MalformedEvidence)
        );
        let charge_offset = EVIDENCE_DOMAIN.len() + 8 + (7 * 8) + 16 + 16 + 8;
        let mut bad_namespace_length = evidence.clone();
        bad_namespace_length[charge_offset] = 34;
        assert_eq!(
            OccupancySettlement::canonical_decode(&bad_namespace_length),
            Err(OccupancyError::MalformedEvidence)
        );
        let mut bad_total = evidence.clone();
        bad_total[EVIDENCE_DOMAIN.len() + 8 + (7 * 8) + 15] ^= 1;
        assert_eq!(
            OccupancySettlement::canonical_decode(&bad_total),
            Err(OccupancyError::MalformedEvidence)
        );
        assert_eq!(
            prior
                .replay_evidence(&evidence, &storage, [])
                .unwrap_or_else(|error| panic!("replay: {error}"))
                .canonical_evidence(),
            evidence
        );
        let mut other_storage = storage.clone();
        seed(&mut other_storage, left, b"extra", b"state");
        assert_eq!(
            prior.replay_evidence(&evidence, &other_storage, []),
            Err(OccupancyError::MalformedEvidence)
        );
        ledger
            .commit_after_debits(prepared, &storage)
            .unwrap_or_else(|error| panic!("commit: {error}"));
        let state = ledger.canonical_state();
        let restored = OccupancyLedger::canonical_decode(&state)
            .unwrap_or_else(|error| panic!("state: {error}"));
        assert_eq!(restored.canonical_state(), state);
        let mut corrupt = state.clone();
        corrupt.push(0);
        assert_eq!(
            OccupancyLedger::canonical_decode(&corrupt),
            Err(OccupancyError::MalformedEvidence)
        );
        let first_position_bytes = LEDGER_DOMAIN.len() + 8 + 1 + 33 + 32;
        let mut zero_tombstone = state.clone();
        zero_tombstone[first_position_bytes..first_position_bytes + 8].fill(0);
        assert_eq!(
            OccupancyLedger::canonical_decode(&zero_tombstone),
            Err(OccupancyError::MalformedEvidence)
        );
    }

    #[test]
    fn u128_boundaries_fee_overflow_and_batch_regression_are_atomic() {
        let first = StorageNamespace::shared(program(1));
        let second = StorageNamespace::shared(program(2));
        let position = |payer| OccupancyPosition {
            payer,
            bytes: u64::MAX,
            batch: 0,
        };
        let one = OccupancyLedger {
            positions: BTreeMap::from([(first, position(principal(1)))]),
        };
        let exact = one
            .prepare_batch(
                u64::MAX,
                &Storage::new(),
                [],
                FeeSchedule::declared().with_occupancy_byte_batch_price(1),
            )
            .unwrap_or_else(|error| panic!("exact: {error}"));
        assert_eq!(
            exact.settlement().usage().byte_batches,
            u128::from(u64::MAX) * u128::from(u64::MAX)
        );
        assert_eq!(
            one.prepare_batch(
                u64::MAX,
                &Storage::new(),
                [],
                FeeSchedule::declared().with_occupancy_byte_batch_price(2),
            )
            .map(|_| ()),
            Err(OccupancyError::ArithmeticOverflow)
        );
        let two = OccupancyLedger {
            positions: BTreeMap::from([
                (first, position(principal(1))),
                (second, position(principal(2))),
            ]),
        };
        assert_eq!(
            two.prepare_batch(
                u64::MAX,
                &Storage::new(),
                [],
                FeeSchedule::declared().with_occupancy_byte_batch_price(1),
            )
            .map(|_| ()),
            Err(OccupancyError::ArithmeticOverflow)
        );
        let regressing = OccupancyLedger {
            positions: BTreeMap::from([(
                first,
                OccupancyPosition {
                    payer: principal(1),
                    bytes: 1,
                    batch: 5,
                },
            )]),
        };
        let state = regressing.canonical_state();
        assert_eq!(
            regressing
                .prepare_batch(4, &Storage::new(), [], FeeSchedule::declared())
                .map(|_| ()),
            Err(OccupancyError::BatchRegression {
                previous: 5,
                attempted: 4,
            })
        );
        let restored = OccupancyLedger::canonical_decode(&state)
            .unwrap_or_else(|error| panic!("restore: {error}"));
        assert_eq!(
            restored
                .prepare_batch(4, &Storage::new(), [], FeeSchedule::declared())
                .map(|_| ()),
            Err(OccupancyError::BatchRegression {
                previous: 5,
                attempted: 4,
            })
        );
    }
}
