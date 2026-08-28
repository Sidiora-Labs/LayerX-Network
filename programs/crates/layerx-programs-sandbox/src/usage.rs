//! Incremental, receipt-backed settlement of sandbox usage.

use core::fmt::{self, Display};
use std::collections::BTreeSet;

use layerx_programs_runtime::{
    hash_bytes, sandbox_escrow_charge_root, BudgetedResourceFailureRecord,
    BudgetedV1FailureRecord, FeeSchedule,
    ActivityBudgetBinding, HashAlgorithm, MeteredUsage, ProgramId,
};

use crate::{Escrow, EscrowRefusal, Lease, LeaseRefusal, LeaseUsage};

const RECEIPT_DOMAIN: &[u8] = b"LayerX/programs/sandbox/usage-receipt/v4\0";
const LEDGER_DOMAIN: &[u8] = b"LayerX/programs/sandbox/usage-ledger/v2\0";
const LEDGER_ACCUMULATOR_DOMAIN: &[u8] = b"LayerX/programs/sandbox/usage-accumulator/v1\0";
const GENESIS_RECEIPT: [u8; 32] = [0; 32];
pub const MAX_USAGE_RECEIPTS: u64 = 1_000_000;
pub const MAX_USAGE_STATE_VALUE_BYTES: usize = 1_048_576;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ActivityOutcome { Success = 1, ProgramFailure = 2, ResourceExhaustion = 3 }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UsageObservation { outcome: ActivityOutcome, root_program: ProgramId,
    activity_binding: ActivityBudgetBinding, usage: MeteredUsage }

impl UsageObservation {
    #[must_use]
    pub(crate) const fn host_sealed(
        outcome: ActivityOutcome, root_program: ProgramId,
        activity_binding: ActivityBudgetBinding, usage: MeteredUsage,
    ) -> Self { Self { outcome, root_program, activity_binding, usage } }
    #[must_use]
    pub const fn success(root_program: ProgramId, activity_binding: ActivityBudgetBinding,
        usage: MeteredUsage) -> Self {
        Self { outcome: ActivityOutcome::Success, root_program, activity_binding, usage }
    }

    #[must_use]
    pub const fn failed(record: &BudgetedV1FailureRecord) -> Self {
        Self { outcome: ActivityOutcome::ProgramFailure, root_program: record.root_program(),
            activity_binding: record.activity_binding(), usage: record.usage() }
    }

    #[must_use]
    pub const fn exhausted(record: &BudgetedResourceFailureRecord) -> Self {
        Self { outcome: ActivityOutcome::ResourceExhaustion, root_program: record.root_program(),
            activity_binding: record.activity_binding(), usage: record.usage() }
    }

    #[must_use] pub const fn outcome(self) -> ActivityOutcome { self.outcome }
    #[must_use] pub const fn usage(self) -> MeteredUsage { self.usage }
    #[must_use] pub const fn root_program(self) -> ProgramId { self.root_program }
    #[must_use] pub const fn activity_binding(self) -> ActivityBudgetBinding { self.activity_binding }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UsagePrices {
    pub schedule_version: u32,
    pub cpu: u64,
    pub memory: u64,
    pub storage_read: u64,
    pub storage_write: u64,
    pub output_values: u64,
    pub output_bytes: u64,
    pub occupancy_byte_batch: u64,
}

impl UsagePrices {
    #[must_use]
    pub const fn from_schedule(schedule: FeeSchedule) -> Self {
        Self {
            schedule_version: schedule.version(), cpu: schedule.cpu_price(),
            memory: schedule.memory_byte_price(),
            storage_read: schedule.storage_read_byte_price(),
            storage_write: schedule.storage_write_byte_price(),
            output_values: schedule.output_value_price(),
            output_bytes: schedule.output_byte_price(),
            occupancy_byte_batch: schedule.occupancy_byte_batch_price(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UsageReceipt {
    lease: crate::LeaseId,
    sequence: u64,
    observed_batch: u64,
    activity_id: [u8; 32],
    lease_terms_digest: [u8; 32],
    expected_lease_digest: [u8; 32],
    resulting_lease_digest: [u8; 32],
    fee_destination: [u8; 32],
    previous: [u8; 32],
    previous_accumulator_root: [u8; 32],
    observation: UsageObservation,
    cumulative: LeaseUsage,
    prices: UsagePrices,
    charged: u128,
    cumulative_spent: u128,
    transfer_root: [u8; 32],
    digest: [u8; 32],
}

/// A usage receipt recovered from a signed, included canonical protocol receipt.
///
/// Construction is restricted to the activity-plane verifier so historical
/// archive verification cannot accidentally trust a naked self-hashed value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthenticatedUsageReceipt { receipt: UsageReceipt }

impl AuthenticatedUsageReceipt {
    pub(crate) const fn new(receipt: UsageReceipt) -> Self { Self { receipt } }
    #[must_use] pub const fn receipt(&self) -> &UsageReceipt { &self.receipt }
}

impl UsageReceipt {
    #[must_use] pub const fn lease(&self) -> crate::LeaseId { self.lease }
    #[must_use] pub const fn sequence(&self) -> u64 { self.sequence }
    #[must_use] pub const fn observed_batch(&self) -> u64 { self.observed_batch }
    #[must_use] pub const fn activity_id(&self) -> [u8; 32] { self.activity_id }
    #[must_use] pub const fn lease_terms_digest(&self) -> [u8; 32] { self.lease_terms_digest }
    #[must_use] pub const fn expected_lease_digest(&self) -> [u8; 32] { self.expected_lease_digest }
    #[must_use] pub const fn resulting_lease_digest(&self) -> [u8; 32] { self.resulting_lease_digest }
    #[must_use] pub const fn fee_destination(&self) -> [u8; 32] { self.fee_destination }
    #[must_use] pub const fn outcome(&self) -> ActivityOutcome { self.observation.outcome }
    #[must_use] pub const fn usage(&self) -> MeteredUsage { self.observation.usage }
    #[must_use] pub const fn cumulative(&self) -> LeaseUsage { self.cumulative }
    #[must_use] pub const fn prices(&self) -> UsagePrices { self.prices }
    #[must_use] pub const fn charged(&self) -> u128 { self.charged }
    #[must_use] pub const fn cumulative_spent(&self) -> u128 { self.cumulative_spent }
    #[must_use] pub const fn transfer_root(&self) -> [u8; 32] { self.transfer_root }
    #[must_use] pub const fn digest(&self) -> [u8; 32] { self.digest }

    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        self.write_canonical(&mut bytes);
        bytes
    }

    pub(crate) fn write_canonical(&self, bytes: &mut Vec<u8>) {
        bytes.clear();
        self.append_canonical(bytes);
    }

    fn append_canonical(&self, bytes: &mut Vec<u8>) {
        bytes.extend_from_slice(RECEIPT_DOMAIN);
        bytes.extend_from_slice(&self.lease.bytes());
        bytes.extend_from_slice(&self.sequence.to_be_bytes());
        bytes.extend_from_slice(&self.observed_batch.to_be_bytes());
        bytes.extend_from_slice(&self.activity_id);
        bytes.extend_from_slice(&self.lease_terms_digest);
        bytes.extend_from_slice(&self.expected_lease_digest);
        bytes.extend_from_slice(&self.resulting_lease_digest);
        bytes.extend_from_slice(&self.fee_destination);
        bytes.extend_from_slice(&self.previous);
        bytes.extend_from_slice(&self.previous_accumulator_root);
        bytes.push(self.observation.outcome as u8);
        bytes.extend_from_slice(&self.observation.root_program.bytes());
        encode_metered(&mut bytes, self.observation.usage);
        encode_lease_usage(&mut bytes, self.cumulative);
        encode_prices(&mut bytes, self.prices);
        bytes.extend_from_slice(&self.charged.to_be_bytes());
        bytes.extend_from_slice(&self.cumulative_spent.to_be_bytes());
        bytes.extend_from_slice(&self.transfer_root);
    }

    pub fn verify(&self) -> Result<(), UsageRefusal> {
        let expected_execution = execution_fee(self.observation.usage, self.prices)?;
        let expected_occupancy = self.observation.usage.occupancy_byte_batches
            .checked_mul(u128::from(self.prices.occupancy_byte_batch))
            .ok_or(UsageRefusal::ArithmeticOverflow)?;
        let expected_charge = expected_execution.checked_add(expected_occupancy)
            .ok_or(UsageRefusal::ArithmeticOverflow)?;
        if self.activity_id == [0; 32] || self.lease_terms_digest == [0; 32]
            || self.expected_lease_digest == [0; 32] || self.resulting_lease_digest == [0; 32]
            || self.fee_destination == [0; 32] || self.transfer_root == [0; 32]
            || self.observation.activity_binding.bytes() != self.activity_id
            || self.charged == 0 || self.cumulative_spent < self.charged
            || self.observation.usage.fee_units != expected_execution
            || self.observation.usage.occupancy_fee_units != expected_occupancy
            || self.charged != expected_charge
            || receipt_digest(&self.canonical_bytes())? != self.digest
        { return Err(UsageRefusal::InvalidReceipt); }
        Ok(())
    }

    pub fn decode(canonical: &[u8], digest: [u8; 32]) -> Result<Self, UsageRefusal> {
        let mut cursor = Cursor::new(canonical);
        if cursor.take(RECEIPT_DOMAIN.len())? != RECEIPT_DOMAIN {
            return Err(UsageRefusal::InvalidReceipt);
        }
        let lease = crate::LeaseId::new(cursor.array()?).map_err(|_| UsageRefusal::InvalidReceipt)?;
        let sequence = cursor.u64()?;
        let observed_batch = cursor.u64()?;
        let activity_id = cursor.array()?;
        let lease_terms_digest = cursor.array()?;
        let expected_lease_digest = cursor.array()?;
        let resulting_lease_digest = cursor.array()?;
        let fee_destination = cursor.array()?;
        let previous = cursor.array()?;
        let previous_accumulator_root = cursor.array()?;
        let outcome = match cursor.u8()? {
            1 => ActivityOutcome::Success,
            2 => ActivityOutcome::ProgramFailure,
            3 => ActivityOutcome::ResourceExhaustion,
            _ => return Err(UsageRefusal::InvalidReceipt),
        };
        let root_program = ProgramId::new(cursor.array()?).map_err(|_| UsageRefusal::InvalidReceipt)?;
        let usage = MeteredUsage {
            cpu_fuel: cursor.u64()?, memory_bytes: cursor.u64()?,
            storage_read_bytes: cursor.u64()?, storage_write_bytes: cursor.u64()?,
            output_values: u32::try_from(cursor.u64()?).map_err(|_| UsageRefusal::InvalidReceipt)?,
            output_bytes: cursor.u64()?, occupancy_byte_batches: cursor.u128()?,
            occupancy_fee_units: cursor.u128()?, fee_units: cursor.u128()?,
        };
        let cumulative = LeaseUsage {
            cpu_fuel: cursor.u64()?, memory_bytes: cursor.u64()?,
            storage_read_bytes: cursor.u64()?, storage_write_bytes: cursor.u64()?,
            output_values: cursor.u64()?, output_bytes: cursor.u64()?,
            table_elements: cursor.u64()?, namespace_bytes: cursor.u64()?,
        };
        let prices = UsagePrices {
            schedule_version: cursor.u32()?, cpu: cursor.u64()?, memory: cursor.u64()?,
            storage_read: cursor.u64()?, storage_write: cursor.u64()?,
            output_values: cursor.u64()?, output_bytes: cursor.u64()?,
            occupancy_byte_batch: cursor.u64()?,
        };
        let charged = cursor.u128()?;
        let cumulative_spent = cursor.u128()?;
        let transfer_root = cursor.array()?;
        if !cursor.is_empty() { return Err(UsageRefusal::InvalidReceipt); }
        let receipt = Self { lease, sequence, observed_batch, activity_id,
            lease_terms_digest, expected_lease_digest, resulting_lease_digest,
            fee_destination, previous, previous_accumulator_root,
            observation: UsageObservation { outcome, root_program,
                activity_binding: ActivityBudgetBinding::new(activity_id)
                    .map_err(|_| UsageRefusal::InvalidReceipt)?, usage }, cumulative, prices, charged,
            cumulative_spent, transfer_root, digest };
        receipt.verify()?;
        if receipt.canonical_bytes() != canonical { return Err(UsageRefusal::InvalidReceipt); }
        Ok(receipt)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UsageLedger {
    receipt_count: u64,
    spent: u128,
    accumulator_root: [u8; 32],
    latest: Option<UsageReceipt>,
}

impl Default for UsageLedger {
    fn default() -> Self { Self::new() }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DurableUsageState { pub lease: Lease, pub escrow: Escrow, pub ledger: UsageLedger }

impl UsageLedger {
    #[must_use] pub const fn new() -> Self {
        Self { receipt_count: 0, spent: 0, accumulator_root: [0; 32], latest: None }
    }
    #[must_use] pub const fn receipt_count(&self) -> u64 { self.receipt_count }
    #[must_use] pub const fn running_total(&self) -> u128 { self.spent }
    #[must_use] pub const fn accumulator_root(&self) -> [u8; 32] { self.accumulator_root }
    #[must_use] pub const fn latest(&self) -> Option<&UsageReceipt> { self.latest.as_ref() }

    #[must_use]
    pub fn canonical_state(&self) -> Result<Vec<u8>, UsageRefusal> {
        let mut state = Vec::new();
        self.write_canonical_state(&mut state)?;
        Ok(state)
    }

    pub(crate) fn write_canonical_state(&self, state: &mut Vec<u8>) -> Result<(), UsageRefusal> {
        state.clear();
        state.extend_from_slice(LEDGER_DOMAIN);
        state.extend_from_slice(&self.receipt_count.to_be_bytes());
        state.extend_from_slice(&self.spent.to_be_bytes());
        state.extend_from_slice(&self.accumulator_root);
        match &self.latest {
            None => state.push(0),
            Some(receipt) => {
                state.push(1);
            let length_offset = state.len();
            state.extend_from_slice(&[0; 4]);
            let canonical_start = state.len();
            receipt.append_canonical(state);
            let canonical_length = state.len() - canonical_start;
            state[length_offset..length_offset + 4].copy_from_slice(
                &u32::try_from(canonical_length)
                .map_err(|_| UsageRefusal::ReceiptLimit)?.to_be_bytes());
            state.extend_from_slice(&receipt.digest);
            }
        }
        if state.len() > MAX_USAGE_STATE_VALUE_BYTES { return Err(UsageRefusal::ReceiptLimit); }
        Ok(())
    }

    pub fn decode_state(state: &[u8], lease: &Lease, escrow: &Escrow) -> Result<Self, UsageRefusal> {
        if state.len() > MAX_USAGE_STATE_VALUE_BYTES { return Err(UsageRefusal::ReceiptLimit); }
        let mut cursor = Cursor::new(state);
        if cursor.take(LEDGER_DOMAIN.len())? != LEDGER_DOMAIN {
            return Err(UsageRefusal::InvalidChain);
        }
        let receipt_count = cursor.u64()?;
        let spent = cursor.u128()?;
        let accumulator_root = cursor.array()?;
        let latest = match cursor.u8()? {
            0 => None,
            1 => {
            let length = usize::try_from(cursor.u32()?).map_err(|_| UsageRefusal::InvalidChain)?;
            let canonical = cursor.take(length)?;
                Some(UsageReceipt::decode(canonical, cursor.array()?)?)
            }
            _ => return Err(UsageRefusal::InvalidChain),
        };
        if !cursor.is_empty() { return Err(UsageRefusal::InvalidChain); }
        let ledger = Self { receipt_count, spent, accumulator_root, latest };
        ledger.verify(lease, escrow)?;
        if ledger.canonical_state()? != state { return Err(UsageRefusal::InvalidChain); }
        Ok(ledger)
    }

    pub fn verify(&self, lease: &Lease, escrow: &Escrow) -> Result<(), UsageRefusal> {
        if self.receipt_count > MAX_USAGE_RECEIPTS
            || (self.receipt_count == 0) != self.latest.is_none()
            || self.spent != escrow.spent() || self.spent != lease.escrow_consumed() {
            return Err(UsageRefusal::ConservationViolation);
        }
        if let Some(receipt) = &self.latest {
            receipt.verify()?;
            if receipt.sequence != self.receipt_count || receipt.lease != lease.id()
                || receipt.cumulative_spent != self.spent || receipt.cumulative != lease.usage()
                || receipt.resulting_lease_digest != lease.state_digest().map_err(UsageRefusal::Lease)?
                || receipt.lease_terms_digest != lease.request_binding_digest().map_err(UsageRefusal::Lease)?
                || receipt.fee_destination != lease.fee_destination()
                || receipt.prices != UsagePrices::from_schedule(lease.fee_schedule())
                || receipt.observation.root_program != lease.host_program()
                || (receipt.sequence == 1 && receipt.previous != GENESIS_RECEIPT)
                || (receipt.sequence > 1 && receipt.previous == GENESIS_RECEIPT)
                || receipt.transfer_root != sandbox_escrow_charge_root(
                    lease.host_program(), lease.namespace().execution_principal().map_err(UsageRefusal::Lease)?,
                    receipt.activity_id, lease.id().bytes(), receipt.expected_lease_digest,
                    lease.escrow_account(), lease.escrow_asset(), lease.fee_destination(), receipt.charged,
                ).map_err(|_| UsageRefusal::InvalidChain)?
                || (receipt.sequence == 1 && receipt.previous_accumulator_root != [0; 32])
                || self.accumulator_root != accumulator_root_for(
                    receipt.previous_accumulator_root, receipt.sequence, receipt.digest)? {
                return Err(UsageRefusal::InvalidChain);
            }
        } else if self.spent != 0 || self.accumulator_root != [0; 32] {
            return Err(UsageRefusal::InvalidChain);
        }
        Ok(())
    }

    fn verify_archive<'a, I>(
        &self, lease: &Lease, escrow: &Escrow, receipts: I,
    ) -> Result<(), UsageRefusal>
    where I: IntoIterator<Item = &'a UsageReceipt> {
        self.verify(lease, escrow)?;
        let mut count = 0u64;
        let mut spent = 0u128;
        let mut previous = GENESIS_RECEIPT;
        let mut accumulator = [0; 32];
        let mut cumulative = LeaseUsage::default();
        let mut prior_batch = lease.opened_at();
        let mut activity_ids = BTreeSet::new();
        let mut prior_lease_digest = lease.state_digest_for_usage(cumulative, 0)
            .map_err(UsageRefusal::Lease)?;
        for receipt in receipts {
            count = count.checked_add(1).ok_or(UsageRefusal::ArithmeticOverflow)?;
            receipt.verify()?;
            if receipt.sequence != count || receipt.lease != lease.id()
                || receipt.previous != previous
                || !activity_ids.insert(receipt.activity_id)
                || receipt.previous_accumulator_root != accumulator
                || receipt.lease_terms_digest != lease.request_binding_digest()
                    .map_err(UsageRefusal::Lease)?
                || receipt.expected_lease_digest != prior_lease_digest
                || receipt.resulting_lease_digest != lease.state_digest_for_usage(
                    receipt.cumulative, receipt.cumulative_spent).map_err(UsageRefusal::Lease)?
                || receipt.fee_destination != lease.fee_destination()
                || receipt.prices != UsagePrices::from_schedule(lease.fee_schedule())
                || receipt.observation.root_program != lease.host_program()
                || receipt.transfer_root != sandbox_escrow_charge_root(
                    lease.host_program(), lease.namespace().execution_principal()
                        .map_err(UsageRefusal::Lease)?, receipt.activity_id,
                    lease.id().bytes(), receipt.expected_lease_digest,
                    lease.escrow_account(), lease.escrow_asset(), lease.fee_destination(),
                    receipt.charged,
                ).map_err(|_| UsageRefusal::InvalidChain)?
                || receipt.cumulative_spent != spent.checked_add(receipt.charged)
                    .ok_or(UsageRefusal::ArithmeticOverflow)?
                || receipt.observed_batch < prior_batch
                || receipt.usage().occupancy_byte_batches != u128::from(cumulative.namespace_bytes)
                    .checked_mul(u128::from(receipt.observed_batch - prior_batch))
                    .ok_or(UsageRefusal::ArithmeticOverflow)?
                || !valid_cumulative(cumulative, receipt.usage(), receipt.cumulative) {
                return Err(UsageRefusal::InvalidChain);
            }
            spent = receipt.cumulative_spent;
            previous = receipt.digest;
            accumulator = accumulator_root_for(accumulator, count, receipt.digest)?;
            cumulative = receipt.cumulative;
            prior_batch = receipt.observed_batch;
            prior_lease_digest = receipt.resulting_lease_digest;
        }
        let expected_latest = if count == 0 { None } else { Some(previous) };
        if count != self.receipt_count || spent != self.spent
            || accumulator != self.accumulator_root || cumulative != lease.usage()
            || prior_lease_digest != lease.state_digest().map_err(UsageRefusal::Lease)?
            || self.latest.as_ref().map(UsageReceipt::digest) != expected_latest {
            return Err(UsageRefusal::ConservationViolation);
        }
        Ok(())
    }

    pub fn verify_authenticated_archive<'a, I>(
        &self, lease: &Lease, escrow: &Escrow, receipts: I,
    ) -> Result<(), UsageRefusal>
    where I: IntoIterator<Item = &'a AuthenticatedUsageReceipt> {
        self.verify_archive(lease, escrow,
            receipts.into_iter().map(AuthenticatedUsageReceipt::receipt))
    }
}

pub(crate) fn record_host_settlement(
    state: &DurableUsageState,
    observation: UsageObservation,
    cumulative_usage: LeaseUsage,
    observed_batch: u64,
    transfer_root: [u8; 32],
) -> Result<(DurableUsageState, UsageReceipt), UsageRefusal> {
    let lease = &state.lease;
    let escrow = &state.escrow;
    let ledger = &state.ledger;
    ledger.verify(lease, escrow)?;
    let activity_id = observation.activity_binding.bytes();
    if activity_id == [0; 32] || observation.root_program != lease.host_program()
        || transfer_root == [0; 32] || ledger.receipt_count >= MAX_USAGE_RECEIPTS {
        return Err(UsageRefusal::InvalidActivity);
    }
    let prior_batch = ledger.latest().map_or(lease.opened_at(), UsageReceipt::observed_batch);
    let elapsed = observed_batch.checked_sub(prior_batch).ok_or(UsageRefusal::InvalidActivity)?;
    let usage = observation.usage;
    if usage.occupancy_byte_batches != occupancy_byte_batches(lease.usage().namespace_bytes, elapsed)?
        || !valid_cumulative(lease.usage(), usage, cumulative_usage) {
        return Err(UsageRefusal::UsageMismatch);
    }
    let prices = UsagePrices::from_schedule(lease.fee_schedule());
    if usage.fee_units != execution_fee(usage, prices)?
        || usage.occupancy_fee_units != usage.occupancy_byte_batches
            .checked_mul(u128::from(prices.occupancy_byte_batch))
            .ok_or(UsageRefusal::ArithmeticOverflow)? {
        return Err(UsageRefusal::PriceMismatch);
    }
    let charged = usage.fee_units.checked_add(usage.occupancy_fee_units)
        .ok_or(UsageRefusal::ArithmeticOverflow)?;
    if charged == 0 { return Err(UsageRefusal::ZeroCharge); }
    let next_spent = ledger.spent.checked_add(charged).ok_or(UsageRefusal::ArithmeticOverflow)?;
    let mut next_lease = lease.clone();
    next_lease.record_usage(cumulative_usage, next_spent, observed_batch, None)
        .map_err(UsageRefusal::Lease)?;
    let next_escrow = escrow.projected_spend(lease, charged).map_err(UsageRefusal::Escrow)?;
    let sequence = ledger.receipt_count.checked_add(1).ok_or(UsageRefusal::ArithmeticOverflow)?;
    let mut receipt = UsageReceipt {
        lease: lease.id(), sequence, observed_batch, activity_id,
        lease_terms_digest: lease.request_binding_digest().map_err(UsageRefusal::Lease)?,
        expected_lease_digest: lease.state_digest().map_err(UsageRefusal::Lease)?,
        resulting_lease_digest: next_lease.state_digest().map_err(UsageRefusal::Lease)?,
        fee_destination: lease.fee_destination(),
        previous: ledger.latest().map_or(GENESIS_RECEIPT, UsageReceipt::digest),
        previous_accumulator_root: ledger.accumulator_root,
        observation, cumulative: cumulative_usage, prices, charged,
        cumulative_spent: next_spent, transfer_root, digest: [0; 32],
    };
    receipt.digest = receipt_digest(&receipt.canonical_bytes())?;
    receipt.verify()?;
    let next_root = accumulator_root_for(ledger.accumulator_root, sequence, receipt.digest)?;
    let next_ledger = UsageLedger { receipt_count: sequence, spent: next_spent,
        accumulator_root: next_root, latest: Some(receipt.clone()) };
    let next = DurableUsageState { lease: next_lease, escrow: next_escrow, ledger: next_ledger };
    next.ledger.verify(&next.lease, &next.escrow)?;
    Ok((next, receipt))
}

pub(crate) fn record_host_settlement_reserved(
    state: &mut DurableUsageState, observation: UsageObservation,
    cumulative_usage: LeaseUsage, observed_batch: u64, transfer_root: [u8; 32],
    lease_terms_digest: [u8; 32], expected_lease_digest: [u8; 32],
    lease_state: &mut Vec<u8>, receipt_bytes: &mut Vec<u8>,
) -> Result<UsageReceipt, UsageRefusal> {
    let activity_id = observation.activity_binding.bytes();
    let prior_batch = state.ledger.latest().map_or(
        state.lease.opened_at(), UsageReceipt::observed_batch);
    let elapsed = observed_batch.checked_sub(prior_batch).ok_or(UsageRefusal::InvalidActivity)?;
    let usage = observation.usage;
    let prices = UsagePrices::from_schedule(state.lease.fee_schedule());
    if activity_id == [0; 32] || transfer_root == [0; 32]
        || observation.root_program != state.lease.host_program()
        || state.ledger.receipt_count >= MAX_USAGE_RECEIPTS
        || lease_terms_digest == [0; 32] || expected_lease_digest == [0; 32]
        || usage.occupancy_byte_batches
            != occupancy_byte_batches(state.lease.usage().namespace_bytes, elapsed)?
        || !valid_cumulative(state.lease.usage(), usage, cumulative_usage)
        || usage.fee_units != execution_fee(usage, prices)?
        || usage.occupancy_fee_units != usage.occupancy_byte_batches
            .checked_mul(u128::from(prices.occupancy_byte_batch))
            .ok_or(UsageRefusal::ArithmeticOverflow)? {
        return Err(UsageRefusal::UsageMismatch);
    }
    let charged = usage.fee_units.checked_add(usage.occupancy_fee_units)
        .ok_or(UsageRefusal::ArithmeticOverflow)?;
    if charged == 0 { return Err(UsageRefusal::ZeroCharge); }
    let next_spent = state.ledger.spent.checked_add(charged)
        .ok_or(UsageRefusal::ArithmeticOverflow)?;
    state.lease.record_usage(cumulative_usage, next_spent, observed_batch, None)
        .map_err(UsageRefusal::Lease)?;
    state.escrow = state.escrow.projected_spend(&state.lease, charged)
        .map_err(UsageRefusal::Escrow)?;
    state.lease.write_canonical_state(lease_state).map_err(UsageRefusal::Lease)?;
    let resulting_lease_digest = hash_bytes(HashAlgorithm::Sha256, lease_state)
        .map_err(|_| UsageRefusal::HashRefusal)?;
    let sequence = state.ledger.receipt_count.checked_add(1)
        .ok_or(UsageRefusal::ArithmeticOverflow)?;
    let mut receipt = UsageReceipt { lease: state.lease.id(), sequence, observed_batch,
        activity_id, lease_terms_digest, expected_lease_digest, resulting_lease_digest,
        fee_destination: state.lease.fee_destination(),
        previous: state.ledger.latest().map_or(GENESIS_RECEIPT, UsageReceipt::digest),
        previous_accumulator_root: state.ledger.accumulator_root, observation,
        cumulative: cumulative_usage, prices, charged, cumulative_spent: next_spent,
        transfer_root, digest: [0; 32] };
    receipt.write_canonical(receipt_bytes);
    receipt.digest = receipt_digest(receipt_bytes)?;
    let next_root = accumulator_root_for(state.ledger.accumulator_root, sequence, receipt.digest)?;
    state.ledger = UsageLedger { receipt_count: sequence, spent: next_spent,
        accumulator_root: next_root, latest: Some(receipt.clone()) };
    Ok(receipt)
}

fn occupancy_byte_batches(prior_bytes: u64, elapsed_batches: u64) -> Result<u128, UsageRefusal> {
    u128::from(prior_bytes).checked_mul(u128::from(elapsed_batches))
        .ok_or(UsageRefusal::ArithmeticOverflow)
}

fn accumulator_root_for(
    previous: [u8; 32], sequence: u64, receipt: [u8; 32],
) -> Result<[u8; 32], UsageRefusal> {
    let mut bytes = [0u8; 160];
    let length = LEDGER_ACCUMULATOR_DOMAIN.len() + 72;
    if length > bytes.len() { return Err(UsageRefusal::HashRefusal); }
    let mut offset = 0;
    bytes[offset..offset + LEDGER_ACCUMULATOR_DOMAIN.len()]
        .copy_from_slice(LEDGER_ACCUMULATOR_DOMAIN);
    offset += LEDGER_ACCUMULATOR_DOMAIN.len();
    bytes[offset..offset + 32].copy_from_slice(&previous); offset += 32;
    bytes[offset..offset + 8].copy_from_slice(&sequence.to_be_bytes()); offset += 8;
    bytes[offset..offset + 32].copy_from_slice(&receipt); offset += 32;
    hash_bytes(HashAlgorithm::Sha256, &bytes[..offset]).map_err(|_| UsageRefusal::HashRefusal)
}

fn receipt_digest(bytes: &[u8]) -> Result<[u8; 32], UsageRefusal> {
    hash_bytes(HashAlgorithm::Sha256, bytes).map_err(|_| UsageRefusal::HashRefusal)
}

fn execution_fee(usage: MeteredUsage, prices: UsagePrices) -> Result<u128, UsageRefusal> {
    [
        (usage.cpu_fuel, prices.cpu), (usage.memory_bytes, prices.memory),
        (usage.storage_read_bytes, prices.storage_read),
        (usage.storage_write_bytes, prices.storage_write),
        (u64::from(usage.output_values), prices.output_values),
        (usage.output_bytes, prices.output_bytes),
    ].into_iter().try_fold(0u128, |total, (units, price)| {
        total.checked_add(u128::from(units).checked_mul(u128::from(price))
            .ok_or(UsageRefusal::ArithmeticOverflow)?)
            .ok_or(UsageRefusal::ArithmeticOverflow)
    })
}

fn valid_cumulative(prior: LeaseUsage, activity: MeteredUsage, next: LeaseUsage) -> bool {
    prior.cpu_fuel.checked_add(activity.cpu_fuel) == Some(next.cpu_fuel)
        && next.memory_bytes == prior.memory_bytes.max(activity.memory_bytes)
        && prior.storage_read_bytes.checked_add(activity.storage_read_bytes) == Some(next.storage_read_bytes)
        && prior.storage_write_bytes.checked_add(activity.storage_write_bytes) == Some(next.storage_write_bytes)
        && prior.output_values.checked_add(u64::from(activity.output_values)) == Some(next.output_values)
        && prior.output_bytes.checked_add(activity.output_bytes) == Some(next.output_bytes)
        && next.table_elements == prior.table_elements
}

fn encode_metered(bytes: &mut Vec<u8>, usage: MeteredUsage) {
    for value in [usage.cpu_fuel, usage.memory_bytes, usage.storage_read_bytes,
        usage.storage_write_bytes, u64::from(usage.output_values), usage.output_bytes] {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    for value in [usage.occupancy_byte_batches, usage.occupancy_fee_units, usage.fee_units] {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
}

fn encode_lease_usage(bytes: &mut Vec<u8>, usage: LeaseUsage) {
    for value in [usage.cpu_fuel, usage.memory_bytes, usage.storage_read_bytes,
        usage.storage_write_bytes, usage.output_values, usage.output_bytes,
        usage.table_elements, usage.namespace_bytes] {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
}

fn encode_prices(bytes: &mut Vec<u8>, prices: UsagePrices) {
    bytes.extend_from_slice(&prices.schedule_version.to_be_bytes());
    for value in [prices.cpu, prices.memory, prices.storage_read, prices.storage_write,
        prices.output_values, prices.output_bytes, prices.occupancy_byte_batch] {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UsageRefusal {
    InvalidActivity, InvalidReceipt, InvalidChain, ReceiptLimit, ZeroCharge, UsageMismatch, PriceMismatch, ArithmeticOverflow,
    ConservationViolation, MissingSettlement, CanonicalStateCas, CanonicalStateAbsent, HashRefusal,
    Lease(LeaseRefusal), Escrow(EscrowRefusal),
}

impl Display for UsageRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result { write!(formatter, "{self:?}") }
}

impl std::error::Error for UsageRefusal {}

struct Cursor<'a> { remaining: &'a [u8] }
impl<'a> Cursor<'a> {
    const fn new(remaining: &'a [u8]) -> Self { Self { remaining } }
    fn take(&mut self, length: usize) -> Result<&'a [u8], UsageRefusal> {
        let (value, rest) = self.remaining.split_at_checked(length)
            .ok_or(UsageRefusal::InvalidReceipt)?;
        self.remaining = rest; Ok(value)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], UsageRefusal> {
        self.take(N)?.try_into().map_err(|_| UsageRefusal::InvalidReceipt)
    }
    fn u8(&mut self) -> Result<u8, UsageRefusal> { Ok(self.array::<1>()?[0]) }
    fn u32(&mut self) -> Result<u32, UsageRefusal> { Ok(u32::from_be_bytes(self.array()?)) }
    fn u64(&mut self) -> Result<u64, UsageRefusal> { Ok(u64::from_be_bytes(self.array()?)) }
    fn u128(&mut self) -> Result<u128, UsageRefusal> { Ok(u128::from_be_bytes(self.array()?)) }
    const fn is_empty(&self) -> bool { self.remaining.is_empty() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_encoding_keeps_failed_work_distinct() {
        assert_ne!(ActivityOutcome::Success as u8, ActivityOutcome::ProgramFailure as u8);
        assert_ne!(ActivityOutcome::ProgramFailure as u8, ActivityOutcome::ResourceExhaustion as u8);
    }

    #[test]
    fn long_lease_receipt_chain_conserves_every_charge() {
        let lease = crate::LeaseId::new([1; 32]).expect("lease");
        let prices = UsagePrices { schedule_version: 1, cpu: 1, memory: 1,
            storage_read: 1, storage_write: 1, output_values: 1, output_bytes: 1,
            occupancy_byte_batch: 1 };
        let mut previous = GENESIS_RECEIPT;
        let mut previous_accumulator_root = [0; 32];
        let mut total = 0u128;
        let mut cpu = 0u64;
        for sequence in 1u64..=10_000 {
            cpu = cpu.checked_add(sequence).expect("cpu");
            total = total.checked_add(u128::from(sequence)).expect("spent");
            let usage = MeteredUsage { cpu_fuel: sequence, memory_bytes: 0,
                storage_read_bytes: 0, storage_write_bytes: 0, output_values: 0,
                output_bytes: 0, occupancy_byte_batches: 0, occupancy_fee_units: 0,
                fee_units: u128::from(sequence) };
            let mut receipt = UsageReceipt { lease, sequence, observed_batch: sequence,
                activity_id: [2; 32], lease_terms_digest: [4; 32],
                expected_lease_digest: [6; 32], resulting_lease_digest: [7; 32],
                fee_destination: [5; 32], previous, previous_accumulator_root,
                observation: UsageObservation::success(
                    ProgramId::new([9; 32]).expect("program"),
                    ActivityBudgetBinding::new([2; 32]).expect("binding"), usage),
                cumulative: LeaseUsage { cpu_fuel: cpu, ..LeaseUsage::default() }, prices,
                charged: u128::from(sequence), cumulative_spent: total,
                transfer_root: [3; 32], digest: [0; 32] };
            receipt.digest = receipt_digest(&receipt.canonical_bytes()).expect("digest");
            let decoded = UsageReceipt::decode(&receipt.canonical_bytes(), receipt.digest).expect("decode");
            assert_eq!(decoded, receipt);
            assert_eq!(decoded.cumulative_spent(), total);
            previous = receipt.digest();
            previous_accumulator_root = accumulator_root_for(
                previous_accumulator_root, sequence, previous).expect("accumulator");
        }
        assert_eq!(total, 50_005_000);
    }

    #[test]
    fn deletion_changes_future_occupancy_without_erasing_elapsed_charge() {
        assert_eq!(occupancy_byte_batches(100, 3), Ok(300));
        assert!(valid_cumulative(
            LeaseUsage { namespace_bytes: 100, ..LeaseUsage::default() },
            MeteredUsage { cpu_fuel: 1, memory_bytes: 0, storage_read_bytes: 0,
                storage_write_bytes: 0, output_values: 0, output_bytes: 0,
                occupancy_byte_batches: 300, occupancy_fee_units: 300, fee_units: 1 },
            LeaseUsage { cpu_fuel: 1, namespace_bytes: 25, ..LeaseUsage::default() },
        ));
        assert_eq!(occupancy_byte_batches(25, 2), Ok(50));
    }

    #[test]
    fn accumulator_binds_sequence_and_receipt_without_monolithic_history() {
        let first = accumulator_root_for([0; 32], 1, [7; 32]).expect("first");
        assert_ne!(first, accumulator_root_for([0; 32], 2, [7; 32]).expect("sequence"));
        assert_ne!(first, accumulator_root_for([0; 32], 1, [8; 32]).expect("receipt"));
    }
}
