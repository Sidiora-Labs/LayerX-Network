//! Deterministic, receipt-bound storage occupancy accounting.

use core::fmt::{self, Display};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
#[cfg(any(feature = "host-ffi", test))]
use std::collections::BTreeSet;

#[cfg(any(feature = "host-ffi", test))]
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
pub const MAX_OCCUPANCY_PAYERS: usize = 256;
const STATE_COMMITMENT_PROTOCOL_VERSION: u16 = 3;
const MAX_PAYER_DID_BYTES: usize = 255;
const ACCOUNT_IDENTIFIER_DOMAIN: &[u8] = b"LX:ACCOUNT:v1";
const DID_IDENTIFIER_DOMAIN: &[u8] = b"LXP/v1/did-id\0";
const TRANSFER_LEAF_DOMAIN: &[u8] = b"LXP/v1/merkle-leaf\0";
const TRANSFER_INTERNAL_DOMAIN: &[u8] = b"LXP/v1/merkle-internal\0";
pub const MAX_OCCUPANCY_LEDGER_BYTES: usize = 60_000;
pub const MAX_OCCUPANCY_EVIDENCE_BYTES: usize = 65_536;

#[cfg(any(feature = "host-ffi", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OccupancyAuthority {
    payer: PrincipalId,
    root_program: ProgramId,
    activity_binding: [u8; 32],
    occupancy_fee_ceiling: u128,
    maximum_price: u64,
}

#[cfg(any(feature = "host-ffi", test))]
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

    #[cfg(feature = "host-ffi")]
    pub(crate) const fn fee_ceiling(self) -> u128 {
        self.occupancy_fee_ceiling
    }
    #[cfg(feature = "host-ffi")]
    pub(crate) const fn payer(self) -> PrincipalId {
        self.payer
    }
    #[cfg(feature = "host-ffi")]
    pub(crate) const fn root_program(self) -> ProgramId {
        self.root_program
    }
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
    pub const fn namespace(self) -> StorageNamespace {
        self.namespace
    }
    #[must_use]
    pub const fn payer(self) -> PrincipalId {
        self.payer
    }
    #[must_use]
    pub const fn root_program(self) -> ProgramId {
        self.root_program
    }
    #[must_use]
    pub const fn activity_binding(self) -> [u8; 32] {
        self.activity_binding
    }
    #[must_use]
    pub const fn maximum_bytes(self) -> u64 {
        self.maximum_bytes
    }
    #[must_use]
    pub const fn maximum_price(self) -> u64 {
        self.maximum_price
    }
    #[must_use]
    pub const fn charge_ceiling(self) -> u128 {
        self.charge_ceiling
    }
    #[must_use]
    pub const fn mandate(self) -> [u8; 32] {
        self.mandate
    }
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
    pub const fn namespace(self) -> StorageNamespace {
        self.namespace
    }
    #[must_use]
    pub const fn payer(self) -> PrincipalId {
        self.payer
    }
    #[must_use]
    pub const fn root_program(self) -> ProgramId {
        self.root_program
    }
    #[must_use]
    pub const fn activity_binding(self) -> [u8; 32] {
        self.activity_binding
    }
    #[must_use]
    pub const fn start_batch(self) -> u64 {
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
        self.accrued_fee_units
    }
    #[must_use]
    pub const fn prior_arrears(self) -> u128 {
        self.prior_arrears
    }
    #[must_use]
    pub const fn amount_due(self) -> u128 {
        self.amount_due
    }
    #[must_use]
    pub const fn paid(self) -> bool {
        matches!(self.disposition, OccupancyDisposition::Paid)
    }
    #[must_use]
    pub const fn disposition(self) -> OccupancyDisposition {
        self.disposition
    }
    #[must_use]
    pub const fn arrears_after(self) -> u128 {
        self.arrears_after
    }
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

type PayerDispositions = BTreeMap<PrincipalId, (u128, u128, u128, bool)>;

fn read_current_charge(
    cursor: &mut Cursor<'_>,
    namespace: StorageNamespace,
) -> Result<OccupancyCharge, OccupancyError> {
    let payer = PrincipalId::new(cursor.array()?).map_err(|_| OccupancyError::MalformedEvidence)?;
    validate_scope(namespace, payer)?;
    let root_program =
        ProgramId::new(cursor.array()?).map_err(|_| OccupancyError::MalformedEvidence)?;
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
    Ok(OccupancyCharge {
        namespace,
        payer,
        root_program,
        activity_binding,
        from_batch,
        to_batch,
        recorded_bytes,
        final_bytes,
        byte_batches,
        price,
        accrued_fee_units,
        prior_arrears,
        amount_due,
        authorized_added_fee_units,
        disposition,
        arrears_after,
        maximum_bytes,
        maximum_price,
        remaining_fee_units,
        mandate,
    })
}

fn validate_current_charge(
    charge: &OccupancyCharge,
    batch: u64,
    fee_schedule: FeeSchedule,
) -> Result<(), OccupancyError> {
    let OccupancyCharge {
        namespace,
        payer,
        root_program,
        activity_binding,
        from_batch,
        to_batch,
        recorded_bytes,
        final_bytes,
        byte_batches,
        price,
        accrued_fee_units,
        prior_arrears,
        amount_due,
        authorized_added_fee_units,
        disposition,
        arrears_after,
        maximum_bytes,
        maximum_price,
        mandate,
        ..
    } = *charge;
    let intervals = to_batch
        .checked_sub(from_batch)
        .ok_or(OccupancyError::MalformedEvidence)?;
    let computed_units = u128::from(recorded_bytes)
        .checked_mul(u128::from(intervals))
        .ok_or(OccupancyError::ArithmeticOverflow)?;
    let computed_fee = computed_units
        .checked_mul(u128::from(price))
        .ok_or(OccupancyError::ArithmeticOverflow)?;
    let computed_due = prior_arrears
        .checked_add(computed_fee)
        .ok_or(OccupancyError::ArithmeticOverflow)?;
    let migration = matches!(disposition, OccupancyDisposition::MigrationRequired);
    if to_batch != batch
        || (!migration && price != fee_schedule.occupancy_byte_batch_price())
        || byte_batches != computed_units
        || accrued_fee_units != computed_fee
        || amount_due != computed_due
        || final_bytes > maximum_bytes
        || (!migration && (mandate == [0; 32] || activity_binding == [0; 32]))
        || (migration
            && (price != 0
                || accrued_fee_units != 0
                || prior_arrears != 0
                || amount_due != 0
                || arrears_after != 0
                || mandate != [0; 32]
                || activity_binding != [0; 32]
                || root_program != namespace.program()))
        || (authorized_added_fee_units != 0
            && mandate
                != mandate_digest(
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
    {
        return Err(OccupancyError::MalformedEvidence);
    }
    Ok(())
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

    ///
    /// # Errors
    ///
    /// Returns a refusal for a zero asset identity or overflowing payer dispositions.
    pub fn transfer_root(&self, asset: [u8; 32]) -> Result<[u8; 32], OccupancyError> {
        if asset == [0; 32] {
            return Err(OccupancyError::MalformedEvidence);
        }
        let treasury = account_identifier(b"system:fees")?;
        let mut level = Vec::new();
        for (payer, (_, paid, _, _)) in self.payer_dispositions()? {
            if paid == 0 {
                continue;
            }
            level.push(transfer_leaf(payer.bytes(), treasury, asset, paid));
        }
        Ok(transfer_merkle_root(level))
    }

    /// Checks the receipt-committed occupancy transfer root for the receipt's
    /// protocol version and returns the payment accounts that root commits.
    ///
    /// Before the state-commitment protocol an account identifier equals its
    /// principal, so the root is rebuilt from the evidence alone. Under the
    /// state-commitment protocol the kernel debits the payer's asset payment
    /// account, so every paying payer needs an account proven from its DID.
    ///
    /// # Errors
    ///
    /// Refuses a zero asset, a paying payer without a proven payment account,
    /// more candidate roots than the payer bound, and a root that no proven
    /// account selection reproduces.
    pub fn verify_transfer_root(
        &self,
        protocol_version: u16,
        asset: [u8; 32],
        accounts: &[OccupancyPaymentAccount],
        committed_root: [u8; 32],
    ) -> Result<Vec<OccupancyPaymentAccount>, OccupancyError> {
        if protocol_version != STATE_COMMITMENT_PROTOCOL_VERSION {
            return if self.transfer_root(asset)? == committed_root {
                Ok(Vec::new())
            } else {
                Err(OccupancyError::TransferRootMismatch)
            };
        }
        if asset == [0; 32] {
            return Err(OccupancyError::MalformedEvidence);
        }
        if accounts.len() > 2 * MAX_OCCUPANCY_PAYERS {
            return Err(OccupancyError::LengthLimit);
        }
        let mut payers = Vec::new();
        let mut selections = 1_usize;
        for (payer, (_, paid, _, _)) in self.payer_dispositions()? {
            if paid == 0 {
                continue;
            }
            let mut proven: Vec<OccupancyPaymentAccount> = Vec::new();
            for account in accounts {
                if account.payer == payer
                    && account.asset == asset
                    && !proven.iter().any(|known| known.account == account.account)
                {
                    proven.push(*account);
                }
            }
            if proven.is_empty() {
                return Err(OccupancyError::UnprovenPaymentAccount);
            }
            selections = selections
                .checked_mul(proven.len())
                .filter(|count| *count <= MAX_OCCUPANCY_PAYERS)
                .ok_or(OccupancyError::LengthLimit)?;
            payers.push((paid, proven));
        }
        if payers.len() > MAX_OCCUPANCY_PAYERS {
            return Err(OccupancyError::LengthLimit);
        }
        let treasury = account_identifier(b"system:fees")?;
        for selection in 0..selections {
            let mut remaining = selection;
            let mut chosen = Vec::with_capacity(payers.len());
            let mut level = Vec::with_capacity(payers.len());
            for (paid, proven) in &payers {
                let account = proven[remaining % proven.len()];
                remaining /= proven.len();
                level.push(transfer_leaf(account.account, treasury, asset, *paid));
                chosen.push(account);
            }
            if transfer_merkle_root(level) == committed_root {
                return Ok(chosen);
            }
        }
        Err(OccupancyError::TransferRootMismatch)
    }

    ///
    /// # Errors
    ///
    /// Returns an arithmetic refusal if aggregated payer amounts overflow.
    pub fn payer_dispositions(&self) -> Result<PayerDispositions, OccupancyError> {
        let mut payers = BTreeMap::new();
        for charge in &self.charges {
            let entry = payers.entry(charge.payer).or_insert((0, 0, 0, false));
            entry.0 = checked_add(entry.0, charge.amount_due)?;
            if charge.paid() {
                entry.1 = checked_add(entry.1, charge.amount_due)?;
            }
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
        out.extend_from_slice(&position_count_bytes(self.charges.len()));
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

    ///
    /// # Errors
    ///
    /// Returns a refusal for malformed, noncanonical, or inconsistent settlement evidence.
    pub fn canonical_decode(encoded: &[u8]) -> Result<Self, OccupancyError> {
        if encoded.len() > MAX_OCCUPANCY_EVIDENCE_BYTES {
            return Err(OccupancyError::LengthLimit);
        }
        if encoded.starts_with(EVIDENCE_DOMAIN_V1) || encoded.starts_with(EVIDENCE_DOMAIN_V2) {
            return decode_legacy_settlement(encoded);
        }
        let mut cursor = Cursor::new(encoded);
        if cursor.take(EVIDENCE_DOMAIN.len())? != EVIDENCE_DOMAIN {
            return Err(OccupancyError::MalformedEvidence);
        }
        let batch = cursor.u64()?;
        let fee_schedule = decode_schedule(&mut cursor, true)?;
        let declared_units = cursor.u128()?;
        let declared_accrued = cursor.u128()?;
        let declared_paid = cursor.u128()?;
        let declared_arrears = cursor.u128()?;
        let count =
            usize::try_from(cursor.u32()?).map_err(|_| OccupancyError::MalformedEvidence)?;
        if count > MAX_OCCUPANCY_POSITIONS {
            return Err(OccupancyError::LengthLimit);
        }
        let mut charges = Vec::with_capacity(count);
        let mut prior = None;
        let mut usage = OccupancyUsage::default();
        for _ in 0..count {
            let namespace = decode_namespace(&mut cursor)?;
            if prior.is_some_and(|value| value >= namespace) {
                return Err(OccupancyError::MalformedEvidence);
            }
            prior = Some(namespace);
            let charge = read_current_charge(&mut cursor, namespace)?;
            validate_current_charge(&charge, batch, fee_schedule)?;
            usage.byte_batches = checked_add(usage.byte_batches, charge.byte_batches)?;
            usage.fee_units = checked_add(usage.fee_units, charge.accrued_fee_units)?;
            if matches!(charge.disposition, OccupancyDisposition::Paid) {
                usage.paid_fee_units = checked_add(usage.paid_fee_units, charge.amount_due)?;
            } else {
                usage.arrears_fee_units =
                    checked_add(usage.arrears_fee_units, charge.arrears_after)?;
            }
            charges.push(charge);
        }
        if !cursor.is_empty()
            || usage.byte_batches != declared_units
            || usage.fee_units != declared_accrued
            || usage.paid_fee_units != declared_paid
            || usage.arrears_fee_units != declared_arrears
        {
            return Err(OccupancyError::MalformedEvidence);
        }
        Ok(Self {
            batch,
            usage,
            fee_schedule,
            charges,
        })
    }
}

/// A payer's state-commitment payment account, proven from the payer's DID.
///
/// The kernel debits the unique `agent:<did>:main` or
/// `agent:<did>:asset:<asset>` account owned by the payer's DID that holds the
/// occupancy asset. A value of this type exists only for an account identifier
/// that is one of those two derivations, bound to the DID identifier the
/// settlement evidence names as the payer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OccupancyPaymentAccount {
    payer: PrincipalId,
    asset: [u8; 32],
    account: [u8; 32],
}

impl OccupancyPaymentAccount {
    /// Derives the DID's main account as the payment account for `asset`.
    ///
    /// # Errors
    ///
    /// Refuses an empty or over-long DID and a zero asset identity.
    pub fn main(did: &[u8], asset: [u8; 32]) -> Result<Self, OccupancyError> {
        let payer = payer_principal(did, asset)?;
        let mut name = Vec::with_capacity(11 + did.len());
        name.extend_from_slice(b"agent:");
        name.extend_from_slice(did);
        name.extend_from_slice(b":main");
        Ok(Self {
            payer,
            asset,
            account: account_identifier(&name)?,
        })
    }

    /// Derives the DID's asset account as the payment account for `asset`.
    ///
    /// # Errors
    ///
    /// Refuses an empty or over-long DID and a zero asset identity.
    pub fn asset(did: &[u8], asset: [u8; 32]) -> Result<Self, OccupancyError> {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let payer = payer_principal(did, asset)?;
        let mut name = Vec::with_capacity(77 + did.len());
        name.extend_from_slice(b"agent:");
        name.extend_from_slice(did);
        name.extend_from_slice(b":asset:");
        for byte in asset {
            name.push(HEX[usize::from(byte >> 4)]);
            name.push(HEX[usize::from(byte & 15)]);
        }
        Ok(Self {
            payer,
            asset,
            account: account_identifier(&name)?,
        })
    }

    /// Proves a supplied account identifier is one of the two payment
    /// accounts derivable from the DID and asset.
    ///
    /// # Errors
    ///
    /// Refuses every identifier that neither derivation produces.
    pub fn prove(did: &[u8], asset: [u8; 32], account: [u8; 32]) -> Result<Self, OccupancyError> {
        [Self::main(did, asset)?, Self::asset(did, asset)?]
            .into_iter()
            .find(|candidate| candidate.account == account)
            .ok_or(OccupancyError::UnprovenPaymentAccount)
    }

    #[must_use]
    pub const fn payer(&self) -> PrincipalId {
        self.payer
    }
    #[must_use]
    pub const fn asset_id(&self) -> [u8; 32] {
        self.asset
    }
    #[must_use]
    pub const fn account(&self) -> [u8; 32] {
        self.account
    }
}

fn payer_principal(did: &[u8], asset: [u8; 32]) -> Result<PrincipalId, OccupancyError> {
    if did.is_empty() || did.len() > MAX_PAYER_DID_BYTES || asset == [0; 32] {
        return Err(OccupancyError::UnprovenPaymentAccount);
    }
    let length = u16::try_from(did.len()).map_err(|_| OccupancyError::LengthLimit)?;
    let mut preimage = DID_IDENTIFIER_DOMAIN.to_vec();
    preimage.extend_from_slice(&length.to_be_bytes());
    preimage.extend_from_slice(did);
    PrincipalId::new(Sha256::digest(preimage).into())
        .map_err(|_| OccupancyError::UnprovenPaymentAccount)
}

fn account_identifier(name: &[u8]) -> Result<[u8; 32], OccupancyError> {
    let length = u32::try_from(name.len()).map_err(|_| OccupancyError::LengthLimit)?;
    let mut preimage = ACCOUNT_IDENTIFIER_DOMAIN.to_vec();
    preimage.extend_from_slice(&length.to_be_bytes());
    preimage.extend_from_slice(name);
    Ok(Sha256::digest(preimage).into())
}

fn transfer_leaf(from: [u8; 32], treasury: [u8; 32], asset: [u8; 32], paid: u128) -> [u8; 32] {
    let mut leaf = TRANSFER_LEAF_DOMAIN.to_vec();
    leaf.push(0);
    leaf.extend_from_slice(&from);
    leaf.extend_from_slice(&treasury);
    leaf.extend_from_slice(&asset);
    leaf.extend_from_slice(&paid.to_be_bytes());
    leaf.extend_from_slice(&23_u16.to_be_bytes());
    Sha256::digest(leaf).into()
}

fn transfer_merkle_root(mut level: Vec<[u8; 32]>) -> [u8; 32] {
    if level.is_empty() {
        return [0; 32];
    }
    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        for pair in level.chunks(2) {
            let right = pair.get(1).unwrap_or(&pair[0]);
            let mut preimage = TRANSFER_INTERNAL_DOMAIN.to_vec();
            preimage.extend_from_slice(&pair[0]);
            preimage.extend_from_slice(right);
            next.push(<[u8; 32]>::from(Sha256::digest(preimage)));
        }
        level = next;
    }
    level[0]
}

#[derive(Clone, Debug)]
pub struct PreparedOccupancySettlement {
    settlement: OccupancySettlement,
    #[cfg(any(feature = "host-ffi", test))]
    prior_state: Vec<u8>,
    #[cfg(any(feature = "host-ffi", test))]
    final_storage_sizes: Vec<u8>,
    #[cfg(any(feature = "host-ffi", test))]
    next_positions: BTreeMap<StorageNamespace, OccupancyPosition>,
    finalizes_batch: bool,
}

impl PreparedOccupancySettlement {
    #[must_use]
    pub const fn settlement(&self) -> &OccupancySettlement {
        &self.settlement
    }

    #[cfg(any(feature = "host-ffi", test))]
    pub(crate) fn defer_unpaid(
        &mut self,
        unpaid: &BTreeSet<PrincipalId>,
    ) -> Result<(), OccupancyError> {
        self.settlement.usage.paid_fee_units = 0;
        self.settlement.usage.arrears_fee_units = 0;
        for charge in &mut self.settlement.charges {
            let position = self
                .next_positions
                .get_mut(&charge.namespace)
                .ok_or(OccupancyError::StalePreparation)?;
            if charge.amount_due != 0 && charge.paid() && unpaid.contains(&charge.payer) {
                charge.disposition = OccupancyDisposition::InsufficientFunds;
                charge.arrears_after = charge.amount_due;
                position.remaining_fee_units = position
                    .remaining_fee_units
                    .checked_add(charge.amount_due)
                    .ok_or(OccupancyError::ArithmeticOverflow)?;
                charge.remaining_fee_units = position.remaining_fee_units;
                position.arrears = charge.amount_due;
                position.frozen = true;
                self.settlement.usage.arrears_fee_units =
                    checked_add(self.settlement.usage.arrears_fee_units, charge.amount_due)?;
            } else if charge.paid() {
                charge.arrears_after = 0;
                position.arrears = 0;
                position.frozen = false;
                self.settlement.usage.paid_fee_units =
                    checked_add(self.settlement.usage.paid_fee_units, charge.amount_due)?;
            } else {
                self.settlement.usage.arrears_fee_units = checked_add(
                    self.settlement.usage.arrears_fee_units,
                    charge.arrears_after,
                )?;
            }
        }
        self.next_positions
            .retain(|_, position| position.bytes != 0 || position.arrears != 0);
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
    UnprovenPaymentAccount,
    TransferRootMismatch,
    Storage(StorageError),
}

impl Display for OccupancyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AuthorityMismatch { .. } => {
                formatter.write_str("occupancy mandate authority mismatch")
            }
            Self::ResponsibilityMismatch { .. } => {
                formatter.write_str("occupancy payer cannot be rebound")
            }
            Self::DuplicateResponsibility { .. } => {
                formatter.write_str("duplicate occupancy mandate")
            }
            Self::MissingResponsibility { .. } => {
                formatter.write_str("occupied namespace has no mandate")
            }
            Self::ResponsibilityCeilingExceeded => {
                formatter.write_str("occupancy mandate ceiling exceeded")
            }
            Self::ScheduleNotAuthorized { .. } => {
                formatter.write_str("occupancy schedule exceeds persisted mandate")
            }
            Self::FrozenNamespace { .. } => {
                formatter.write_str("occupancy namespace is frozen by arrears")
            }
            Self::BatchRegression {
                previous,
                attempted,
            } => write!(formatter, "occupancy batch {attempted} precedes {previous}"),
            Self::StalePreparation => formatter.write_str("stale occupancy preparation"),
            Self::ArithmeticOverflow => formatter.write_str("occupancy arithmetic overflow"),
            Self::LengthLimit => formatter.write_str("occupancy state exceeds protocol bounds"),
            Self::MalformedEvidence => formatter.write_str("malformed occupancy evidence"),
            Self::UnprovenPaymentAccount => {
                formatter.write_str("occupancy payer has no proven payment account")
            }
            Self::TransferRootMismatch => {
                formatter.write_str("occupancy transfer root is not the committed root")
            }
            Self::Storage(error) => write!(formatter, "occupancy storage refusal: {error}"),
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
    last_finalized_batch: u64,
    positions: BTreeMap<StorageNamespace, OccupancyPosition>,
}

fn charge_legacy_position(
    namespace: StorageNamespace,
    position: &mut OccupancyPosition,
    final_bytes: u64,
    batch: u64,
    usage: &mut OccupancyUsage,
    charges: &mut Vec<OccupancyCharge>,
) -> Result<(), OccupancyError> {
    let intervals = batch
        .checked_sub(position.batch)
        .ok_or(OccupancyError::BatchRegression {
            previous: position.batch,
            attempted: batch,
        })?;
    let byte_batches = u128::from(position.bytes)
        .checked_mul(u128::from(intervals))
        .ok_or(OccupancyError::ArithmeticOverflow)?;
    usage.byte_batches = checked_add(usage.byte_batches, byte_batches)?;
    charges.push(OccupancyCharge {
        namespace,
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
    Ok(())
}

fn charge_governed_position(
    namespace: StorageNamespace,
    position: &mut OccupancyPosition,
    final_bytes: u64,
    batch: u64,
    price: u64,
    authorized_added_fee_units: u128,
    usage: &mut OccupancyUsage,
) -> Result<OccupancyCharge, OccupancyError> {
    if final_bytes > position.maximum_bytes {
        return Err(OccupancyError::ResponsibilityCeilingExceeded);
    }
    let intervals = batch
        .checked_sub(position.batch)
        .ok_or(OccupancyError::BatchRegression {
            previous: position.batch,
            attempted: batch,
        })?;
    let byte_batches = u128::from(position.bytes)
        .checked_mul(u128::from(intervals))
        .ok_or(OccupancyError::ArithmeticOverflow)?;
    let accrued_fee_units = byte_batches
        .checked_mul(u128::from(price))
        .ok_or(OccupancyError::ArithmeticOverflow)?;
    let amount_due = position
        .arrears
        .checked_add(accrued_fee_units)
        .ok_or(OccupancyError::ArithmeticOverflow)?;
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
    let charge = OccupancyCharge {
        namespace,
        payer: position.payer,
        root_program: position.root_program,
        activity_binding: position.activity_binding,
        from_batch: position.batch,
        to_batch: batch,
        recorded_bytes: position.bytes,
        final_bytes,
        byte_batches,
        price,
        accrued_fee_units,
        prior_arrears: position.arrears,
        amount_due,
        disposition,
        authorized_added_fee_units,
        arrears_after: if matches!(disposition, OccupancyDisposition::Paid) {
            0
        } else {
            amount_due
        },
        maximum_bytes: position.maximum_bytes,
        maximum_price: position.maximum_price,
        remaining_fee_units: position.remaining_fee_units,
        mandate: position.mandate,
    };
    position.bytes = final_bytes;
    position.batch = batch;
    position.arrears = if matches!(disposition, OccupancyDisposition::Paid) {
        0
    } else {
        amount_due
    };
    position.frozen = !matches!(disposition, OccupancyDisposition::Paid);
    Ok(charge)
}

fn apply_responsibilities(
    next: &mut BTreeMap<StorageNamespace, OccupancyPosition>,
    declarations: BTreeMap<StorageNamespace, OccupancyResponsibility>,
    batch: u64,
) -> Result<BTreeMap<StorageNamespace, u128>, OccupancyError> {
    let mut authorized_additions = BTreeMap::new();
    for (namespace, responsibility) in declarations {
        validate_scope(namespace, responsibility.payer)?;
        match next.get_mut(&namespace) {
            Some(position) if !position.legacy && position.payer != responsibility.payer => {
                return Err(OccupancyError::ResponsibilityMismatch { namespace })
            }
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
                position.remaining_fee_units = position
                    .remaining_fee_units
                    .checked_add(responsibility.charge_ceiling)
                    .ok_or(OccupancyError::ArithmeticOverflow)?;
                position.mandate = responsibility.mandate;
                if position.legacy {
                    position.frozen = false;
                }
                position.legacy = false;
            }
            None => {
                next.insert(
                    namespace,
                    OccupancyPosition {
                        payer: responsibility.payer,
                        root_program: responsibility.root_program,
                        activity_binding: responsibility.activity_binding,
                        bytes: 0,
                        batch,
                        maximum_bytes: responsibility.maximum_bytes,
                        maximum_price: responsibility.maximum_price,
                        remaining_fee_units: responsibility.charge_ceiling,
                        mandate: responsibility.mandate,
                        arrears: 0,
                        frozen: false,
                        legacy: false,
                    },
                );
            }
        }
        authorized_additions.insert(namespace, responsibility.charge_ceiling);
    }
    Ok(authorized_additions)
}

impl OccupancyLedger {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            last_finalized_batch: 0,
            positions: BTreeMap::new(),
        }
    }
    #[must_use]
    pub const fn activated_after(last_finalized_batch: u64) -> Self {
        Self {
            last_finalized_batch,
            positions: BTreeMap::new(),
        }
    }
    #[must_use]
    pub const fn last_finalized_batch(&self) -> u64 {
        self.last_finalized_batch
    }
    #[must_use]
    pub fn contains_namespace(&self, namespace: StorageNamespace) -> bool {
        self.positions.contains_key(&namespace)
    }
    #[cfg(feature = "host-ffi")]
    pub(crate) fn responsibility_limits(
        &self,
        namespace: StorageNamespace,
    ) -> Option<(PrincipalId, u64)> {
        self.positions
            .get(&namespace)
            .map(|position| (position.payer, position.maximum_bytes))
    }
    ///
    /// # Errors
    ///
    /// Returns a refusal if a requested namespace is inaccessible under the occupancy ledger.
    pub fn ensure_accessible(
        &self,
        namespaces: impl IntoIterator<Item = StorageNamespace>,
    ) -> Result<(), OccupancyError> {
        for namespace in namespaces {
            if self
                .positions
                .get(&namespace)
                .is_some_and(|position| position.frozen)
            {
                return Err(OccupancyError::FrozenNamespace { namespace });
            }
        }
        Ok(())
    }

    #[cfg(feature = "host-ffi")]
    pub(crate) fn frozen_namespaces(&self) -> impl Iterator<Item = StorageNamespace> + '_ {
        self.positions
            .iter()
            .filter_map(|(namespace, position)| position.frozen.then_some(*namespace))
    }
    #[cfg(feature = "host-ffi")]
    pub(crate) fn requires_migration(&self, namespace: StorageNamespace) -> bool {
        self.positions
            .get(&namespace)
            .is_some_and(|position| position.legacy)
    }
    #[cfg(feature = "host-ffi")]
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

    #[cfg(feature = "host-ffi")]
    pub(crate) fn import_activation_position(
        &mut self,
        namespace: StorageNamespace,
        payer: PrincipalId,
        bytes: u64,
    ) -> Result<(), OccupancyError> {
        validate_scope(namespace, payer)?;
        if bytes == 0 {
            return Err(OccupancyError::MalformedEvidence);
        }
        if self.positions.contains_key(&namespace) {
            return Ok(());
        }
        if self.positions.len() == MAX_OCCUPANCY_POSITIONS {
            return Err(OccupancyError::LengthLimit);
        }
        self.positions.insert(
            namespace,
            OccupancyPosition {
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
            },
        );
        Ok(())
    }

    ///
    /// # Errors
    ///
    /// Returns a refusal for an invalid batch, schedule, or occupancy settlement.
    pub fn prepare_unchanged_batch(
        &self,
        batch: u64,
        schedule: FeeSchedule,
    ) -> Result<PreparedOccupancySettlement, OccupancyError> {
        let mut prepared = self.prepare_positions(
            batch,
            &canonical_position_sizes(&self.positions)?,
            BTreeMap::new(),
            schedule,
        )?;
        prepared.finalizes_batch = true;
        Ok(prepared)
    }

    ///
    /// # Errors
    ///
    /// Returns a refusal for invalid batch inputs, responsibility evidence, or settlement arithmetic.
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
            if declarations
                .insert(responsibility.namespace, responsibility)
                .is_some()
            {
                return Err(OccupancyError::DuplicateResponsibility {
                    namespace: responsibility.namespace,
                });
            }
        }
        self.prepare_positions(batch, &canonical_sizes(&sizes)?, declarations, schedule)
    }

    fn prepare_positions(
        &self,
        batch: u64,
        final_storage_sizes: &[u8],
        declarations: BTreeMap<StorageNamespace, OccupancyResponsibility>,
        schedule: FeeSchedule,
    ) -> Result<PreparedOccupancySettlement, OccupancyError> {
        let expected = self
            .last_finalized_batch
            .checked_add(1)
            .ok_or(OccupancyError::ArithmeticOverflow)?;
        if batch != expected {
            return Err(OccupancyError::BatchRegression {
                previous: self.last_finalized_batch,
                attempted: batch,
            });
        }
        if self.positions.len() > MAX_OCCUPANCY_POSITIONS
            || declarations.len() > MAX_OCCUPANCY_POSITIONS
        {
            return Err(OccupancyError::LengthLimit);
        }
        let final_sizes = decode_sizes(final_storage_sizes)?;
        let mut next = self.positions.clone();
        let authorized_additions = apply_responsibilities(&mut next, declarations, batch)?;
        for namespace in final_sizes.keys() {
            if !next.contains_key(namespace) {
                return Err(OccupancyError::MissingResponsibility {
                    namespace: *namespace,
                });
            }
        }
        if next.len() > MAX_OCCUPANCY_POSITIONS {
            return Err(OccupancyError::LengthLimit);
        }
        let price = schedule.occupancy_byte_batch_price();
        let mut usage = OccupancyUsage::default();
        let mut charges = Vec::with_capacity(next.len());
        for (namespace, position) in &mut next {
            let final_bytes = final_sizes.get(namespace).copied().unwrap_or(0);
            if position.legacy {
                charge_legacy_position(
                    *namespace,
                    position,
                    final_bytes,
                    batch,
                    &mut usage,
                    &mut charges,
                )?;
                continue;
            }
            let charge = charge_governed_position(
                *namespace,
                position,
                final_bytes,
                batch,
                price,
                authorized_additions.get(namespace).copied().unwrap_or(0),
                &mut usage,
            )?;
            charges.push(charge);
        }
        next.retain(|_, position| position.bytes != 0 || position.arrears != 0);
        let settlement = OccupancySettlement {
            batch,
            usage,
            fee_schedule: schedule,
            charges,
        };
        if settlement.canonical_evidence().len() > MAX_OCCUPANCY_EVIDENCE_BYTES {
            return Err(OccupancyError::LengthLimit);
        }
        let prior_state = self.canonical_state();
        if prior_state.len() > MAX_OCCUPANCY_LEDGER_BYTES {
            return Err(OccupancyError::LengthLimit);
        }
        Ok(PreparedOccupancySettlement {
            settlement,
            #[cfg(any(feature = "host-ffi", test))]
            prior_state,
            #[cfg(any(feature = "host-ffi", test))]
            final_storage_sizes: final_storage_sizes.to_vec(),
            #[cfg(any(feature = "host-ffi", test))]
            next_positions: next,
            finalizes_batch: false,
        })
    }

    #[cfg(any(feature = "host-ffi", test))]
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
        if prepared.finalizes_batch {
            self.last_finalized_batch = prepared.settlement.batch;
        }
        Ok(prepared.settlement)
    }
    #[cfg(any(feature = "host-ffi", test))]
    pub(crate) fn commit_unchanged_after_debits(
        &mut self,
        prepared: PreparedOccupancySettlement,
    ) -> Result<OccupancySettlement, OccupancyError> {
        if self.canonical_state() != prepared.prior_state
            || canonical_position_sizes(&self.positions)? != prepared.final_storage_sizes
        {
            return Err(OccupancyError::StalePreparation);
        }
        self.positions = prepared.next_positions;
        if prepared.finalizes_batch {
            self.last_finalized_batch = prepared.settlement.batch;
        }
        Ok(prepared.settlement)
    }
    ///
    /// # Errors
    ///
    /// Returns a refusal if evidence decoding or replay disagrees with the supplied final state.
    pub fn replay_evidence(
        &self,
        evidence: &[u8],
        final_storage: &Storage,
        responsibilities: impl IntoIterator<Item = OccupancyResponsibility>,
    ) -> Result<OccupancySettlement, OccupancyError> {
        let recorded = OccupancySettlement::canonical_decode(evidence)?;
        if evidence.starts_with(EVIDENCE_DOMAIN_V1) || evidence.starts_with(EVIDENCE_DOMAIN_V2) {
            return Ok(recorded);
        }
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
        out.extend_from_slice(&self.last_finalized_batch.to_be_bytes());
        out.extend_from_slice(&position_count_bytes(self.positions.len()));
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
    ///
    /// # Errors
    ///
    /// Returns a refusal for malformed or noncanonical ledger encoding.
    pub fn canonical_decode(encoded: &[u8]) -> Result<Self, OccupancyError> {
        if encoded.len() > MAX_OCCUPANCY_LEDGER_BYTES {
            return Err(OccupancyError::LengthLimit);
        }
        if encoded.starts_with(LEDGER_DOMAIN_V1) {
            return decode_legacy_ledger(encoded);
        }
        let mut cursor = Cursor::new(encoded);
        if cursor.take(LEDGER_DOMAIN.len())? != LEDGER_DOMAIN {
            return Err(OccupancyError::MalformedEvidence);
        }
        let last_finalized_batch = cursor.u64()?;
        let count =
            usize::try_from(cursor.u32()?).map_err(|_| OccupancyError::MalformedEvidence)?;
        if count > MAX_OCCUPANCY_POSITIONS {
            return Err(OccupancyError::LengthLimit);
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
            validate_scope(namespace, payer)?;
            let root_program =
                ProgramId::new(cursor.array()?).map_err(|_| OccupancyError::MalformedEvidence)?;
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
            if bytes > maximum_bytes
                || (!legacy && (mandate == [0; 32] || activity_binding == [0; 32]))
                || (!legacy && frozen != (arrears != 0))
                || (legacy && (!frozen || arrears != 0))
                || (bytes == 0 && arrears == 0)
            {
                return Err(OccupancyError::MalformedEvidence);
            }
            if positions
                .insert(
                    namespace,
                    OccupancyPosition {
                        payer,
                        root_program,
                        activity_binding,
                        bytes,
                        batch,
                        maximum_bytes,
                        maximum_price,
                        remaining_fee_units,
                        mandate,
                        arrears,
                        frozen,
                        legacy,
                    },
                )
                .is_some()
            {
                return Err(OccupancyError::MalformedEvidence);
            }
        }
        if !cursor.is_empty() {
            return Err(OccupancyError::MalformedEvidence);
        }
        Ok(Self {
            last_finalized_batch,
            positions,
        })
    }
    #[must_use]
    pub fn recorded_bytes(&self, namespace: StorageNamespace) -> Option<u64> {
        self.positions
            .get(&namespace)
            .map(|position| position.bytes)
    }
}

fn checked_add(left: u128, right: u128) -> Result<u128, OccupancyError> {
    left.checked_add(right)
        .ok_or(OccupancyError::ArithmeticOverflow)
}
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
    if namespace
        .principal_scope()
        .is_some_and(|principal| principal != payer)
    {
        Err(OccupancyError::AuthorityMismatch { namespace })
    } else {
        Ok(())
    }
}
fn bool_byte(value: u8) -> Result<bool, OccupancyError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(OccupancyError::MalformedEvidence),
    }
}
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
        (2, 65) => Ok(StorageNamespace::protocol_private(
            program,
            bytes[33..65]
                .try_into()
                .map_err(|_| OccupancyError::MalformedEvidence)?,
        )),
        _ => Err(OccupancyError::MalformedEvidence),
    }
}
fn encode_schedule(out: &mut Vec<u8>, schedule: FeeSchedule) {
    out.extend_from_slice(&schedule.version().to_be_bytes());
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
fn decode_schedule(
    cursor: &mut Cursor<'_>,
    versioned: bool,
) -> Result<FeeSchedule, OccupancyError> {
    let version = if versioned { cursor.u32()? } else { 1 };
    if version == 0 {
        return Err(OccupancyError::MalformedEvidence);
    }
    Ok(FeeSchedule::new_complete(crate::FeeScheduleParameters {
        version,
        fee_units_per_cpu_fuel: cursor.u64()?,
        fee_units_per_memory_byte: cursor.u64()?,
        fee_units_per_storage_read_byte: cursor.u64()?,
        fee_units_per_storage_write_byte: cursor.u64()?,
        fee_units_per_output_value: cursor.u64()?,
        fee_units_per_output_byte: cursor.u64()?,
        fee_units_per_occupancy_byte_batch: cursor.u64()?,
    }))
}
#[cfg(any(feature = "host-ffi", test))]
fn canonical_storage_sizes(storage: &Storage) -> Result<Vec<u8>, OccupancyError> {
    canonical_sizes(&storage.namespace_sizes()?.into_iter().collect())
}
fn canonical_position_sizes(
    positions: &BTreeMap<StorageNamespace, OccupancyPosition>,
) -> Result<Vec<u8>, OccupancyError> {
    let sizes = positions
        .iter()
        .filter(|(_, position)| position.bytes != 0)
        .map(|(namespace, position)| (*namespace, position.bytes))
        .collect();
    canonical_sizes(&sizes)
}
fn position_count_bytes(count: usize) -> [u8; 4] {
    let bytes = count.to_le_bytes();
    [bytes[3], bytes[2], bytes[1], bytes[0]]
}

fn canonical_sizes(sizes: &BTreeMap<StorageNamespace, u64>) -> Result<Vec<u8>, OccupancyError> {
    if sizes.len() > MAX_OCCUPANCY_POSITIONS {
        return Err(OccupancyError::LengthLimit);
    }
    let mut out = Vec::new();
    out.extend_from_slice(
        &u32::try_from(sizes.len())
            .map_err(|_| OccupancyError::LengthLimit)?
            .to_be_bytes(),
    );
    for (namespace, bytes) in sizes {
        encode_namespace(&mut out, *namespace);
        out.extend_from_slice(&bytes.to_be_bytes());
    }
    Ok(out)
}
fn decode_sizes(encoded: &[u8]) -> Result<BTreeMap<StorageNamespace, u64>, OccupancyError> {
    let mut cursor = Cursor::new(encoded);
    let count = usize::try_from(cursor.u32()?).map_err(|_| OccupancyError::MalformedEvidence)?;
    if count > MAX_OCCUPANCY_POSITIONS {
        return Err(OccupancyError::LengthLimit);
    }
    let mut sizes = BTreeMap::new();
    for _ in 0..count {
        let namespace = decode_namespace(&mut cursor)?;
        let bytes = cursor.u64()?;
        if bytes == 0 || sizes.insert(namespace, bytes).is_some() {
            return Err(OccupancyError::MalformedEvidence);
        }
    }
    if !cursor.is_empty() {
        return Err(OccupancyError::MalformedEvidence);
    }
    Ok(sizes)
}
fn decode_legacy_ledger(encoded: &[u8]) -> Result<OccupancyLedger, OccupancyError> {
    let mut cursor = Cursor::new(encoded);
    let _ = cursor.take(LEDGER_DOMAIN_V1.len())?;
    let count = usize::try_from(cursor.u64()?).map_err(|_| OccupancyError::MalformedEvidence)?;
    if count > MAX_OCCUPANCY_POSITIONS {
        return Err(OccupancyError::LengthLimit);
    }
    let mut positions = BTreeMap::new();
    let mut last_finalized_batch = 0;
    for _ in 0..count {
        let namespace = decode_namespace(&mut cursor)?;
        let payer =
            PrincipalId::new(cursor.array()?).map_err(|_| OccupancyError::MalformedEvidence)?;
        validate_scope(namespace, payer)?;
        let bytes = cursor.u64()?;
        let batch = cursor.u64()?;
        last_finalized_batch = last_finalized_batch.max(batch);
        if bytes == 0
            || positions
                .insert(
                    namespace,
                    OccupancyPosition {
                        payer,
                        root_program: namespace.program(),
                        activity_binding: [0; 32],
                        bytes,
                        batch,
                        maximum_bytes: bytes,
                        maximum_price: 0,
                        remaining_fee_units: 0,
                        mandate: [0; 32],
                        arrears: 0,
                        frozen: true,
                        legacy: true,
                    },
                )
                .is_some()
        {
            return Err(OccupancyError::MalformedEvidence);
        }
    }
    if !cursor.is_empty() {
        return Err(OccupancyError::MalformedEvidence);
    }
    for position in positions.values_mut() {
        position.batch = last_finalized_batch;
    }
    Ok(OccupancyLedger {
        last_finalized_batch,
        positions,
    })
}
fn decode_legacy_settlement(encoded: &[u8]) -> Result<OccupancySettlement, OccupancyError> {
    let versioned = encoded.starts_with(EVIDENCE_DOMAIN_V2);
    let domain = if versioned {
        EVIDENCE_DOMAIN_V2
    } else {
        EVIDENCE_DOMAIN_V1
    };
    let mut cursor = Cursor::new(encoded);
    let _ = cursor.take(domain.len())?;
    let batch = cursor.u64()?;
    let fee_schedule = decode_schedule(&mut cursor, versioned)?;
    let declared_units = cursor.u128()?;
    let declared_fee = cursor.u128()?;
    let count = usize::try_from(cursor.u64()?).map_err(|_| OccupancyError::MalformedEvidence)?;
    if count > MAX_OCCUPANCY_POSITIONS {
        return Err(OccupancyError::LengthLimit);
    }
    let mut usage = OccupancyUsage::default();
    let mut charges = Vec::with_capacity(count);
    for _ in 0..count {
        let namespace = decode_namespace(&mut cursor)?;
        let payer =
            PrincipalId::new(cursor.array()?).map_err(|_| OccupancyError::MalformedEvidence)?;
        let from_batch = cursor.u64()?;
        let to_batch = cursor.u64()?;
        let recorded_bytes = cursor.u64()?;
        let final_bytes = cursor.u64()?;
        let byte_batches = cursor.u128()?;
        let price = cursor.u64()?;
        let accrued_fee_units = cursor.u128()?;
        let intervals = to_batch
            .checked_sub(from_batch)
            .ok_or(OccupancyError::MalformedEvidence)?;
        let expected_units = u128::from(recorded_bytes)
            .checked_mul(u128::from(intervals))
            .ok_or(OccupancyError::ArithmeticOverflow)?;
        if to_batch != batch
            || byte_batches != expected_units
            || price != fee_schedule.occupancy_byte_batch_price()
            || accrued_fee_units
                != byte_batches
                    .checked_mul(u128::from(price))
                    .ok_or(OccupancyError::ArithmeticOverflow)?
        {
            return Err(OccupancyError::MalformedEvidence);
        }
        usage.byte_batches = checked_add(usage.byte_batches, byte_batches)?;
        usage.fee_units = checked_add(usage.fee_units, accrued_fee_units)?;
        usage.paid_fee_units = checked_add(usage.paid_fee_units, accrued_fee_units)?;
        charges.push(OccupancyCharge {
            namespace,
            payer,
            root_program: namespace.program(),
            activity_binding: [0; 32],
            from_batch,
            to_batch,
            recorded_bytes,
            final_bytes,
            byte_batches,
            price,
            accrued_fee_units,
            prior_arrears: 0,
            amount_due: accrued_fee_units,
            authorized_added_fee_units: 0,
            disposition: OccupancyDisposition::Paid,
            arrears_after: 0,
            maximum_bytes: final_bytes.max(recorded_bytes),
            maximum_price: price,
            remaining_fee_units: 0,
            mandate: [0; 32],
        });
    }
    if !cursor.is_empty() || usage.byte_batches != declared_units || usage.fee_units != declared_fee
    {
        return Err(OccupancyError::MalformedEvidence);
    }
    Ok(OccupancySettlement {
        batch,
        usage,
        fee_schedule,
        charges,
    })
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
    fn u32(&mut self) -> Result<u32, OccupancyError> {
        Ok(u32::from_be_bytes(self.array()?))
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
    use crate::{ActivityBudgetBinding, ResourceBudget};

    fn principal(value: u8) -> PrincipalId {
        PrincipalId::new([value; 32]).unwrap_or_else(|error| panic!("nonzero principal: {error:?}"))
    }

    fn namespace(payer: PrincipalId) -> StorageNamespace {
        StorageNamespace::principal(
            crate::ProgramId::new([7; 32])
                .unwrap_or_else(|error| panic!("nonzero program: {error:?}")),
            payer,
        )
    }

    const fn schedule(version: u32, occupancy_price: u64) -> FeeSchedule {
        FeeSchedule::new_complete(crate::FeeScheduleParameters {
            version,
            fee_units_per_cpu_fuel: 0,
            fee_units_per_memory_byte: 0,
            fee_units_per_storage_read_byte: 0,
            fee_units_per_storage_write_byte: 0,
            fee_units_per_output_value: 0,
            fee_units_per_output_byte: 0,
            fee_units_per_occupancy_byte_batch: occupancy_price,
        })
    }

    fn authority(payer: PrincipalId, ceiling: u128) -> OccupancyAuthority {
        let schedule = schedule(1, 2);
        let admitted = AdmittedBudget::new(
            ResourceBudget::new_complete(3, 65_536, 0, 0, 1, 0, 0),
            payer,
            ActivityBudgetBinding::new([9; 32])
                .unwrap_or_else(|error| panic!("nonzero activity: {error:?}")),
            0,
            schedule,
            ResourceBudget::declared(),
        );
        OccupancyAuthority::from_admitted(
            &admitted,
            ceiling,
            schedule,
            crate::ProgramId::new([7; 32])
                .unwrap_or_else(|error| panic!("nonzero program: {error:?}")),
        )
        .unwrap_or_else(|error| panic!("admitted authority: {error:?}"))
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
                &vec![
                    1;
                    usize::try_from(bytes - 1)
                        .unwrap_or_else(|error| panic!("bounded bytes: {error:?}"))
                ],
            )
            .unwrap_or_else(|error| panic!("bounded write: {error:?}"));
        assert_eq!(transaction.commit(), 1);
        storage
    }

    fn initialized(ceiling: u128) -> (OccupancyLedger, Storage, StorageNamespace) {
        let payer = principal(3);
        let namespace = namespace(payer);
        let storage = occupied(namespace);
        let responsibility = authority(payer, ceiling)
            .authorize(namespace, 10, ceiling)
            .unwrap_or_else(|error| panic!("signed occupancy mandate: {error:?}"));
        let mut ledger = OccupancyLedger::new();
        let prepared = ledger
            .prepare_batch(1, &storage, [responsibility], schedule(1, 2))
            .unwrap_or_else(|error| panic!("initial position: {error:?}"));
        ledger
            .commit_after_debits(prepared, &storage)
            .unwrap_or_else(|error| panic!("initial position commit: {error:?}"));
        let prepared = ledger
            .prepare_unchanged_batch(1, schedule(1, 2))
            .unwrap_or_else(|error| panic!("first terminal transition: {error:?}"));
        ledger
            .commit_unchanged_after_debits(prepared)
            .unwrap_or_else(|error| panic!("first terminal commit: {error:?}"));
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
            .unwrap_or_else(|error| panic!("signed occupancy mandate: {error:?}"));
        let mut ledger = OccupancyLedger::new();
        let prepared = ledger
            .prepare_batch(1, &storage, [responsibility], schedule(1, 2))
            .unwrap_or_else(|error| panic!("initial position: {error:?}"));
        ledger
            .commit_after_debits(prepared, &storage)
            .unwrap_or_else(|error| panic!("initial position commit: {error:?}"));
        let prepared = ledger
            .prepare_unchanged_batch(1, schedule(1, 2))
            .unwrap_or_else(|error| panic!("first terminal transition: {error:?}"));
        ledger
            .commit_unchanged_after_debits(prepared)
            .unwrap_or_else(|error| panic!("first terminal commit: {error:?}"));
        (ledger, storage, namespace)
    }

    #[test]
    fn lifetime_ceiling_exhaustion_freezes_only_its_position() {
        let (mut ledger, _storage, namespace) = initialized(20);
        let paid = ledger
            .prepare_unchanged_batch(2, schedule(1, 2))
            .unwrap_or_else(|error| panic!("contiguous second batch: {error:?}"));
        assert_eq!(paid.settlement().usage().paid_fee_units, 20);
        ledger
            .commit_unchanged_after_debits(paid)
            .unwrap_or_else(|error| panic!("paid terminal commit: {error:?}"));
        let exhausted = ledger
            .prepare_unchanged_batch(3, schedule(1, 2))
            .unwrap_or_else(|error| panic!("ceiling exhaustion is a disposition: {error:?}"));
        assert_eq!(
            exhausted.settlement().charges()[0].disposition(),
            OccupancyDisposition::ChargeCeilingExceeded
        );
        assert_eq!(exhausted.settlement().usage().arrears_fee_units, 20);
        ledger
            .commit_unchanged_after_debits(exhausted)
            .unwrap_or_else(|error| panic!("frozen terminal commit: {error:?}"));
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
            .unwrap_or_else(|error| panic!("contiguous settlement: {error:?}"));
        prepared
            .defer_unpaid(&BTreeSet::from([principal(3)]))
            .unwrap_or_else(|error| panic!("typed insolvency: {error:?}"));
        assert_eq!(
            prepared.settlement().charges()[0].disposition(),
            OccupancyDisposition::InsufficientFunds
        );
        insolvent
            .commit_unchanged_after_debits(prepared)
            .unwrap_or_else(|error| {
                panic!("insolvent position does not halt the batch: {error:?}")
            });

        let (mut repriced, _, _) = initialized(100);
        let prepared = repriced
            .prepare_unchanged_batch(2, schedule(2, 3))
            .unwrap_or_else(|error| panic!("versioned schedule transition: {error:?}"));
        assert_eq!(
            prepared.settlement().charges()[0].disposition(),
            OccupancyDisposition::ScheduleCeilingExceeded
        );
        repriced
            .commit_unchanged_after_debits(prepared)
            .unwrap_or_else(|error| panic!("repriced position does not halt the batch: {error:?}"));
    }

    #[test]
    fn multiple_payers_settle_atomically_with_isolated_arrears() {
        let first = principal(3);
        let second = principal(4);
        let first_namespace = namespace(first);
        let second_namespace = StorageNamespace::principal(
            crate::ProgramId::new([8; 32])
                .unwrap_or_else(|error| panic!("nonzero program: {error:?}")),
            second,
        );
        let mut storage = occupied(first_namespace);
        let mut transaction = storage.transaction(second_namespace);
        transaction
            .write(b"k", &[2; 9])
            .unwrap_or_else(|error| panic!("bounded write: {error:?}"));
        assert_eq!(transaction.commit(), 1);
        let responsibilities = [
            authority(first, 100)
                .authorize(first_namespace, 10, 100)
                .unwrap_or_else(|error| panic!("first mandate: {error:?}")),
            authority(second, 100)
                .authorize(second_namespace, 10, 100)
                .unwrap_or_else(|error| panic!("second mandate: {error:?}")),
        ];
        let mut ledger = OccupancyLedger::new();
        let prepared = ledger
            .prepare_batch(1, &storage, responsibilities, schedule(1, 2))
            .unwrap_or_else(|error| panic!("multi-payer initialization: {error:?}"));
        ledger
            .commit_after_debits(prepared, &storage)
            .unwrap_or_else(|error| panic!("multi-payer initialization commit: {error:?}"));
        let first_batch = ledger
            .prepare_unchanged_batch(1, schedule(1, 2))
            .unwrap_or_else(|error| panic!("initial terminal: {error:?}"));
        ledger
            .commit_unchanged_after_debits(first_batch)
            .unwrap_or_else(|error| panic!("initial terminal commit: {error:?}"));
        let mut second_batch = ledger
            .prepare_unchanged_batch(2, schedule(1, 2))
            .unwrap_or_else(|error| panic!("multi-payer settlement: {error:?}"));
        second_batch
            .defer_unpaid(&BTreeSet::from([second]))
            .unwrap_or_else(|error| panic!("one payer insolvent: {error:?}"));
        let dispositions = second_batch
            .settlement()
            .payer_dispositions()
            .unwrap_or_else(|error| panic!("bounded payer totals: {error:?}"));
        assert_eq!(dispositions[&first], (20, 20, 0, false));
        assert_eq!(dispositions[&second], (20, 0, 20, true));
        ledger
            .commit_unchanged_after_debits(second_batch)
            .unwrap_or_else(|error| panic!("one insolvent payer cannot halt the batch: {error:?}"));
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
            ledger
                .prepare_unchanged_batch(3, schedule(1, 2))
                .err()
                .unwrap_or_else(|| panic!("expected refusal")),
            OccupancyError::BatchRegression {
                previous: 1,
                attempted: 3,
            }
        );
        let empty = Storage::new();
        let prepared = ledger
            .prepare_batch(2, &empty, [], schedule(1, 2))
            .unwrap_or_else(|error| panic!("drop settlement: {error:?}"));
        assert_eq!(prepared.settlement().charges()[0].final_bytes, 0);
        assert_eq!(prepared.settlement().charges()[0].fee_units(), 20);
        let evidence = prepared.settlement().canonical_evidence();
        assert_eq!(
            OccupancySettlement::canonical_decode(&evidence),
            Ok(prepared.settlement().clone())
        );
        ledger
            .commit_after_debits(prepared, &empty)
            .unwrap_or_else(|error| panic!("drop commit: {error:?}"));
        let terminal = ledger
            .prepare_unchanged_batch(2, schedule(1, 2))
            .unwrap_or_else(|error| panic!("same-batch terminal transition: {error:?}"));
        ledger
            .commit_unchanged_after_debits(terminal)
            .unwrap_or_else(|error| panic!("same-batch terminal commit: {error:?}"));
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
                    .unwrap_or_else(|error| panic!("contiguous property settlement: {error:?}"));
                let usage = prepared.settlement().usage();
                assert_eq!(usage.byte_batches, u128::from(bytes));
                assert_eq!(usage.fee_units, u128::from(bytes) * 2);
                cumulative_units = cumulative_units
                    .checked_add(usage.fee_units)
                    .unwrap_or_else(|| panic!("bounded matrix"));
                assert!(cumulative_units > prior_cumulative_units);
                prior_cumulative_units = cumulative_units;
                if batch == 2 {
                    assert!(usage.fee_units > prior_first_interval_fee);
                    prior_first_interval_fee = usage.fee_units;
                }
                ledger
                    .commit_unchanged_after_debits(prepared)
                    .unwrap_or_else(|error| panic!("property terminal commit: {error:?}"));
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
                        .unwrap_or_else(|error| panic!("pre-drop interval: {error:?}"));
                    ledger
                        .commit_unchanged_after_debits(prepared)
                        .unwrap_or_else(|error| panic!("pre-drop commit: {error:?}"));
                }
                let empty = Storage::new();
                let dropped = ledger
                    .prepare_batch(drop_batch, &empty, [], schedule(1, 2))
                    .unwrap_or_else(|error| panic!("drop interval: {error:?}"));
                assert_eq!(dropped.settlement().charges().len(), 1);
                assert_eq!(
                    dropped.settlement().charges()[0].byte_batches(),
                    u128::from(bytes)
                );
                assert_eq!(dropped.settlement().charges()[0].final_bytes, 0);
                ledger
                    .commit_after_debits(dropped, &empty)
                    .unwrap_or_else(|error| panic!("drop state commit: {error:?}"));
                let terminal = ledger
                    .prepare_unchanged_batch(drop_batch, schedule(1, 2))
                    .unwrap_or_else(|error| panic!("drop terminal: {error:?}"));
                ledger
                    .commit_unchanged_after_debits(terminal)
                    .unwrap_or_else(|error| panic!("drop terminal commit: {error:?}"));
                assert_eq!(ledger.recorded_bytes(namespace), None);
                let after = ledger
                    .prepare_unchanged_batch(drop_batch + 1, schedule(1, 2))
                    .unwrap_or_else(|error| panic!("post-drop interval: {error:?}"));
                assert_eq!(after.settlement().usage(), OccupancyUsage::default());
            }
        }
    }

    const SELLER_DID: &[u8] =
        b"did:layerx:c4420d73f7b2e56599e25f99790d680f84348adf420eed89b2484b2ece345e64";

    fn digest(text: &str) -> [u8; 32] {
        let mut bytes = [0_u8; 32];
        for (index, pair) in text.as_bytes().chunks_exact(2).enumerate() {
            let pair = std::str::from_utf8(pair).unwrap_or_else(|error| panic!("{error}"));
            bytes[index] = u8::from_str_radix(pair, 16).unwrap_or_else(|error| panic!("{error}"));
        }
        bytes
    }

    fn native_asset() -> [u8; 32] {
        let mut asset = [0_u8; 32];
        asset[0] = 1;
        asset
    }

    fn paid_settlement(payers: &[PrincipalId]) -> OccupancySettlement {
        let mut storage = Storage::new();
        let mut responsibilities = Vec::new();
        for payer in payers {
            let namespace = namespace(*payer);
            let mut transaction = storage.transaction(namespace);
            transaction
                .write(b"k", &[1; 118])
                .unwrap_or_else(|error| panic!("bounded write: {error:?}"));
            assert_eq!(transaction.commit(), 1);
            let authority = OccupancyAuthority {
                payer: *payer,
                root_program: crate::ProgramId::new([7; 32])
                    .unwrap_or_else(|error| panic!("nonzero program: {error:?}")),
                activity_binding: [9; 32],
                occupancy_fee_ceiling: 1_000_000,
                maximum_price: 60,
            };
            responsibilities.push(
                authority
                    .authorize(namespace, 119, 1_000_000)
                    .unwrap_or_else(|error| panic!("signed occupancy mandate: {error:?}")),
            );
        }
        let mut ledger = OccupancyLedger::new();
        let prepared = ledger
            .prepare_batch(1, &storage, responsibilities, schedule(1, 60))
            .unwrap_or_else(|error| panic!("initial position: {error:?}"));
        ledger
            .commit_after_debits(prepared, &storage)
            .unwrap_or_else(|error| panic!("initial position commit: {error:?}"));
        let prepared = ledger
            .prepare_unchanged_batch(1, schedule(1, 60))
            .unwrap_or_else(|error| panic!("first terminal transition: {error:?}"));
        ledger
            .commit_unchanged_after_debits(prepared)
            .unwrap_or_else(|error| panic!("first terminal commit: {error:?}"));
        ledger
            .prepare_unchanged_batch(2, schedule(1, 60))
            .unwrap_or_else(|error| panic!("paid interval: {error:?}"))
            .settlement()
            .clone()
    }

    #[test]
    fn state_commitment_root_commits_the_proven_payment_account() {
        let asset = native_asset();
        let payer = PrincipalId::new(digest(
            "b3ae288574ffc1a59920de3c7e8b7f5bc79463a205a4df3fae8e7dc516ee4fb3",
        ))
        .unwrap_or_else(|error| panic!("nonzero principal: {error:?}"));
        let kernel_root =
            digest("ba040b25be4b4328ea29a6169b560c80da57d207405258c372787f3ca938cfaa");
        let principal_root =
            digest("554eb383946c1aea69e6fb7523097d7d51c7b19739289be622e8e4c0694886ff");
        let settlement = paid_settlement(&[payer]);
        let dispositions = settlement
            .payer_dispositions()
            .unwrap_or_else(|error| panic!("bounded payer totals: {error:?}"));
        assert_eq!(dispositions[&payer], (7140, 7140, 0, false));

        let main = OccupancyPaymentAccount::main(SELLER_DID, asset)
            .unwrap_or_else(|error| panic!("main account: {error:?}"));
        let asset_account = OccupancyPaymentAccount::asset(SELLER_DID, asset)
            .unwrap_or_else(|error| panic!("asset account: {error:?}"));
        assert_eq!(main.payer(), payer);
        assert_eq!(asset_account.payer(), payer);
        assert_eq!(
            main.account(),
            digest("1673e7832a44e5fa42c263e256728b61d28245a6ae13d160dda1954b7a1f2a79")
        );
        assert_eq!(
            asset_account.account(),
            digest("fe200a1277cc137963ae36b5971010476f653250ef6b737cb0948758622b99bb")
        );

        assert_eq!(settlement.transfer_root(asset), Ok(principal_root));
        assert_eq!(
            settlement.verify_transfer_root(2, asset, &[], principal_root),
            Ok(Vec::new())
        );
        assert_eq!(
            settlement.verify_transfer_root(2, asset, &[main], kernel_root),
            Err(OccupancyError::TransferRootMismatch)
        );
        assert_eq!(
            settlement.verify_transfer_root(3, asset, &[main], kernel_root),
            Ok(vec![main])
        );
        assert_eq!(
            settlement.verify_transfer_root(3, asset, &[asset_account, main], kernel_root),
            Ok(vec![main])
        );
        assert_eq!(
            settlement.verify_transfer_root(3, asset, &[main], principal_root),
            Err(OccupancyError::TransferRootMismatch)
        );
        assert_eq!(
            settlement.verify_transfer_root(3, asset, &[asset_account], kernel_root),
            Err(OccupancyError::TransferRootMismatch)
        );
        assert_eq!(
            settlement.verify_transfer_root(3, asset, &[], principal_root),
            Err(OccupancyError::UnprovenPaymentAccount)
        );
        assert_eq!(
            settlement.verify_transfer_root(3, [0; 32], &[main], kernel_root),
            Err(OccupancyError::MalformedEvidence)
        );
    }

    #[test]
    fn state_commitment_root_refuses_accounts_the_payer_does_not_derive() {
        let asset = native_asset();
        let payer = PrincipalId::new(digest(
            "b3ae288574ffc1a59920de3c7e8b7f5bc79463a205a4df3fae8e7dc516ee4fb3",
        ))
        .unwrap_or_else(|error| panic!("nonzero principal: {error:?}"));
        let settlement = paid_settlement(&[payer]);
        let stranger_did: &[u8] = b"did:layerx:stranger";
        let stranger = OccupancyPaymentAccount::main(stranger_did, asset)
            .unwrap_or_else(|error| panic!("stranger account: {error:?}"));
        assert_ne!(stranger.payer(), payer);
        let treasury = account_identifier(b"system:fees")
            .unwrap_or_else(|error| panic!("treasury: {error:?}"));
        let stranger_root = transfer_merkle_root(vec![transfer_leaf(
            stranger.account(),
            treasury,
            asset,
            7140,
        )]);
        assert_eq!(
            settlement.verify_transfer_root(3, asset, &[stranger], stranger_root),
            Err(OccupancyError::UnprovenPaymentAccount)
        );
        assert_eq!(
            OccupancyPaymentAccount::prove(SELLER_DID, asset, stranger.account()),
            Err(OccupancyError::UnprovenPaymentAccount)
        );
        assert_eq!(
            OccupancyPaymentAccount::prove(SELLER_DID, asset, payer.bytes()),
            Err(OccupancyError::UnprovenPaymentAccount)
        );
        let other_asset = [0x81; 32];
        let wrong_asset = OccupancyPaymentAccount::main(SELLER_DID, other_asset)
            .unwrap_or_else(|error| panic!("other asset account: {error:?}"));
        let kernel_root =
            digest("ba040b25be4b4328ea29a6169b560c80da57d207405258c372787f3ca938cfaa");
        assert_eq!(
            settlement.verify_transfer_root(3, asset, &[wrong_asset], kernel_root),
            Err(OccupancyError::UnprovenPaymentAccount)
        );
        assert_eq!(
            OccupancyPaymentAccount::main(b"", asset),
            Err(OccupancyError::UnprovenPaymentAccount)
        );
        assert_eq!(
            OccupancyPaymentAccount::main(&[b'a'; 256], asset),
            Err(OccupancyError::UnprovenPaymentAccount)
        );
    }

    #[test]
    fn state_commitment_root_selects_one_account_per_payer_within_the_bound() {
        let asset = native_asset();
        let dids: Vec<Vec<u8>> = (0..9_u8)
            .map(|index| format!("did:layerx:payer{index}").into_bytes())
            .collect();
        let mut accounts = Vec::new();
        let mut payers = Vec::new();
        for did in &dids {
            let main = OccupancyPaymentAccount::main(did, asset)
                .unwrap_or_else(|error| panic!("main account: {error:?}"));
            let asset_account = OccupancyPaymentAccount::asset(did, asset)
                .unwrap_or_else(|error| panic!("asset account: {error:?}"));
            payers.push(main.payer());
            accounts.push(main);
            accounts.push(asset_account);
        }
        let treasury = account_identifier(b"system:fees")
            .unwrap_or_else(|error| panic!("treasury: {error:?}"));

        let pair = paid_settlement(&payers[..2]);
        let mut ordered: Vec<_> = accounts[..4].chunks(2).collect();
        ordered.sort_by_key(|candidates| candidates[0].payer());
        let committed = vec![ordered[0][1], ordered[1][0]];
        let root = transfer_merkle_root(
            committed
                .iter()
                .map(|account| transfer_leaf(account.account(), treasury, asset, 7140))
                .collect(),
        );
        assert_eq!(
            pair.verify_transfer_root(3, asset, &accounts[..4], root),
            Ok(committed)
        );

        let nine = paid_settlement(&payers);
        assert_eq!(
            nine.verify_transfer_root(3, asset, &accounts, root),
            Err(OccupancyError::LengthLimit)
        );
        let exact: Vec<_> = accounts.iter().copied().step_by(2).collect();
        let mut leaves: Vec<_> = exact.clone();
        leaves.sort_by_key(OccupancyPaymentAccount::payer);
        let nine_root = transfer_merkle_root(
            leaves
                .iter()
                .map(|account| transfer_leaf(account.account(), treasury, asset, 7140))
                .collect(),
        );
        assert_eq!(
            nine.verify_transfer_root(3, asset, &exact, nine_root),
            Ok(leaves)
        );
    }
}
