//! Deterministic, receipt-bound storage occupancy accounting.

use core::fmt::{self, Display};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

use crate::budget::AdmittedBudget;
use crate::meter::FeeSchedule;
use crate::storage::{PrincipalId, ProgramId, Storage, StorageError, StorageNamespace};

const EVIDENCE_DOMAIN_V1: &[u8] = b"LXP/storage-occupancy-settlement/v1\0";
const EVIDENCE_DOMAIN_V2: &[u8] = b"LXP/storage-occupancy-settlement/v2\0";
const EVIDENCE_DOMAIN: &[u8] = b"LXP/storage-occupancy-settlement/v3\0";
const LEDGER_DOMAIN_V1: &[u8] = b"LXP/storage-occupancy-ledger/v1\0";
const LEDGER_DOMAIN: &[u8] = b"LXP/storage-occupancy-ledger/v2\0";
const MANDATE_DOMAIN: &[u8] = b"LXP/storage-occupancy-mandate/v1\0";

pub const MAX_OCCUPANCY_POSITIONS: usize = 256;
pub const MAX_OCCUPANCY_LEDGER_BYTES: usize = 60_000;
pub const MAX_OCCUPANCY_EVIDENCE_BYTES: usize = 65_536;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OccupancyAuthority {
    payer: PrincipalId,
    root_program: ProgramId,
    activity_binding: [u8; 32],
    occupancy_fee_ceiling: u128,
    maximum_price: u64,
}

impl OccupancyAuthority {
    pub(crate) fn from_admitted(
        admitted: &AdmittedBudget,
        signed_fee_limit: u128,
        schedule: FeeSchedule,
        root_program: ProgramId,
    ) -> Result<Self, OccupancyError> {
        let occupancy_fee_ceiling = signed_fee_limit
            .checked_sub(admitted.maximum_fee_units())
            .ok_or(OccupancyError::ResponsibilityCeilingExceeded)?;
        Ok(Self {
            payer: admitted.payer(),
            root_program,
            activity_binding: admitted.activity_binding().bytes(),
            occupancy_fee_ceiling,
            maximum_price: schedule.occupancy_byte_batch_price(),
        })
    }

    pub(crate) fn authorize(
        self,
        namespace: StorageNamespace,
        maximum_bytes: u64,
        charge_ceiling: u128,
    ) -> Result<OccupancyResponsibility, OccupancyError> {
        if maximum_bytes == 0
            || namespace
                .principal_scope()
                .is_some_and(|principal| principal != self.payer)
        {
            return Err(OccupancyError::AuthorityMismatch { namespace });
        }
        let maximum_fee = u128::from(maximum_bytes)
            .checked_mul(u128::from(self.maximum_price))
            .ok_or(OccupancyError::ArithmeticOverflow)?;
        if maximum_fee > charge_ceiling || charge_ceiling > self.occupancy_fee_ceiling {
            return Err(OccupancyError::ResponsibilityCeilingExceeded);
        }
        Ok(OccupancyResponsibility {
            namespace,
            payer: self.payer,
            root_program: self.root_program,
            activity_binding: self.activity_binding,
            maximum_bytes,
            maximum_price: self.maximum_price,
            charge_ceiling,
            mandate: mandate_digest(
                self.payer,
                self.root_program,
                self.activity_binding,
                namespace,
                maximum_bytes,
                self.maximum_price,
                charge_ceiling,
            ),
        })
    }

    pub(crate) const fn fee_ceiling(self) -> u128 {
        self.occupancy_fee_ceiling
    }
    pub(crate) const fn payer(self) -> PrincipalId { self.payer }
    pub(crate) const fn root_program(self) -> ProgramId { self.root_program }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OccupancyResponsibility {
    namespace: StorageNamespace,
    payer: PrincipalId,
    root_program: ProgramId,
    activity_binding: [u8; 32],
    maximum_bytes: u64,
    maximum_price: u64,
    charge_ceiling: u128,
    mandate: [u8; 32],
}

impl OccupancyResponsibility {
    #[must_use]
    pub const fn namespace(self) -> StorageNamespace { self.namespace }
    #[must_use]
    pub const fn payer(self) -> PrincipalId { self.payer }
    #[must_use]
    pub const fn root_program(self) -> ProgramId { self.root_program }
    #[must_use]
    pub const fn activity_binding(self) -> [u8; 32] { self.activity_binding }
    #[must_use]
    pub const fn maximum_bytes(self) -> u64 { self.maximum_bytes }
    #[must_use]
    pub const fn maximum_price(self) -> u64 { self.maximum_price }
    #[must_use]
    pub const fn charge_ceiling(self) -> u128 { self.charge_ceiling }
    #[must_use]
    pub const fn mandate(self) -> [u8; 32] { self.mandate }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct OccupancyPosition {
    payer: PrincipalId,
    root_program: ProgramId,
    activity_binding: [u8; 32],
    bytes: u64,
    batch: u64,
    maximum_bytes: u64,
    maximum_price: u64,
    remaining_fee_units: u128,
    mandate: [u8; 32],
    arrears: u128,
    frozen: bool,
    legacy: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OccupancyUsage {
    pub byte_batches: u128,
    pub fee_units: u128,
    pub paid_fee_units: u128,
    pub arrears_fee_units: u128,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OccupancyCharge {
    namespace: StorageNamespace,
    payer: PrincipalId,
    root_program: ProgramId,
    activity_binding: [u8; 32],
    from_batch: u64,
    to_batch: u64,
    recorded_bytes: u64,
    final_bytes: u64,
    byte_batches: u128,
    price: u64,
    accrued_fee_units: u128,
    prior_arrears: u128,
    amount_due: u128,
    authorized_added_fee_units: u128,
    disposition: OccupancyDisposition,
    arrears_after: u128,
    maximum_bytes: u64,
    maximum_price: u64,
    remaining_fee_units: u128,
    mandate: [u8; 32],
}

impl OccupancyCharge {
    #[must_use]
    pub const fn namespace(self) -> StorageNamespace { self.namespace }
    #[must_use]
    pub const fn payer(self) -> PrincipalId { self.payer }
    #[must_use]
    pub const fn root_program(self) -> ProgramId { self.root_program }
    #[must_use]
    pub const fn activity_binding(self) -> [u8; 32] { self.activity_binding }
    #[must_use]
    pub const fn from_batch(self) -> u64 { self.from_batch }
    #[must_use]
    pub const fn to_batch(self) -> u64 { self.to_batch }
    #[must_use]
    pub const fn recorded_bytes(self) -> u64 { self.recorded_bytes }
    #[must_use]
    pub const fn final_bytes(self) -> u64 { self.final_bytes }
    #[must_use]
    pub const fn byte_batches(self) -> u128 { self.byte_batches }
    #[must_use]
    pub const fn price(self) -> u64 { self.price }
    #[must_use]
    pub const fn fee_units(self) -> u128 { self.accrued_fee_units }
    #[must_use]
    pub const fn prior_arrears(self) -> u128 { self.prior_arrears }
    #[must_use]
    pub const fn amount_due(self) -> u128 { self.amount_due }
    #[must_use]
    pub const fn paid(self) -> bool { matches!(self.disposition, OccupancyDisposition::Paid) }
    #[must_use]
    pub const fn disposition(self) -> OccupancyDisposition { self.disposition }
    #[must_use]
    pub const fn arrears_after(self) -> u128 { self.arrears_after }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum OccupancyDisposition {
    Paid = 1,
    InsufficientFunds = 2,
    ChargeCeilingExceeded = 3,
    ScheduleCeilingExceeded = 4,
    MigrationRequired = 5,
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
    pub const fn batch(&self) -> u64 { self.batch }
    #[must_use]
    pub const fn usage(&self) -> OccupancyUsage { self.usage }
    #[must_use]
    pub const fn fee_schedule(&self) -> FeeSchedule { self.fee_schedule }
    #[must_use]
    pub fn charges(&self) -> &[OccupancyCharge] { &self.charges }

    pub fn transfer_root(&self,asset:[u8;32])->Result<[u8;32],OccupancyError>{
        const LEAF:&[u8]=b"LXP/v1/merkle-leaf\0";const INTERNAL:&[u8]=b"LXP/v1/merkle-internal\0";
        if asset==[0;32]{return Err(OccupancyError::MalformedEvidence)}
        let mut treasury_preimage=b"LX:ACCOUNT:v1".to_vec();treasury_preimage.extend_from_slice(&11_u32.to_be_bytes());treasury_preimage.extend_from_slice(b"system:fees");
        let treasury:[u8;32]=Sha256::digest(treasury_preimage).into();
        let payers=self.payer_dispositions()?;let mut level=Vec::new();
        for (payer,(_,paid,_,_)) in payers {if paid==0{continue}let mut leg=Vec::with_capacity(115);leg.push(0);leg.extend_from_slice(&payer.bytes());leg.extend_from_slice(&treasury);leg.extend_from_slice(&asset);leg.extend_from_slice(&paid.to_be_bytes());leg.extend_from_slice(&23_u16.to_be_bytes());let mut leaf=LEAF.to_vec();leaf.extend_from_slice(&leg);level.push(<[u8;32]>::from(Sha256::digest(leaf)));}
        if level.is_empty(){return Ok([0;32])}while level.len()>1{let mut next=Vec::with_capacity(level.len().div_ceil(2));for pair in level.chunks(2){let right=pair.get(1).unwrap_or(&pair[0]);let mut preimage=INTERNAL.to_vec();preimage.extend_from_slice(&pair[0]);preimage.extend_from_slice(right);next.push(<[u8;32]>::from(Sha256::digest(preimage)));}level=next;}Ok(level[0])
    }

    pub fn payer_dispositions(
        &self,
    ) -> Result<BTreeMap<PrincipalId, (u128, u128, u128, bool)>, OccupancyError> {
        let mut payers = BTreeMap::new();
        for charge in &self.charges {
            let entry = payers.entry(charge.payer).or_insert((0, 0, 0, false));
            entry.0 = checked_add(entry.0, charge.amount_due)?;
            if charge.paid() { entry.1 = checked_add(entry.1, charge.amount_due)?; }
            entry.2 = checked_add(entry.2, charge.arrears_after)?;
            entry.3 |= !charge.paid() && charge.amount_due != 0;
        }
        payers.retain(|_, values| values.0 != 0 || values.2 != 0);
        Ok(payers)
    }

    #[must_use]
    pub fn canonical_evidence(&self) -> Vec<u8> {
        let mut out = EVIDENCE_DOMAIN.to_vec();
        out.extend_from_slice(&self.batch.to_be_bytes());
        encode_schedule(&mut out, self.fee_schedule);
        out.extend_from_slice(&self.usage.byte_batches.to_be_bytes());
        out.extend_from_slice(&self.usage.fee_units.to_be_bytes());
        out.extend_from_slice(&self.usage.paid_fee_units.to_be_bytes());
        out.extend_from_slice(&self.usage.arrears_fee_units.to_be_bytes());
        out.extend_from_slice(&(self.charges.len() as u32).to_be_bytes());
        for charge in &self.charges {
            encode_namespace(&mut out, charge.namespace);
            out.extend_from_slice(&charge.payer.bytes());
            out.extend_from_slice(&charge.root_program.bytes());
            out.extend_from_slice(&charge.activity_binding);
            out.extend_from_slice(&charge.from_batch.to_be_bytes());
            out.extend_from_slice(&charge.to_batch.to_be_bytes());
            out.extend_from_slice(&charge.recorded_bytes.to_be_bytes());
            out.extend_from_slice(&charge.final_bytes.to_be_bytes());
            out.extend_from_slice(&charge.byte_batches.to_be_bytes());
            out.extend_from_slice(&charge.price.to_be_bytes());
            out.extend_from_slice(&charge.accrued_fee_units.to_be_bytes());
            out.extend_from_slice(&charge.prior_arrears.to_be_bytes());
            out.extend_from_slice(&charge.amount_due.to_be_bytes());
            out.extend_from_slice(&charge.authorized_added_fee_units.to_be_bytes());
            out.push(charge.disposition as u8);
            out.extend_from_slice(&charge.arrears_after.to_be_bytes());
            out.extend_from_slice(&charge.maximum_bytes.to_be_bytes());
            out.extend_from_slice(&charge.maximum_price.to_be_bytes());
            out.extend_from_slice(&charge.remaining_fee_units.to_be_bytes());
            out.extend_from_slice(&charge.mandate);
        }
        out
    }

    pub fn canonical_decode(encoded: &[u8]) -> Result<Self, OccupancyError> {
        if encoded.len() > MAX_OCCUPANCY_EVIDENCE_BYTES { return Err(OccupancyError::LengthLimit); }
        if encoded.starts_with(EVIDENCE_DOMAIN_V1) || encoded.starts_with(EVIDENCE_DOMAIN_V2) {
            return decode_legacy_settlement(encoded);
        }
        let mut cursor = Cursor::new(encoded);
        if cursor.take(EVIDENCE_DOMAIN.len())? != EVIDENCE_DOMAIN { return Err(OccupancyError::MalformedEvidence); }
        let batch = cursor.u64()?;
        let fee_schedule = decode_schedule(&mut cursor, true)?;
        let declared_units = cursor.u128()?;
        let declared_accrued = cursor.u128()?;
        let declared_paid = cursor.u128()?;
        let declared_arrears = cursor.u128()?;
        let count = usize::try_from(cursor.u32()?).map_err(|_| OccupancyError::MalformedEvidence)?;
        if count > MAX_OCCUPANCY_POSITIONS { return Err(OccupancyError::LengthLimit); }
        let mut charges = Vec::with_capacity(count);
        let mut prior = None;
        let mut usage = OccupancyUsage::default();
        for _ in 0..count {
            let namespace = decode_namespace(&mut cursor)?;
            if prior.is_some_and(|value| value >= namespace) { return Err(OccupancyError::MalformedEvidence); }
            prior = Some(namespace);
            let payer = PrincipalId::new(cursor.array()?).map_err(|_| OccupancyError::MalformedEvidence)?;
            validate_scope(namespace, payer)?;
            let root_program = ProgramId::new(cursor.array()?).map_err(|_| OccupancyError::MalformedEvidence)?;
            let activity_binding = cursor.array()?;
            let from_batch = cursor.u64()?;
            let to_batch = cursor.u64()?;
            let recorded_bytes = cursor.u64()?;
            let final_bytes = cursor.u64()?;
            let byte_batches = cursor.u128()?;
            let price = cursor.u64()?;
            let accrued_fee_units = cursor.u128()?;
            let prior_arrears = cursor.u128()?;
            let amount_due = cursor.u128()?;
            let authorized_added_fee_units = cursor.u128()?;
            let disposition = disposition(cursor.byte()?)?;
            let arrears_after = cursor.u128()?;
            let maximum_bytes = cursor.u64()?;
            let maximum_price = cursor.u64()?;
            let remaining_fee_units = cursor.u128()?;
            let mandate = cursor.array()?;
            let intervals = to_batch.checked_sub(from_batch).ok_or(OccupancyError::MalformedEvidence)?;
            let computed_units = u128::from(recorded_bytes).checked_mul(u128::from(intervals)).ok_or(OccupancyError::ArithmeticOverflow)?;
            let computed_fee = computed_units.checked_mul(u128::from(price)).ok_or(OccupancyError::ArithmeticOverflow)?;
            let computed_due = prior_arrears.checked_add(computed_fee).ok_or(OccupancyError::ArithmeticOverflow)?;
            let migration = matches!(disposition, OccupancyDisposition::MigrationRequired);
            if to_batch != batch || (!migration && price != fee_schedule.occupancy_byte_batch_price())
                || byte_batches != computed_units || accrued_fee_units != computed_fee
                || amount_due != computed_due || final_bytes > maximum_bytes
                || (!migration && (mandate == [0; 32] || activity_binding == [0; 32]))
                || (migration && (price != 0 || accrued_fee_units != 0 || prior_arrears != 0 ||
                    amount_due != 0 || arrears_after != 0 || mandate != [0; 32] ||
                    activity_binding != [0; 32] || root_program != namespace.program()))
                || (authorized_added_fee_units != 0 && mandate != mandate_digest(
                    payer,
                    root_program,
                    activity_binding,
                    namespace,
                    maximum_bytes,
                    maximum_price,
                    authorized_added_fee_units,
                ))
                || (matches!(disposition, OccupancyDisposition::ScheduleCeilingExceeded)
                    != (price > maximum_price))
                || (matches!(disposition, OccupancyDisposition::Paid) && arrears_after != 0)
                || (!matches!(disposition, OccupancyDisposition::Paid) && arrears_after != amount_due)
            { return Err(OccupancyError::MalformedEvidence); }
            usage.byte_batches = checked_add(usage.byte_batches, byte_batches)?;
            usage.fee_units = checked_add(usage.fee_units, accrued_fee_units)?;
            if matches!(disposition, OccupancyDisposition::Paid) { usage.paid_fee_units = checked_add(usage.paid_fee_units, amount_due)?; }
            else { usage.arrears_fee_units = checked_add(usage.arrears_fee_units, arrears_after)?; }
            charges.push(OccupancyCharge { namespace, payer, root_program, activity_binding,
                from_batch, to_batch, recorded_bytes,
                final_bytes, byte_batches, price, accrued_fee_units, prior_arrears, amount_due,
                authorized_added_fee_units,
                disposition, arrears_after, maximum_bytes, maximum_price,
                remaining_fee_units, mandate });
        }
        if !cursor.is_empty() || usage.byte_batches != declared_units || usage.fee_units != declared_accrued
            || usage.paid_fee_units != declared_paid || usage.arrears_fee_units != declared_arrears
        { return Err(OccupancyError::MalformedEvidence); }
        Ok(Self { batch, usage, fee_schedule, charges })
    }
}

#[derive(Clone, Debug)]
pub struct PreparedOccupancySettlement {
    settlement: OccupancySettlement,
    prior_state: Vec<u8>,
    final_storage_sizes: Vec<u8>,
    next_positions: BTreeMap<StorageNamespace, OccupancyPosition>,
    finalizes_batch: bool,
}

impl PreparedOccupancySettlement {
    #[must_use]
    pub const fn settlement(&self) -> &OccupancySettlement { &self.settlement }

    pub(crate) fn defer_unpaid(&mut self, unpaid: &BTreeSet<PrincipalId>) -> Result<(), OccupancyError> {
        self.settlement.usage.paid_fee_units = 0;
        self.settlement.usage.arrears_fee_units = 0;
        for charge in &mut self.settlement.charges {
            let position = self.next_positions.get_mut(&charge.namespace).ok_or(OccupancyError::StalePreparation)?;
            if charge.amount_due != 0 && charge.paid() && unpaid.contains(&charge.payer) {
                charge.disposition = OccupancyDisposition::InsufficientFunds;
                charge.arrears_after = charge.amount_due;
                position.remaining_fee_units = position.remaining_fee_units
                    .checked_add(charge.amount_due)
                    .ok_or(OccupancyError::ArithmeticOverflow)?;
                charge.remaining_fee_units = position.remaining_fee_units;
                position.arrears = charge.amount_due;
                position.frozen = true;
                self.settlement.usage.arrears_fee_units = checked_add(self.settlement.usage.arrears_fee_units, charge.amount_due)?;
            } else if charge.paid() {
                charge.arrears_after = 0;
                position.arrears = 0;
                position.frozen = false;
                self.settlement.usage.paid_fee_units = checked_add(self.settlement.usage.paid_fee_units, charge.amount_due)?;
            } else {
                self.settlement.usage.arrears_fee_units = checked_add(
                    self.settlement.usage.arrears_fee_units,
                    charge.arrears_after,
                )?;
            }
        }
        self.next_positions.retain(|_, position| position.bytes != 0 || position.arrears != 0);
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OccupancyError {
    AuthorityMismatch { namespace: StorageNamespace },
    ResponsibilityMismatch { namespace: StorageNamespace },
    DuplicateResponsibility { namespace: StorageNamespace },
    MissingResponsibility { namespace: StorageNamespace },
    ResponsibilityCeilingExceeded,
    ScheduleNotAuthorized { namespace: StorageNamespace },
    FrozenNamespace { namespace: StorageNamespace },
    BatchRegression { previous: u64, attempted: u64 },
    StalePreparation,
    ArithmeticOverflow,
    LengthLimit,
    MalformedEvidence,
    Storage(StorageError),
}

impl Display for OccupancyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AuthorityMismatch { .. } => formatter.write_str("occupancy mandate authority mismatch"),
            Self::ResponsibilityMismatch { .. } => formatter.write_str("occupancy payer cannot be rebound"),
            Self::DuplicateResponsibility { .. } => formatter.write_str("duplicate occupancy mandate"),
            Self::MissingResponsibility { .. } => formatter.write_str("occupied namespace has no mandate"),
            Self::ResponsibilityCeilingExceeded => formatter.write_str("occupancy mandate ceiling exceeded"),
            Self::ScheduleNotAuthorized { .. } => formatter.write_str("occupancy schedule exceeds persisted mandate"),
            Self::FrozenNamespace { .. } => formatter.write_str("occupancy namespace is frozen by arrears"),
            Self::BatchRegression { previous, attempted } => write!(formatter, "occupancy batch {attempted} precedes {previous}"),
            Self::StalePreparation => formatter.write_str("stale occupancy preparation"),
            Self::ArithmeticOverflow => formatter.write_str("occupancy arithmetic overflow"),
            Self::LengthLimit => formatter.write_str("occupancy state exceeds protocol bounds"),
            Self::MalformedEvidence => formatter.write_str("malformed occupancy evidence"),
            Self::Storage(error) => write!(formatter, "occupancy storage refusal: {error}"),
        }
    }
}
impl std::error::Error for OccupancyError {}
impl From<StorageError> for OccupancyError { fn from(value: StorageError) -> Self { Self::Storage(value) } }

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OccupancyLedger {
    last_finalized_batch: u64,
    positions: BTreeMap<StorageNamespace, OccupancyPosition>,
}

impl OccupancyLedger {
    #[must_use]
    pub const fn new() -> Self { Self { last_finalized_batch: 0, positions: BTreeMap::new() } }
    #[must_use]
    pub const fn activated_after(last_finalized_batch: u64) -> Self {
        Self { last_finalized_batch, positions: BTreeMap::new() }
    }
    #[must_use]
    pub const fn last_finalized_batch(&self) -> u64 { self.last_finalized_batch }
    #[must_use]
    pub fn contains_namespace(&self, namespace: StorageNamespace) -> bool { self.positions.contains_key(&namespace) }
    pub(crate) fn responsibility_limits(
        &self,
        namespace: StorageNamespace,
    ) -> Option<(PrincipalId, u64)> {
        self.positions
            .get(&namespace)
            .map(|position| (position.payer, position.maximum_bytes))
    }
    pub fn ensure_accessible(&self, namespaces: impl IntoIterator<Item = StorageNamespace>) -> Result<(), OccupancyError> {
        for namespace in namespaces {
            if self.positions.get(&namespace).is_some_and(|position| position.frozen) {
                return Err(OccupancyError::FrozenNamespace { namespace });
            }
        }
        Ok(())
    }

    pub(crate) fn frozen_namespaces(&self) -> impl Iterator<Item = StorageNamespace> + '_ {
        self.positions.iter().filter_map(|(namespace, position)| position.frozen.then_some(*namespace))
    }
    pub(crate) fn requires_migration(&self, namespace: StorageNamespace) -> bool {
        self.positions.get(&namespace).is_some_and(|position| position.legacy)
    }
    pub(crate) fn import_activation_positions(
        &mut self,
        storage: &Storage,
        program_owners: &BTreeMap<ProgramId, PrincipalId>,
    ) -> Result<(), OccupancyError> {
        for (namespace, bytes) in storage.namespace_sizes()? {
            let payer = match namespace.principal_scope() {
                Some(principal) => principal,
                None => *program_owners
                    .get(&namespace.program())
                    .ok_or(OccupancyError::MissingResponsibility { namespace })?,
            };
            self.import_activation_position(namespace, payer, bytes)?;
        }
        Ok(())
    }

    pub(crate) fn import_activation_position(
        &mut self,
        namespace: StorageNamespace,
        payer: PrincipalId,
        bytes: u64,
    ) -> Result<(), OccupancyError> {
        validate_scope(namespace, payer)?;
        if bytes == 0 { return Err(OccupancyError::MalformedEvidence); }
        if self.positions.contains_key(&namespace) { return Ok(()); }
        if self.positions.len() == MAX_OCCUPANCY_POSITIONS {
            return Err(OccupancyError::LengthLimit);
        }
        self.positions.insert(namespace, OccupancyPosition {
                payer,
                root_program: namespace.program(),
                activity_binding: [0; 32],
                bytes,
                batch: self.last_finalized_batch,
                maximum_bytes: bytes,
                maximum_price: 0,
                remaining_fee_units: 0,
                mandate: [0; 32],
                arrears: 0,
                frozen: true,
                legacy: true,
            });
        Ok(())
    }

    pub fn prepare_unchanged_batch(&self, batch: u64, schedule: FeeSchedule) -> Result<PreparedOccupancySettlement, OccupancyError> {
        let mut prepared = self.prepare_positions(batch, canonical_position_sizes(&self.positions)?, BTreeMap::new(), schedule)?;
        prepared.finalizes_batch = true;
        Ok(prepared)
    }

    pub fn prepare_batch(
        &self,
        batch: u64,
        storage: &Storage,
        responsibilities: impl IntoIterator<Item = OccupancyResponsibility>,
        schedule: FeeSchedule,
    ) -> Result<PreparedOccupancySettlement, OccupancyError> {
        let sizes: BTreeMap<_, _> = storage.namespace_sizes()?.into_iter().collect();
        let mut declarations = BTreeMap::new();
        for responsibility in responsibilities {
            if declarations.insert(responsibility.namespace, responsibility).is_some() {
                return Err(OccupancyError::DuplicateResponsibility { namespace: responsibility.namespace });
            }
        }
        self.prepare_positions(batch, canonical_sizes(&sizes)?, declarations, schedule)
    }

    fn prepare_positions(
        &self,
        batch: u64,
        final_storage_sizes: Vec<u8>,
        declarations: BTreeMap<StorageNamespace, OccupancyResponsibility>,
        schedule: FeeSchedule,
    ) -> Result<PreparedOccupancySettlement, OccupancyError> {
        let expected = self.last_finalized_batch.checked_add(1).ok_or(OccupancyError::ArithmeticOverflow)?;
        if batch != expected {
            return Err(OccupancyError::BatchRegression { previous: self.last_finalized_batch, attempted: batch });
        }
        if self.positions.len() > MAX_OCCUPANCY_POSITIONS || declarations.len() > MAX_OCCUPANCY_POSITIONS { return Err(OccupancyError::LengthLimit); }
        let final_sizes = decode_sizes(&final_storage_sizes)?;
        let mut next = self.positions.clone();
        let mut authorized_additions = BTreeMap::new();
        for (namespace, responsibility) in declarations {
            validate_scope(namespace, responsibility.payer)?;
            match next.get_mut(&namespace) {
                Some(position) if !position.legacy &&
                    position.payer != responsibility.payer =>
                    return Err(OccupancyError::ResponsibilityMismatch { namespace }),
                Some(position) => {
                    if !position.legacy && position.root_program != responsibility.root_program {
                        return Err(OccupancyError::ResponsibilityMismatch { namespace });
                    }
                    if position.legacy {
                        position.payer = responsibility.payer;
                    }
                    position.root_program = responsibility.root_program;
                    position.activity_binding = responsibility.activity_binding;
                    position.maximum_bytes = responsibility.maximum_bytes;
                    position.maximum_price = responsibility.maximum_price;
                    position.remaining_fee_units = position.remaining_fee_units
                        .checked_add(responsibility.charge_ceiling)
                        .ok_or(OccupancyError::ArithmeticOverflow)?;
                    position.mandate = responsibility.mandate;
                    if position.legacy { position.frozen = false; }
                    position.legacy = false;
                }
                None => { next.insert(namespace, OccupancyPosition { payer: responsibility.payer,
                    root_program: responsibility.root_program,
                    activity_binding: responsibility.activity_binding, bytes: 0, batch,
                    maximum_bytes: responsibility.maximum_bytes, maximum_price: responsibility.maximum_price,
                    remaining_fee_units: responsibility.charge_ceiling,
                    mandate: responsibility.mandate, arrears: 0, frozen: false, legacy: false }); }
            }
            authorized_additions.insert(namespace, responsibility.charge_ceiling);
        }
        for namespace in final_sizes.keys() {
            if !next.contains_key(namespace) { return Err(OccupancyError::MissingResponsibility { namespace: *namespace }); }
        }
        if next.len() > MAX_OCCUPANCY_POSITIONS { return Err(OccupancyError::LengthLimit); }
        let price = schedule.occupancy_byte_batch_price();
        let mut usage = OccupancyUsage::default();
        let mut charges = Vec::with_capacity(next.len());
        for (namespace, position) in &mut next {
            let final_bytes = final_sizes.get(namespace).copied().unwrap_or(0);
            if position.legacy {
                let intervals = batch.checked_sub(position.batch).ok_or(
                    OccupancyError::BatchRegression { previous: position.batch, attempted: batch })?;
                let byte_batches = u128::from(position.bytes)
                    .checked_mul(u128::from(intervals))
                    .ok_or(OccupancyError::ArithmeticOverflow)?;
                usage.byte_batches = checked_add(usage.byte_batches, byte_batches)?;
                charges.push(OccupancyCharge {
                    namespace: *namespace,
                    payer: position.payer,
                    root_program: position.root_program,
                    activity_binding: position.activity_binding,
                    from_batch: position.batch,
                    to_batch: batch,
                    recorded_bytes: position.bytes,
                    final_bytes,
                    byte_batches,
                    price: 0,
                    accrued_fee_units: 0,
                    prior_arrears: 0,
                    amount_due: 0,
                    authorized_added_fee_units: 0,
                    disposition: OccupancyDisposition::MigrationRequired,
                    arrears_after: 0,
                    maximum_bytes: position.maximum_bytes.max(final_bytes),
                    maximum_price: 0,
                    remaining_fee_units: 0,
                    mandate: [0; 32],
                });
                position.bytes = final_bytes;
                position.batch = batch;
                position.maximum_bytes = position.maximum_bytes.max(final_bytes);
                position.frozen = true;
                continue;
            }
            if final_bytes > position.maximum_bytes { return Err(OccupancyError::ResponsibilityCeilingExceeded); }
            let intervals = batch.checked_sub(position.batch).ok_or(OccupancyError::BatchRegression { previous: position.batch, attempted: batch })?;
            let byte_batches = u128::from(position.bytes).checked_mul(u128::from(intervals)).ok_or(OccupancyError::ArithmeticOverflow)?;
            let accrued_fee_units = byte_batches.checked_mul(u128::from(price)).ok_or(OccupancyError::ArithmeticOverflow)?;
            let amount_due = position.arrears.checked_add(accrued_fee_units).ok_or(OccupancyError::ArithmeticOverflow)?;
            let disposition = if price > position.maximum_price {
                OccupancyDisposition::ScheduleCeilingExceeded
            } else if amount_due > position.remaining_fee_units {
                OccupancyDisposition::ChargeCeilingExceeded
            } else {
                position.remaining_fee_units -= amount_due;
                OccupancyDisposition::Paid
            };
            usage.byte_batches = checked_add(usage.byte_batches, byte_batches)?;
            usage.fee_units = checked_add(usage.fee_units, accrued_fee_units)?;
            if matches!(disposition, OccupancyDisposition::Paid) {
                usage.paid_fee_units = checked_add(usage.paid_fee_units, amount_due)?;
            } else {
                usage.arrears_fee_units = checked_add(usage.arrears_fee_units, amount_due)?;
            }
            charges.push(OccupancyCharge { namespace: *namespace, payer: position.payer,
                root_program: position.root_program, activity_binding: position.activity_binding,
                from_batch: position.batch,
                to_batch: batch, recorded_bytes: position.bytes, final_bytes, byte_batches, price, accrued_fee_units,
                prior_arrears: position.arrears, amount_due, disposition,
                authorized_added_fee_units: authorized_additions
                    .get(namespace).copied().unwrap_or(0),
                arrears_after: if matches!(disposition, OccupancyDisposition::Paid) { 0 } else { amount_due },
                maximum_bytes: position.maximum_bytes, maximum_price: position.maximum_price,
                remaining_fee_units: position.remaining_fee_units, mandate: position.mandate });
            position.bytes = final_bytes;
            position.batch = batch;
            position.arrears = if matches!(disposition, OccupancyDisposition::Paid) { 0 } else { amount_due };
            position.frozen = !matches!(disposition, OccupancyDisposition::Paid);
        }
        next.retain(|_, position| position.bytes != 0 || position.arrears != 0);
        let settlement = OccupancySettlement { batch, usage, fee_schedule: schedule, charges };
        if settlement.canonical_evidence().len() > MAX_OCCUPANCY_EVIDENCE_BYTES { return Err(OccupancyError::LengthLimit); }
        let prior_state = self.canonical_state();
        if prior_state.len() > MAX_OCCUPANCY_LEDGER_BYTES { return Err(OccupancyError::LengthLimit); }
        Ok(PreparedOccupancySettlement { settlement, prior_state, final_storage_sizes,
            next_positions: next, finalizes_batch: false })
    }

    pub(crate) fn commit_after_debits(&mut self, prepared: PreparedOccupancySettlement, current_storage: &Storage) -> Result<OccupancySettlement, OccupancyError> {
        if self.canonical_state() != prepared.prior_state || canonical_storage_sizes(current_storage)? != prepared.final_storage_sizes {
            return Err(OccupancyError::StalePreparation);
        }
        self.positions = prepared.next_positions;
        if prepared.finalizes_batch { self.last_finalized_batch = prepared.settlement.batch; }
        Ok(prepared.settlement)
    }
    pub(crate) fn commit_unchanged_after_debits(&mut self, prepared: PreparedOccupancySettlement) -> Result<OccupancySettlement, OccupancyError> {
        if self.canonical_state() != prepared.prior_state || canonical_position_sizes(&self.positions)? != prepared.final_storage_sizes {
            return Err(OccupancyError::StalePreparation);
        }
        self.positions = prepared.next_positions;
        if prepared.finalizes_batch { self.last_finalized_batch = prepared.settlement.batch; }
        Ok(prepared.settlement)
    }
    pub fn replay_evidence(&self, evidence: &[u8], final_storage: &Storage,
        responsibilities: impl IntoIterator<Item = OccupancyResponsibility>) -> Result<OccupancySettlement, OccupancyError> {
        let recorded = OccupancySettlement::canonical_decode(evidence)?;
        if evidence.starts_with(EVIDENCE_DOMAIN_V1) ||
            evidence.starts_with(EVIDENCE_DOMAIN_V2) {
            return Ok(recorded);
        }
        let prepared = self.prepare_batch(recorded.batch(), final_storage, responsibilities, recorded.fee_schedule())?;
        if prepared.settlement != recorded { return Err(OccupancyError::MalformedEvidence); }
        Ok(recorded)
    }
    #[must_use]
    pub fn canonical_state(&self) -> Vec<u8> {
        let mut out = LEDGER_DOMAIN.to_vec();
        out.extend_from_slice(&self.last_finalized_batch.to_be_bytes());
        out.extend_from_slice(&(self.positions.len() as u32).to_be_bytes());
        for (namespace, position) in &self.positions {
            encode_namespace(&mut out, *namespace);
            out.extend_from_slice(&position.payer.bytes());
            out.extend_from_slice(&position.root_program.bytes());
            out.extend_from_slice(&position.activity_binding);
            out.extend_from_slice(&position.bytes.to_be_bytes());
            out.extend_from_slice(&position.batch.to_be_bytes());
            out.extend_from_slice(&position.maximum_bytes.to_be_bytes());
            out.extend_from_slice(&position.maximum_price.to_be_bytes());
            out.extend_from_slice(&position.remaining_fee_units.to_be_bytes());
            out.extend_from_slice(&position.mandate);
            out.extend_from_slice(&position.arrears.to_be_bytes());
            out.push(u8::from(position.frozen));
            out.push(u8::from(position.legacy));
        }
        out
    }
    pub fn canonical_decode(encoded: &[u8]) -> Result<Self, OccupancyError> {
        if encoded.len() > MAX_OCCUPANCY_LEDGER_BYTES { return Err(OccupancyError::LengthLimit); }
        if encoded.starts_with(LEDGER_DOMAIN_V1) { return decode_legacy_ledger(encoded); }
        let mut cursor = Cursor::new(encoded);
        if cursor.take(LEDGER_DOMAIN.len())? != LEDGER_DOMAIN { return Err(OccupancyError::MalformedEvidence); }
        let last_finalized_batch = cursor.u64()?;
        let count = usize::try_from(cursor.u32()?).map_err(|_| OccupancyError::MalformedEvidence)?;
        if count > MAX_OCCUPANCY_POSITIONS { return Err(OccupancyError::LengthLimit); }
        let mut positions = BTreeMap::new();
        let mut prior = None;
        for _ in 0..count {
            let namespace = decode_namespace(&mut cursor)?;
            if prior.is_some_and(|value| value >= namespace) { return Err(OccupancyError::MalformedEvidence); }
            prior = Some(namespace);
            let payer = PrincipalId::new(cursor.array()?).map_err(|_| OccupancyError::MalformedEvidence)?;
            validate_scope(namespace, payer)?;
            let root_program = ProgramId::new(cursor.array()?).map_err(|_| OccupancyError::MalformedEvidence)?;
            let activity_binding = cursor.array()?;
            let bytes = cursor.u64()?;
            let batch = cursor.u64()?;
            let maximum_bytes = cursor.u64()?;
            let maximum_price = cursor.u64()?;
            let remaining_fee_units = cursor.u128()?;
            let mandate = cursor.array()?;
            let arrears = cursor.u128()?;
            let frozen = bool_byte(cursor.byte()?)?;
            let legacy = bool_byte(cursor.byte()?)?;
            if bytes > maximum_bytes || (!legacy && (mandate == [0; 32] || activity_binding == [0; 32])) ||
                (!legacy && frozen != (arrears != 0)) || (legacy && (!frozen || arrears != 0)) ||
                (bytes == 0 && arrears == 0) {
                return Err(OccupancyError::MalformedEvidence);
            }
            if positions.insert(namespace, OccupancyPosition { payer, root_program, activity_binding,
                bytes, batch, maximum_bytes, maximum_price,
                remaining_fee_units, mandate, arrears, frozen, legacy }).is_some() { return Err(OccupancyError::MalformedEvidence); }
        }
        if !cursor.is_empty() { return Err(OccupancyError::MalformedEvidence); }
        Ok(Self { last_finalized_batch, positions })
    }
    #[must_use]
    pub fn recorded_bytes(&self, namespace: StorageNamespace) -> Option<u64> { self.positions.get(&namespace).map(|position| position.bytes) }
}

fn checked_add(left: u128, right: u128) -> Result<u128, OccupancyError> { left.checked_add(right).ok_or(OccupancyError::ArithmeticOverflow) }
fn mandate_digest(
    payer: PrincipalId,
    root_program: ProgramId,
    activity_binding: [u8; 32],
    namespace: StorageNamespace,
    maximum_bytes: u64,
    maximum_price: u64,
    charge_ceiling: u128,
) -> [u8; 32] {
    let mut material = MANDATE_DOMAIN.to_vec();
    material.extend_from_slice(&payer.bytes());
    material.extend_from_slice(&root_program.bytes());
    material.extend_from_slice(&activity_binding);
    encode_namespace(&mut material, namespace);
    material.extend_from_slice(&maximum_bytes.to_be_bytes());
    material.extend_from_slice(&maximum_price.to_be_bytes());
    material.extend_from_slice(&charge_ceiling.to_be_bytes());
    Sha256::digest(&material).into()
}
fn validate_scope(namespace: StorageNamespace, payer: PrincipalId) -> Result<(), OccupancyError> {
    if namespace.principal_scope().is_some_and(|principal| principal != payer) { Err(OccupancyError::AuthorityMismatch { namespace }) } else { Ok(()) }
}
fn bool_byte(value: u8) -> Result<bool, OccupancyError> { match value { 0 => Ok(false), 1 => Ok(true), _ => Err(OccupancyError::MalformedEvidence) } }
fn disposition(value: u8) -> Result<OccupancyDisposition, OccupancyError> {
    match value {
        1 => Ok(OccupancyDisposition::Paid),
        2 => Ok(OccupancyDisposition::InsufficientFunds),
        3 => Ok(OccupancyDisposition::ChargeCeilingExceeded),
        4 => Ok(OccupancyDisposition::ScheduleCeilingExceeded),
        5 => Ok(OccupancyDisposition::MigrationRequired),
        _ => Err(OccupancyError::MalformedEvidence),
    }
}
fn encode_namespace(out: &mut Vec<u8>, namespace: StorageNamespace) {
    let bytes = namespace.canonical_bytes();
    out.push(u8::try_from(bytes.len()).unwrap_or(u8::MAX));
    out.extend_from_slice(&bytes);
}
fn decode_namespace(cursor: &mut Cursor<'_>) -> Result<StorageNamespace, OccupancyError> {
    use crate::storage::ProgramId;
    let length = usize::from(cursor.byte()?);
    let bytes = cursor.take(length)?;
    if length != 33 && length != 65 { return Err(OccupancyError::MalformedEvidence); }
    let program = ProgramId::new(bytes[0..32].try_into().map_err(|_| OccupancyError::MalformedEvidence)?).map_err(|_| OccupancyError::MalformedEvidence)?;
    match (bytes[32], length) {
        (0, 65) => Ok(StorageNamespace::principal(program, PrincipalId::new(bytes[33..65].try_into().map_err(|_| OccupancyError::MalformedEvidence)?).map_err(|_| OccupancyError::MalformedEvidence)?)),
        (1, 33) => Ok(StorageNamespace::shared(program)),
        (2, 65) => Ok(StorageNamespace::protocol_private(program,
            bytes[33..65].try_into().map_err(|_| OccupancyError::MalformedEvidence)?)),
        _ => Err(OccupancyError::MalformedEvidence),
    }
}
fn encode_schedule(out: &mut Vec<u8>, schedule: FeeSchedule) {
    out.extend_from_slice(&schedule.version().to_be_bytes());
    for price in [schedule.cpu_price(), schedule.memory_byte_price(), schedule.storage_read_byte_price(), schedule.storage_write_byte_price(),
        schedule.output_value_price(), schedule.output_byte_price(), schedule.occupancy_byte_batch_price()] { out.extend_from_slice(&price.to_be_bytes()); }
}
fn decode_schedule(cursor: &mut Cursor<'_>, versioned: bool) -> Result<FeeSchedule, OccupancyError> {
    let version = if versioned { cursor.u32()? } else { 1 };
    if version == 0 { return Err(OccupancyError::MalformedEvidence); }
    Ok(FeeSchedule::new_complete(version, cursor.u64()?, cursor.u64()?, cursor.u64()?, cursor.u64()?, cursor.u64()?, cursor.u64()?, cursor.u64()?))
}
fn canonical_storage_sizes(storage: &Storage) -> Result<Vec<u8>, OccupancyError> { canonical_sizes(&storage.namespace_sizes()?.into_iter().collect()) }
fn canonical_position_sizes(positions: &BTreeMap<StorageNamespace, OccupancyPosition>) -> Result<Vec<u8>, OccupancyError> {
    let sizes = positions.iter().filter(|(_, position)| position.bytes != 0).map(|(namespace, position)| (*namespace, position.bytes)).collect();
    canonical_sizes(&sizes)
}
fn canonical_sizes(sizes: &BTreeMap<StorageNamespace, u64>) -> Result<Vec<u8>, OccupancyError> {
    if sizes.len() > MAX_OCCUPANCY_POSITIONS { return Err(OccupancyError::LengthLimit); }
    let mut out = Vec::new();
    out.extend_from_slice(&(sizes.len() as u32).to_be_bytes());
    for (namespace, bytes) in sizes { encode_namespace(&mut out, *namespace); out.extend_from_slice(&bytes.to_be_bytes()); }
    Ok(out)
}
fn decode_sizes(encoded: &[u8]) -> Result<BTreeMap<StorageNamespace, u64>, OccupancyError> {
    let mut cursor = Cursor::new(encoded);
    let count = usize::try_from(cursor.u32()?).map_err(|_| OccupancyError::MalformedEvidence)?;
    if count > MAX_OCCUPANCY_POSITIONS { return Err(OccupancyError::LengthLimit); }
    let mut sizes = BTreeMap::new();
    for _ in 0..count {
        let namespace = decode_namespace(&mut cursor)?;
        let bytes = cursor.u64()?;
        if bytes == 0 || sizes.insert(namespace, bytes).is_some() { return Err(OccupancyError::MalformedEvidence); }
    }
    if !cursor.is_empty() { return Err(OccupancyError::MalformedEvidence); }
    Ok(sizes)
}
fn decode_legacy_ledger(encoded: &[u8]) -> Result<OccupancyLedger, OccupancyError> {
    let mut cursor = Cursor::new(encoded);
    let _ = cursor.take(LEDGER_DOMAIN_V1.len())?;
    let count = usize::try_from(cursor.u64()?).map_err(|_| OccupancyError::MalformedEvidence)?;
    if count > MAX_OCCUPANCY_POSITIONS { return Err(OccupancyError::LengthLimit); }
    let mut positions = BTreeMap::new();
    let mut last_finalized_batch = 0;
    for _ in 0..count {
        let namespace = decode_namespace(&mut cursor)?;
        let payer = PrincipalId::new(cursor.array()?).map_err(|_| OccupancyError::MalformedEvidence)?;
        validate_scope(namespace, payer)?;
        let bytes = cursor.u64()?;
        let batch = cursor.u64()?;
        last_finalized_batch = last_finalized_batch.max(batch);
        if bytes == 0 || positions.insert(namespace, OccupancyPosition { payer,
            root_program: namespace.program(), activity_binding: [0; 32], bytes, batch, maximum_bytes: bytes,
            maximum_price: 0, remaining_fee_units: 0, mandate: [0; 32], arrears: 0,
            frozen: true, legacy: true }).is_some() {
            return Err(OccupancyError::MalformedEvidence);
        }
    }
    if !cursor.is_empty() { return Err(OccupancyError::MalformedEvidence); }
    for position in positions.values_mut() { position.batch = last_finalized_batch; }
    Ok(OccupancyLedger { last_finalized_batch, positions })
}
fn decode_legacy_settlement(encoded: &[u8]) -> Result<OccupancySettlement, OccupancyError> {
    let versioned = encoded.starts_with(EVIDENCE_DOMAIN_V2);
    let domain = if versioned { EVIDENCE_DOMAIN_V2 } else { EVIDENCE_DOMAIN_V1 };
    let mut cursor = Cursor::new(encoded);
    let _ = cursor.take(domain.len())?;
    let batch = cursor.u64()?;
    let fee_schedule = decode_schedule(&mut cursor, versioned)?;
    let declared_units = cursor.u128()?;
    let declared_fee = cursor.u128()?;
    let count = usize::try_from(cursor.u64()?).map_err(|_| OccupancyError::MalformedEvidence)?;
    if count > MAX_OCCUPANCY_POSITIONS { return Err(OccupancyError::LengthLimit); }
    let mut usage = OccupancyUsage::default();
    let mut charges = Vec::with_capacity(count);
    for _ in 0..count {
        let namespace = decode_namespace(&mut cursor)?;
        let payer = PrincipalId::new(cursor.array()?).map_err(|_| OccupancyError::MalformedEvidence)?;
        let from_batch = cursor.u64()?;
        let to_batch = cursor.u64()?;
        let recorded_bytes = cursor.u64()?;
        let final_bytes = cursor.u64()?;
        let byte_batches = cursor.u128()?;
        let price = cursor.u64()?;
        let accrued_fee_units = cursor.u128()?;
        let intervals = to_batch.checked_sub(from_batch).ok_or(OccupancyError::MalformedEvidence)?;
        let expected_units = u128::from(recorded_bytes).checked_mul(u128::from(intervals)).ok_or(OccupancyError::ArithmeticOverflow)?;
        if to_batch != batch || byte_batches != expected_units || price != fee_schedule.occupancy_byte_batch_price()
            || accrued_fee_units != byte_batches.checked_mul(u128::from(price)).ok_or(OccupancyError::ArithmeticOverflow)? {
            return Err(OccupancyError::MalformedEvidence);
        }
        usage.byte_batches = checked_add(usage.byte_batches, byte_batches)?;
        usage.fee_units = checked_add(usage.fee_units, accrued_fee_units)?;
        usage.paid_fee_units = checked_add(usage.paid_fee_units, accrued_fee_units)?;
        charges.push(OccupancyCharge { namespace, payer, root_program: namespace.program(),
            activity_binding: [0; 32], from_batch, to_batch, recorded_bytes, final_bytes, byte_batches,
            price, accrued_fee_units, prior_arrears: 0, amount_due: accrued_fee_units,
            authorized_added_fee_units: 0,
            disposition: OccupancyDisposition::Paid, arrears_after: 0,
            maximum_bytes: final_bytes.max(recorded_bytes), maximum_price: price,
            remaining_fee_units: 0, mandate: [0; 32] });
    }
    if !cursor.is_empty() || usage.byte_batches != declared_units || usage.fee_units != declared_fee { return Err(OccupancyError::MalformedEvidence); }
    Ok(OccupancySettlement { batch, usage, fee_schedule, charges })
}

struct Cursor<'a> { remaining: &'a [u8] }
impl<'a> Cursor<'a> {
    const fn new(remaining: &'a [u8]) -> Self { Self { remaining } }
    fn take(&mut self, length: usize) -> Result<&'a [u8], OccupancyError> {
        let (value, rest) = self.remaining.split_at_checked(length).ok_or(OccupancyError::MalformedEvidence)?;
        self.remaining = rest; Ok(value)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], OccupancyError> { self.take(N)?.try_into().map_err(|_| OccupancyError::MalformedEvidence) }
    fn byte(&mut self) -> Result<u8, OccupancyError> { Ok(self.take(1)?[0]) }
    fn u32(&mut self) -> Result<u32, OccupancyError> { Ok(u32::from_be_bytes(self.array()?)) }
    fn u64(&mut self) -> Result<u64, OccupancyError> { Ok(u64::from_be_bytes(self.array()?)) }
    fn u128(&mut self) -> Result<u128, OccupancyError> { Ok(u128::from_be_bytes(self.array()?)) }
    const fn is_empty(&self) -> bool { self.remaining.is_empty() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActivityBudgetBinding, ResourceBudget};

    fn principal(value: u8) -> PrincipalId {
        PrincipalId::new([value; 32]).expect("nonzero principal")
    }

    fn namespace(payer: PrincipalId) -> StorageNamespace {
        StorageNamespace::principal(
            crate::ProgramId::new([7; 32]).expect("nonzero program"),
            payer,
        )
    }

    const fn schedule(version: u32, occupancy_price: u64) -> FeeSchedule {
        FeeSchedule::new_complete(version, 0, 0, 0, 0, 0, 0, occupancy_price)
    }

    fn authority(payer: PrincipalId, ceiling: u128) -> OccupancyAuthority {
        let schedule = schedule(1, 2);
        let admitted = AdmittedBudget::new(
            ResourceBudget::new_complete(3, 65_536, 0, 0, 1, 0, 0),
            payer,
            ActivityBudgetBinding::new([9; 32]).expect("nonzero activity"),
            0,
            schedule,
            ResourceBudget::declared(),
        );
        OccupancyAuthority::from_admitted(
            &admitted,
            ceiling,
            schedule,
            crate::ProgramId::new([7; 32]).expect("nonzero program"),
        )
            .expect("admitted authority")
    }

    fn occupied(namespace: StorageNamespace) -> Storage {
        occupied_bytes(namespace, 10)
    }

    fn occupied_bytes(namespace: StorageNamespace, bytes: u64) -> Storage {
        assert!(bytes > 1);
        let mut storage = Storage::new();
        let mut transaction = storage.transaction(namespace);
        transaction
            .write(
                b"k",
                &vec![1; usize::try_from(bytes - 1).expect("bounded bytes")],
            )
            .expect("bounded write");
        assert_eq!(transaction.commit(), 1);
        storage
    }

    fn initialized(ceiling: u128) -> (OccupancyLedger, Storage, StorageNamespace) {
        let payer = principal(3);
        let namespace = namespace(payer);
        let storage = occupied(namespace);
        let responsibility = authority(payer, ceiling)
            .authorize(namespace, 10, ceiling)
            .expect("signed occupancy mandate");
        let mut ledger = OccupancyLedger::new();
        let prepared = ledger
            .prepare_batch(1, &storage, [responsibility], schedule(1, 2))
            .expect("initial position");
        ledger
            .commit_after_debits(prepared, &storage)
            .expect("initial position commit");
        let prepared = ledger
            .prepare_unchanged_batch(1, schedule(1, 2))
            .expect("first terminal transition");
        ledger
            .commit_unchanged_after_debits(prepared)
            .expect("first terminal commit");
        (ledger, storage, namespace)
    }

    fn initialized_bytes(
        bytes: u64,
        ceiling: u128,
    ) -> (OccupancyLedger, Storage, StorageNamespace) {
        let payer = principal(3);
        let namespace = namespace(payer);
        let storage = occupied_bytes(namespace, bytes);
        let responsibility = authority(payer, ceiling)
            .authorize(namespace, bytes, ceiling)
            .expect("signed occupancy mandate");
        let mut ledger = OccupancyLedger::new();
        let prepared = ledger
            .prepare_batch(1, &storage, [responsibility], schedule(1, 2))
            .expect("initial position");
        ledger
            .commit_after_debits(prepared, &storage)
            .expect("initial position commit");
        let prepared = ledger
            .prepare_unchanged_batch(1, schedule(1, 2))
            .expect("first terminal transition");
        ledger
            .commit_unchanged_after_debits(prepared)
            .expect("first terminal commit");
        (ledger, storage, namespace)
    }

    #[test]
    fn lifetime_ceiling_exhaustion_freezes_only_its_position() {
        let (mut ledger, _storage, namespace) = initialized(20);
        let paid = ledger
            .prepare_unchanged_batch(2, schedule(1, 2))
            .expect("contiguous second batch");
        assert_eq!(paid.settlement().usage().paid_fee_units, 20);
        ledger
            .commit_unchanged_after_debits(paid)
            .expect("paid terminal commit");
        let exhausted = ledger
            .prepare_unchanged_batch(3, schedule(1, 2))
            .expect("ceiling exhaustion is a disposition");
        assert_eq!(
            exhausted.settlement().charges()[0].disposition(),
            OccupancyDisposition::ChargeCeilingExceeded
        );
        assert_eq!(exhausted.settlement().usage().arrears_fee_units, 20);
        ledger
            .commit_unchanged_after_debits(exhausted)
            .expect("frozen terminal commit");
        assert_eq!(
            ledger.ensure_accessible([namespace]),
            Err(OccupancyError::FrozenNamespace { namespace })
        );
        assert_eq!(
            OccupancyLedger::canonical_decode(&ledger.canonical_state()),
            Ok(ledger)
        );
    }

    #[test]
    fn insufficient_funds_and_schedule_cap_are_nonfatal_dispositions() {
        let (mut insolvent, _, _) = initialized(100);
        let mut prepared = insolvent
            .prepare_unchanged_batch(2, schedule(1, 2))
            .expect("contiguous settlement");
        prepared
            .defer_unpaid(&BTreeSet::from([principal(3)]))
            .expect("typed insolvency");
        assert_eq!(
            prepared.settlement().charges()[0].disposition(),
            OccupancyDisposition::InsufficientFunds
        );
        insolvent
            .commit_unchanged_after_debits(prepared)
            .expect("insolvent position does not halt the batch");

        let (mut repriced, _, _) = initialized(100);
        let prepared = repriced
            .prepare_unchanged_batch(2, schedule(2, 3))
            .expect("versioned schedule transition");
        assert_eq!(
            prepared.settlement().charges()[0].disposition(),
            OccupancyDisposition::ScheduleCeilingExceeded
        );
        repriced
            .commit_unchanged_after_debits(prepared)
            .expect("repriced position does not halt the batch");
    }

    #[test]
    fn multiple_payers_settle_atomically_with_isolated_arrears() {
        let first = principal(3);
        let second = principal(4);
        let first_namespace = namespace(first);
        let second_namespace = StorageNamespace::principal(
            crate::ProgramId::new([8; 32]).expect("nonzero program"),
            second,
        );
        let mut storage = occupied(first_namespace);
        let mut transaction = storage.transaction(second_namespace);
        transaction.write(b"k", &[2; 9]).expect("bounded write");
        assert_eq!(transaction.commit(), 1);
        let responsibilities = [
            authority(first, 100)
                .authorize(first_namespace, 10, 100)
                .expect("first mandate"),
            authority(second, 100)
                .authorize(second_namespace, 10, 100)
                .expect("second mandate"),
        ];
        let mut ledger = OccupancyLedger::new();
        let prepared = ledger
            .prepare_batch(1, &storage, responsibilities, schedule(1, 2))
            .expect("multi-payer initialization");
        ledger
            .commit_after_debits(prepared, &storage)
            .expect("multi-payer initialization commit");
        let first_batch = ledger
            .prepare_unchanged_batch(1, schedule(1, 2))
            .expect("initial terminal");
        ledger
            .commit_unchanged_after_debits(first_batch)
            .expect("initial terminal commit");
        let mut second_batch = ledger
            .prepare_unchanged_batch(2, schedule(1, 2))
            .expect("multi-payer settlement");
        second_batch
            .defer_unpaid(&BTreeSet::from([second]))
            .expect("one payer insolvent");
        let dispositions = second_batch
            .settlement()
            .payer_dispositions()
            .expect("bounded payer totals");
        assert_eq!(dispositions[&first], (20, 20, 0, false));
        assert_eq!(dispositions[&second], (20, 0, 20, true));
        ledger
            .commit_unchanged_after_debits(second_batch)
            .expect("one insolvent payer cannot halt the batch");
        assert!(ledger.ensure_accessible([first_namespace]).is_ok());
        assert_eq!(
            ledger.ensure_accessible([second_namespace]),
            Err(OccupancyError::FrozenNamespace {
                namespace: second_namespace,
            })
        );
    }

    #[test]
    fn gaps_refuse_and_committed_drop_stops_future_accrual() {
        let (mut ledger, _storage, namespace) = initialized(100);
        assert_eq!(
            ledger.prepare_unchanged_batch(3, schedule(1, 2)).unwrap_err(),
            OccupancyError::BatchRegression {
                previous: 1,
                attempted: 3,
            }
        );
        let empty = Storage::new();
        let prepared = ledger
            .prepare_batch(2, &empty, [], schedule(1, 2))
            .expect("drop settlement");
        assert_eq!(prepared.settlement().charges()[0].final_bytes, 0);
        assert_eq!(prepared.settlement().charges()[0].fee_units(), 20);
        let evidence = prepared.settlement().canonical_evidence();
        assert_eq!(
            OccupancySettlement::canonical_decode(&evidence),
            Ok(prepared.settlement().clone())
        );
        ledger
            .commit_after_debits(prepared, &empty)
            .expect("drop commit");
        let terminal = ledger
            .prepare_unchanged_batch(2, schedule(1, 2))
            .expect("same-batch terminal transition");
        ledger
            .commit_unchanged_after_debits(terminal)
            .expect("same-batch terminal commit");
        assert_eq!(ledger.recorded_bytes(namespace), None);
    }

    #[test]
    fn property_usage_is_monotone_in_bytes_and_contiguous_batches() {
        let mut prior_first_interval_fee = 0u128;
        for bytes in 2u64..=64 {
            let ceiling = u128::from(bytes) * 64;
            let (mut ledger, _, _) = initialized_bytes(bytes, ceiling);
            let mut cumulative_units = 0u128;
            let mut prior_cumulative_units = 0u128;
            for batch in 2u64..=16 {
                let prepared = ledger
                    .prepare_unchanged_batch(batch, schedule(1, 2))
                    .expect("contiguous property settlement");
                let usage = prepared.settlement().usage();
                assert_eq!(usage.byte_batches, u128::from(bytes));
                assert_eq!(usage.fee_units, u128::from(bytes) * 2);
                cumulative_units = cumulative_units
                    .checked_add(usage.fee_units)
                    .expect("bounded matrix");
                assert!(cumulative_units > prior_cumulative_units);
                prior_cumulative_units = cumulative_units;
                if batch == 2 {
                    assert!(usage.fee_units > prior_first_interval_fee);
                    prior_first_interval_fee = usage.fee_units;
                }
                ledger
                    .commit_unchanged_after_debits(prepared)
                    .expect("property terminal commit");
            }
        }
    }

    #[test]
    fn property_drop_charges_through_commit_and_never_after() {
        for bytes in 2u64..=32 {
            for drop_batch in 2u64..=8 {
                let ceiling = u128::from(bytes) * 64;
                let (mut ledger, _, namespace) = initialized_bytes(bytes, ceiling);
                for batch in 2..drop_batch {
                    let prepared = ledger
                        .prepare_unchanged_batch(batch, schedule(1, 2))
                        .expect("pre-drop interval");
                    ledger
                        .commit_unchanged_after_debits(prepared)
                        .expect("pre-drop commit");
                }
                let empty = Storage::new();
                let dropped = ledger
                    .prepare_batch(drop_batch, &empty, [], schedule(1, 2))
                    .expect("drop interval");
                assert_eq!(dropped.settlement().charges().len(), 1);
                assert_eq!(
                    dropped.settlement().charges()[0].byte_batches(),
                    u128::from(bytes)
                );
                assert_eq!(dropped.settlement().charges()[0].final_bytes, 0);
                ledger
                    .commit_after_debits(dropped, &empty)
                    .expect("drop state commit");
                let terminal = ledger
                    .prepare_unchanged_batch(drop_batch, schedule(1, 2))
                    .expect("drop terminal");
                ledger
                    .commit_unchanged_after_debits(terminal)
                    .expect("drop terminal commit");
                assert_eq!(ledger.recorded_bytes(namespace), None);
                let after = ledger
                    .prepare_unchanged_batch(drop_batch + 1, schedule(1, 2))
                    .expect("post-drop interval");
                assert_eq!(after.settlement().usage(), OccupancyUsage::default());
            }
        }
    }
}
