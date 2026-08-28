use layerx_programs_runtime::{
    reserve_host_sandbox_escrow_charge, settle_reserved_host_sandbox_escrow_charge,
    ActivityBudgetBinding, MeteredUsage, PrincipalId, ProgramId, ReservedSandboxEscrowCharge,
};
use std::cell::RefCell;

use crate::usage::record_host_settlement_reserved;
use crate::{ActivityOutcome, DurableUsageState, Escrow, Lease, LeaseUsage, UsageLedger, UsageObservation};

const OK: i32 = 0;
const NON_CANONICAL: i32 = -3;

struct HostSettlementReservation {
    token: u64,
    state: DurableUsageState,
    transfer: ReservedSandboxEscrowCharge,
    lease_state: Vec<u8>,
    escrow_state: Vec<u8>,
    ledger_state: Vec<u8>,
    receipt: Vec<u8>,
    lease_terms_digest: [u8; 32],
    expected_lease_digest: [u8; 32],
}

thread_local! {
    static HOST_SETTLEMENT_RESERVATION: RefCell<Option<HostSettlementReservation>> =
        const { RefCell::new(None) };
}

fn admitted_charge_fits(remaining: u128, occupancy: u128, maximum_execution: u128) -> bool {
    maximum_execution != 0
        && maximum_execution.checked_add(occupancy).is_some_and(|total| total <= remaining)
}

unsafe extern "C" {
    fn layerx_programs_call_sandbox_context_byte(token: u64, section: u16, offset: u32) -> i32;
    fn layerx_programs_call_sandbox_expected_sequence(token: u64, expected: u64) -> i32;
    fn layerx_programs_call_sandbox_fee_schedule(
        token: u64, version: u32, cpu: u64, memory: u64, read: u64, write: u64,
        output_values: u64, output_bytes: u64, occupancy: u64,
    ) -> i32;
    fn layerx_programs_call_sandbox_state_length(token: u64, kind: u16) -> i32;
    fn layerx_programs_call_sandbox_state_byte(token: u64, kind: u16, offset: u32) -> i32;
    fn layerx_programs_call_sandbox_state_stage_begin(token: u64, kind: u16, length: u32) -> i32;
    fn layerx_programs_call_sandbox_state_stage_byte(
        token: u64, kind: u16, offset: u32, byte: u8,
    ) -> i32;
    fn layerx_programs_call_sandbox_state_stage_apply(token: u64, kind: u16) -> i32;
    fn layerx_programs_call_sandbox_usage_result_begin(
        token: u64, occupancy_hi: u64, occupancy_lo: u64,
        occupancy_fee_hi: u64, occupancy_fee_lo: u64,
        transfer0: u64, transfer1: u64, transfer2: u64, transfer3: u64,
        receipt_length: u32,
    ) -> i32;
    fn layerx_programs_call_sandbox_usage_result_receipt_byte(
        token: u64, offset: u32, byte: u8,
    ) -> i32;
    fn layerx_programs_call_sandbox_usage_result_field(token: u64, index: u16, value: u64) -> i32;
    fn layerx_programs_call_sandbox_usage_result_publish(token: u64) -> i32;
    fn layerx_programs_sandbox_lifecycle_length(token: u64, section: u16) -> i32;
    fn layerx_programs_sandbox_lifecycle_byte(token: u64, section: u16, offset: u32) -> i32;
}

fn read_state(token: u64, kind: u16) -> Result<Vec<u8>, i32> {
    let length = unsafe { layerx_programs_call_sandbox_state_length(token, kind) };
    let length = usize::try_from(length).map_err(|_| NON_CANONICAL)?;
    if length == 0 { return Err(NON_CANONICAL); }
    (0..length).map(|offset| {
        let value = unsafe { layerx_programs_call_sandbox_state_byte(
            token, kind, u32::try_from(offset).map_err(|_| NON_CANONICAL)?,
        ) };
        u8::try_from(value).map_err(|_| NON_CANONICAL)
    }).collect()
}

fn context_field(token: u64, section: u16) -> Result<[u8; 32], i32> {
    let mut bytes = [0u8; 32];
    for (offset, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::try_from(unsafe { layerx_programs_call_sandbox_context_byte(
            token, section, u32::try_from(offset).map_err(|_| NON_CANONICAL)?,
        ) }).map_err(|_| NON_CANONICAL)?;
    }
    Ok(bytes)
}

fn lifecycle_field(token: u64, section: u16) -> Result<Vec<u8>, i32> {
    let length = unsafe { layerx_programs_sandbox_lifecycle_length(token, section) };
    let length = usize::try_from(length).map_err(|_| NON_CANONICAL)?;
    (0..length).map(|offset| u8::try_from(unsafe {
        layerx_programs_sandbox_lifecycle_byte(
            token, section, u32::try_from(offset).map_err(|_| NON_CANONICAL)?,
        )
    }).map_err(|_| NON_CANONICAL)).collect()
}

fn stage(token: u64, kind: u16, bytes: &[u8]) -> Result<(), i32> {
    let length = u32::try_from(bytes.len()).map_err(|_| NON_CANONICAL)?;
    if unsafe { layerx_programs_call_sandbox_state_stage_begin(token, kind, length) } != OK {
        return Err(NON_CANONICAL);
    }
    for (offset, byte) in bytes.iter().copied().enumerate() {
        if unsafe { layerx_programs_call_sandbox_state_stage_byte(
            token, kind, u32::try_from(offset).map_err(|_| NON_CANONICAL)?, byte,
        ) } != OK { return Err(NON_CANONICAL); }
    }
    if unsafe { layerx_programs_call_sandbox_state_stage_apply(token, kind) } != OK {
        return Err(NON_CANONICAL);
    }
    Ok(())
}

fn words(bytes: [u8; 32]) -> [u64; 4] {
    core::array::from_fn(|index| {
        let mut word = [0; 8];
        word.copy_from_slice(&bytes[index * 8..index * 8 + 8]);
        u64::from_be_bytes(word)
    })
}

#[no_mangle]
pub extern "C" fn layerx_programs_sandbox_admit_host(
    token: u64, observed_batch: u64, maximum_fee_hi: u64, maximum_fee_lo: u64,
) -> i32 {
    let admitted = (|| -> Result<(), i32> {
        let lease = Lease::decode_state(&read_state(token, 0)?).map_err(|_| NON_CANONICAL)?;
        let escrow = Escrow::decode_state(&lease, &read_state(token, 1)?)
            .map_err(|_| NON_CANONICAL)?;
        let ledger = UsageLedger::decode_state(&read_state(token, 2)?, &lease, &escrow)
            .map_err(|_| NON_CANONICAL)?;
        if context_field(token, 0)? != lease.id().bytes()
            || context_field(token, 1)? != lease.escrow_account()
            || context_field(token, 2)? != lease.escrow_asset()
            || context_field(token, 3)? != lease.fee_destination()
            || context_field(token, 4)? != lease.state_digest().map_err(|_| NON_CANONICAL)? {
            return Err(NON_CANONICAL);
        }
        let schedule = lease.fee_schedule();
        if unsafe { layerx_programs_call_sandbox_fee_schedule(token, schedule.version(),
            schedule.cpu_price(), schedule.memory_byte_price(),
            schedule.storage_read_byte_price(), schedule.storage_write_byte_price(),
            schedule.output_value_price(), schedule.output_byte_price(),
            schedule.occupancy_byte_batch_price()) } != OK {
            return Err(NON_CANONICAL);
        }
        let prior_batch = ledger.latest().map_or(lease.opened_at(), crate::UsageReceipt::observed_batch);
        let elapsed = observed_batch.checked_sub(prior_batch).ok_or(NON_CANONICAL)?;
        let occupancy = u128::from(lease.usage().namespace_bytes)
            .checked_mul(u128::from(elapsed))
            .and_then(|units| units.checked_mul(u128::from(schedule.occupancy_byte_batch_price())))
            .ok_or(NON_CANONICAL)?;
        let maximum_execution = (u128::from(maximum_fee_hi) << 64)
            | u128::from(maximum_fee_lo);
        let remaining = escrow.funded().checked_sub(escrow.spent())
            .and_then(|value| value.checked_sub(escrow.refunded()))
            .ok_or(NON_CANONICAL)?;
        if !admitted_charge_fits(remaining, occupancy, maximum_execution) {
            return Err(NON_CANONICAL);
        }
        let activity_id: [u8; 32] = lifecycle_field(token, 8)?.try_into()
            .map_err(|_| NON_CANONICAL)?;
        let principal = lease.namespace().execution_principal().map_err(|_| NON_CANONICAL)?;
        let expected_lease_digest = lease.state_digest().map_err(|_| NON_CANONICAL)?;
        let lease_terms_digest = lease.request_binding_digest().map_err(|_| NON_CANONICAL)?;
        let transfer = reserve_host_sandbox_escrow_charge(
            lease.host_program(), principal, activity_id, lease.id().bytes(),
            expected_lease_digest, lease.escrow_account(),
            lease.escrow_asset(), lease.fee_destination(), maximum_execution,
        ).map_err(|_| NON_CANONICAL)?;
        HOST_SETTLEMENT_RESERVATION.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot.is_some() { return Err(NON_CANONICAL); }
            *slot = Some(HostSettlementReservation { token,
                state: DurableUsageState { lease, escrow, ledger }, transfer,
                lease_state: Vec::with_capacity(crate::MAX_USAGE_STATE_VALUE_BYTES),
                escrow_state: Vec::with_capacity(crate::MAX_USAGE_STATE_VALUE_BYTES),
                ledger_state: Vec::with_capacity(crate::MAX_USAGE_STATE_VALUE_BYTES),
                receipt: Vec::with_capacity(4096),
                lease_terms_digest, expected_lease_digest,
            });
            Ok(())
        })?;
        Ok(())
    })();
    admitted.map_or_else(|error| error, |()| OK)
}

#[no_mangle]
pub extern "C" fn layerx_programs_sandbox_cancel_host(token: u64) {
    HOST_SETTLEMENT_RESERVATION.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.as_ref().is_some_and(|reservation| reservation.token == token) {
            *slot = None;
        }
    });
}

#[cfg(test)]
mod admission_cases {
    use super::admitted_charge_fits;

    #[test]
    fn reserve_includes_accrued_occupancy_and_full_admitted_execution() {
        assert!(admitted_charge_fits(100, 20, 80));
        assert!(!admitted_charge_fits(99, 20, 80));
        assert!(!admitted_charge_fits(u128::MAX, 1, u128::MAX));
        assert!(!admitted_charge_fits(100, 20, 0));
    }
}

#[allow(clippy::too_many_arguments)]
fn settle_call(
    token: u64, outcome: u8, observed_batch: u64, host_words: [u64; 4],
    binding_words: [u64; 4], cpu: u64, memory: u64, storage_read: u64,
    storage_write: u64, output_values: u32, output_bytes: u64,
    final_namespace_bytes: u64,
) -> Result<(), i32> {
    let host = ProgramId::new(words_to_bytes(host_words)).map_err(|_| NON_CANONICAL)?;
    let binding = ActivityBudgetBinding::new(words_to_bytes(binding_words))
        .map_err(|_| NON_CANONICAL)?;
    let mut reservation = HOST_SETTLEMENT_RESERVATION.with(|slot|
        slot.borrow_mut().take()).ok_or(NON_CANONICAL)?;
    if reservation.token != token { return Err(NON_CANONICAL); }
    let lease = &reservation.state.lease;
    let escrow = &reservation.state.escrow;
    let ledger = &reservation.state.ledger;
    if lease.host_program() != host || context_field(token, 0)? != lease.id().bytes()
        || context_field(token, 1)? != lease.escrow_account()
        || context_field(token, 2)? != lease.escrow_asset()
        || context_field(token, 3)? != lease.fee_destination()
        || context_field(token, 4)? != reservation.expected_lease_digest {
        return Err(NON_CANONICAL);
    }
    let expected_sequence = ledger.receipt_count().checked_add(1).ok_or(NON_CANONICAL)?;
    if unsafe { layerx_programs_call_sandbox_expected_sequence(token, expected_sequence) } != OK {
        return Err(NON_CANONICAL);
    }
    let schedule = lease.fee_schedule();
    if unsafe { layerx_programs_call_sandbox_fee_schedule(token, schedule.version(),
        schedule.cpu_price(), schedule.memory_byte_price(), schedule.storage_read_byte_price(),
        schedule.storage_write_byte_price(), schedule.output_value_price(),
        schedule.output_byte_price(), schedule.occupancy_byte_batch_price()) } != OK {
        return Err(NON_CANONICAL);
    }
    let prior_batch = ledger.latest().map_or(lease.opened_at(), crate::UsageReceipt::observed_batch);
    let elapsed = observed_batch.checked_sub(prior_batch).ok_or(NON_CANONICAL)?;
    let occupancy_byte_batches = u128::from(lease.usage().namespace_bytes)
        .checked_mul(u128::from(elapsed)).ok_or(NON_CANONICAL)?;
    let occupancy_fee_units = occupancy_byte_batches
        .checked_mul(u128::from(schedule.occupancy_byte_batch_price())).ok_or(NON_CANONICAL)?;
    let execution_fee = [
        (cpu, schedule.cpu_price()), (memory, schedule.memory_byte_price()),
        (storage_read, schedule.storage_read_byte_price()),
        (storage_write, schedule.storage_write_byte_price()),
        (u64::from(output_values), schedule.output_value_price()),
        (output_bytes, schedule.output_byte_price()),
    ].into_iter().try_fold(0u128, |total, (units, price)| total
        .checked_add(u128::from(units).checked_mul(u128::from(price)).ok_or(NON_CANONICAL)?)
        .ok_or(NON_CANONICAL))?;
    let usage = MeteredUsage { cpu_fuel: cpu, memory_bytes: memory,
        storage_read_bytes: storage_read, storage_write_bytes: storage_write,
        output_values, output_bytes, occupancy_byte_batches, occupancy_fee_units,
        fee_units: execution_fee };
    let cumulative = LeaseUsage {
        cpu_fuel: lease.usage().cpu_fuel.checked_add(cpu).ok_or(NON_CANONICAL)?,
        memory_bytes: lease.usage().memory_bytes.max(memory),
        storage_read_bytes: lease.usage().storage_read_bytes.checked_add(storage_read).ok_or(NON_CANONICAL)?,
        storage_write_bytes: lease.usage().storage_write_bytes.checked_add(storage_write).ok_or(NON_CANONICAL)?,
        output_values: lease.usage().output_values.checked_add(u64::from(output_values)).ok_or(NON_CANONICAL)?,
        output_bytes: lease.usage().output_bytes.checked_add(output_bytes).ok_or(NON_CANONICAL)?,
        table_elements: lease.usage().table_elements,
        namespace_bytes: if outcome == 1 { final_namespace_bytes } else { lease.usage().namespace_bytes },
    };
    let execution_principal = lease.namespace().execution_principal().map_err(|_| NON_CANONICAL)?;
    let exact_fee = execution_fee.checked_add(occupancy_fee_units).ok_or(NON_CANONICAL)?;
    escrow.permits_execution(&lease, exact_fee).map_err(|_| NON_CANONICAL)?;
    let settlement = settle_reserved_host_sandbox_escrow_charge(
        token, &mut reservation.transfer, exact_fee)
        .map_err(|_| NON_CANONICAL)?;
    let observation = UsageObservation::host_sealed(match outcome {
        1 => ActivityOutcome::Success, 2 => ActivityOutcome::ProgramFailure,
        3 => ActivityOutcome::ResourceExhaustion, _ => return Err(NON_CANONICAL),
    }, host, binding, usage);
    let HostSettlementReservation { state, lease_state, escrow_state,
        ledger_state, receipt: receipt_bytes, lease_terms_digest,
        expected_lease_digest, .. } = &mut reservation;
    let receipt = record_host_settlement_reserved(state, observation,
        cumulative, observed_batch, settlement.transfer_set_root(),
        *lease_terms_digest, *expected_lease_digest, lease_state, receipt_bytes)
        .map_err(|_| NON_CANONICAL)?;
    state.escrow.write_canonical_state(escrow_state);
    state.ledger.write_canonical_state(ledger_state)
        .map_err(|_| NON_CANONICAL)?;
    receipt_bytes.extend_from_slice(&receipt.digest());
    stage(token, 0, lease_state)?;
    stage(token, 1, escrow_state)?;
    stage(token, 2, ledger_state)?;
    let root = words(settlement.transfer_set_root());
    if unsafe { layerx_programs_call_sandbox_usage_result_begin(token,
        (occupancy_byte_batches >> 64) as u64, occupancy_byte_batches as u64,
        (occupancy_fee_units >> 64) as u64, occupancy_fee_units as u64,
        root[0], root[1], root[2], root[3],
        u32::try_from(receipt_bytes.len()).map_err(|_| NON_CANONICAL)?) } != OK {
        return Err(NON_CANONICAL);
    }
    for (offset, byte) in receipt_bytes.iter().copied().enumerate() {
        if unsafe { layerx_programs_call_sandbox_usage_result_receipt_byte(
            token, u32::try_from(offset).map_err(|_| NON_CANONICAL)?, byte,
        ) } != OK { return Err(NON_CANONICAL); }
    }
    let fee_hi = u64::try_from(execution_fee >> 64).map_err(|_| NON_CANONICAL)?;
    let fee_lo = execution_fee as u64;
    for (index, value) in [cpu, memory, storage_read, storage_write,
        u64::from(output_values), output_bytes, fee_hi, fee_lo].into_iter().enumerate() {
        if unsafe { layerx_programs_call_sandbox_usage_result_field(
            token, u16::try_from(index).map_err(|_| NON_CANONICAL)?, value,
        ) } != OK { return Err(NON_CANONICAL); }
    }
    if unsafe { layerx_programs_call_sandbox_usage_result_publish(token) } != OK {
        return Err(NON_CANONICAL);
    }
    Ok(())
}

const fn words_to_bytes(words: [u64; 4]) -> [u8; 32] {
    let mut bytes = [0; 32];
    let mut index = 0;
    while index < 4 {
        let word = words[index].to_be_bytes();
        let mut offset = 0;
        while offset < 8 { bytes[index * 8 + offset] = word[offset]; offset += 1; }
        index += 1;
    }
    bytes
}

#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn layerx_programs_sandbox_settle_call_rust(
    token: u64, outcome: u8, observed_batch: u64,
    h0: u64, h1: u64, h2: u64, h3: u64,
    b0: u64, b1: u64, b2: u64, b3: u64,
    cpu: u64, memory: u64, storage_read: u64, storage_write: u64,
    output_values: u32, output_bytes: u64, final_namespace_bytes: u64,
) -> i32 {
    std::panic::catch_unwind(|| settle_call(token, outcome, observed_batch,
        [h0, h1, h2, h3], [b0, b1, b2, b3], cpu, memory, storage_read,
        storage_write, output_values, output_bytes, final_namespace_bytes))
        .ok().and_then(Result::ok).map_or(NON_CANONICAL, |()| OK)
}

fn validate_lifecycle(token: u64, operation: u8) -> Result<(), i32> {
    if operation == 2 || operation == 0x82 {
        let mut lease = Lease::decode_state(&lifecycle_field(token, 0)?)
            .map_err(|_| NON_CANONICAL)?;
        let tenant: [u8; 32] = lifecycle_field(token, 3)?.try_into().map_err(|_| NON_CANONICAL)?;
        let host: [u8; 32] = lifecycle_field(token, 4)?.try_into().map_err(|_| NON_CANONICAL)?;
        let amount: [u8; 16] = lifecycle_field(token, 5)?.try_into().map_err(|_| NON_CANONICAL)?;
        let expiry: [u8; 8] = lifecycle_field(token, 6)?.try_into().map_err(|_| NON_CANONICAL)?;
        if lease.state() != crate::LeaseState::Requested || lease.tenant().bytes() != tenant
            || lease.host_program().bytes() != host || lease.escrow_amount() != u128::from_be_bytes(amount)
            || lease.expiry() != u64::from_be_bytes(expiry) {
            return Err(NON_CANONICAL);
        }
        if operation == 0x82 {
            let funding_root: [u8; 32] = lifecycle_field(token, 7)?.try_into()
                .map_err(|_| NON_CANONICAL)?;
            let activity_id: [u8; 32] = lifecycle_field(token, 8)?.try_into()
                .map_err(|_| NON_CANONICAL)?;
            let batch = u64::from_be_bytes(lifecycle_field(token, 10)?.try_into()
                .map_err(|_| NON_CANONICAL)?);
            lease.apply_host_activity(crate::LeaseActivity::Fund, activity_id, batch)
                .map_err(|_| NON_CANONICAL)?;
            let escrow = Escrow::funded_genesis(&lease, funding_root).map_err(|_| NON_CANONICAL)?;
            let ledger = UsageLedger::new();
            stage(token, 0, &lease.canonical_state_bytes().map_err(|_| NON_CANONICAL)?)?;
            stage(token, 1, &escrow.canonical_state())?;
            stage(token, 2, &ledger.canonical_state().map_err(|_| NON_CANONICAL)?)?;
        }
    } else if operation == 3 {
        let current = Lease::decode_state(&lifecycle_field(token, 13)?)
            .map_err(|_| NON_CANONICAL)?;
        let expected_digest: [u8; 32] = lifecycle_field(token, 11)?.try_into()
            .map_err(|_| NON_CANONICAL)?;
        let expected_sequence = u64::from_be_bytes(lifecycle_field(token, 12)?.try_into()
            .map_err(|_| NON_CANONICAL)?);
        let activity_id: [u8; 32] = lifecycle_field(token, 8)?.try_into()
            .map_err(|_| NON_CANONICAL)?;
        let batch = u64::from_be_bytes(lifecycle_field(token, 10)?.try_into()
            .map_err(|_| NON_CANONICAL)?);
        if current.state_digest().map_err(|_| NON_CANONICAL)? != expected_digest
            || expected_sequence != u64::try_from(current.history().len()).map_err(|_| NON_CANONICAL)?
                .checked_add(1).ok_or(NON_CANONICAL)?
            || current.state() != crate::LeaseState::Funded {
            return Err(NON_CANONICAL);
        }
        let mut expected = current;
        expected.apply_host_activity(crate::LeaseActivity::Activate, activity_id, batch)
            .map_err(|_| NON_CANONICAL)?;
        stage(token, 0, &expected.canonical_state_bytes().map_err(|_| NON_CANONICAL)?)?;
    } else { return Err(NON_CANONICAL); }
    Ok(())
}

#[no_mangle]
pub extern "C" fn layerx_programs_sandbox_lifecycle_validate(token: u64, operation: u8) -> i32 {
    std::panic::catch_unwind(|| validate_lifecycle(token, operation))
        .ok().and_then(Result::ok).map_or(NON_CANONICAL, |()| OK)
}
