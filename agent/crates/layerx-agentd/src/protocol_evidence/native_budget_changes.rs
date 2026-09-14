use super::{EvidenceAuthority, RawActivityReceiptEvidence, VerifiedCumulativeReceipt};
use crate::budget::{
    NativeBudgetBinding, NativeBudgetError as Error, NativeBudgetOutcome,
    NativeBudgetRecoveryEvidence,
};
use layerx_proof::receipt::{
    verify_native_owner_outcome, AuthorizedBatch, NativeOwnerOutcomeContext,
};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_wire::activity::{decode_signed, encode_signed, Activity};
use layerx_wire::hash::activity_id;
use layerx_wire::native_budget::BudgetRecord;
use layerx_wire::receipt::{decode, decode_batch_header, ProtocolReceipt};

fn rollover(record: &mut BudgetRecord, timestamp: u64) -> Result<(), Error> {
    let periods = timestamp
        .checked_sub(record.period_start)
        .ok_or(Error::Window)?
        / record.period_length;
    if periods == 0 {
        return Ok(());
    }
    let configured = if record.configured_limit == 0 {
        record.limit
    } else {
        record.configured_limit
    };
    let unspent = record
        .limit
        .checked_sub(record.spent)
        .ok_or(Error::Consumption)?;
    record.carried = if record.rollover_policy == 2 {
        unspent.min(record.carry_cap)
    } else {
        0
    };
    record.limit = configured
        .checked_add(record.carried)
        .ok_or(Error::Arithmetic)?;
    record.period_start = record
        .period_start
        .checked_add(
            periods
                .checked_mul(record.period_length)
                .ok_or(Error::Arithmetic)?,
        )
        .ok_or(Error::Arithmetic)?;
    record.spent = 0;
    Ok(())
}

fn budget_activity(
    entry: &RawActivityReceiptEvidence,
    receipt: &VerifiedCumulativeReceipt,
) -> Result<Option<Activity>, Error> {
    if receipt.module_id() != 3 {
        return Ok(None);
    }
    let kinds = (1..=7)
        .map(|ordinal| ActivityType::new(ModuleId::Budget, ordinal).map_err(|_| Error::Activity))
        .collect::<Result<Vec<_>, _>>()?;
    let registration =
        ModuleRegistration::new(ModuleId::Budget, &kinds).map_err(|_| Error::Activity)?;
    let registry = ModuleRegistry::new(&[registration]).map_err(|_| Error::Activity)?;
    let activity =
        decode_signed(entry.canonical_activity(), &registry).map_err(|_| Error::Activity)?;
    if encode_signed(&activity).map_err(|_| Error::Activity)? != entry.canonical_activity()
        || activity_id(&activity).map_err(|_| Error::Activity)? != receipt.activity_id()
        || activity.protocol_version() != receipt.header().protocol_version()
        || activity.network_id() != receipt.header().network_id()
    {
        return Err(Error::Activity);
    }
    Ok(Some(activity))
}

fn event(receipt: &ProtocolReceipt, ordinal: u16, expected: &[u8]) -> Result<(), Error> {
    let mut matching = receipt
        .effects()
        .iter()
        .filter(|value| value.module_id() == 3 && value.event_type() == ordinal);
    let effect = matching.next().ok_or(Error::Receipt)?;
    if matching.next().is_some()
        || effect.kind() != 3
        || effect.monetary()
        || effect.body() != expected
    {
        return Err(Error::Receipt);
    }
    Ok(())
}

fn apply_amend(
    record: &mut BudgetRecord,
    activity: &Activity,
    receipt: &ProtocolReceipt,
) -> Result<(), Error> {
    let payload = activity.payload();
    if payload.len() != 75 || payload[..2] != [0, 1] || record.closed || record.revoked {
        return Err(Error::Activity);
    }
    let configured = u128::from_be_bytes(payload[34..50].try_into().map_err(|_| Error::Activity)?);
    let carry_cap = u128::from_be_bytes(payload[50..66].try_into().map_err(|_| Error::Activity)?);
    let expiry = u64::from_be_bytes(payload[66..74].try_into().map_err(|_| Error::Activity)?);
    let policy = payload[74];
    let limit = configured
        .checked_add(record.carried)
        .ok_or(Error::Arithmetic)?;
    if configured == 0
        || limit < record.spent
        || expiry <= record.period_start
        || !matches!(policy, 1 | 2)
        || (policy == 1 && carry_cap != 0)
    {
        return Err(Error::Record);
    }
    let mut expected = record.id.to_vec();
    expected.extend_from_slice(&limit.to_be_bytes());
    expected.extend_from_slice(&expiry.to_be_bytes());
    event(receipt, 3, &expected)?;
    record.configured_limit = configured;
    record.limit = limit;
    record.carry_cap = carry_cap;
    record.expiry = expiry;
    record.rollover_policy = policy;
    Ok(())
}

fn apply_close(
    record: &mut BudgetRecord,
    activity: &Activity,
    receipt: &ProtocolReceipt,
) -> Result<(), Error> {
    let payload = activity.payload();
    if payload.len() != 42 || payload[..2] != [0, 1] || record.closed {
        return Err(Error::Activity);
    }
    let revocation = u64::from_be_bytes(payload[34..42].try_into().map_err(|_| Error::Activity)?);
    if revocation <= record.revocation {
        return Err(Error::Record);
    }
    let mut matching = receipt
        .effects()
        .iter()
        .filter(|value| value.module_id() == 3 && value.event_type() == 7);
    let effect = matching.next().ok_or(Error::Receipt)?;
    if matching.next().is_some()
        || effect.kind() != 3
        || effect.monetary()
        || effect.body().len() != 56
        || effect.body()[..32] != record.id
        || effect.body()[48..] != revocation.to_be_bytes()
    {
        return Err(Error::Receipt);
    }
    record.revocation = revocation;
    record.closed = true;
    Ok(())
}

struct Replay<'a> {
    authority: &'a EvidenceAuthority,
    binding: &'a NativeBudgetBinding,
    record: BudgetRecord,
    outcomes: Vec<NativeBudgetOutcome>,
}

impl Replay<'_> {
    fn activity(
        &mut self,
        entry: &RawActivityReceiptEvidence,
        receipt: &VerifiedCumulativeReceipt,
        owner_key: [u8; 32],
    ) -> Result<(), Error> {
        let Some(activity) = budget_activity(entry, receipt)? else {
            return Ok(());
        };
        if activity.payload().get(2..34) != Some(self.record.id.as_slice()) {
            return Ok(());
        }
        let ordinal = activity.activity_type().value() & 0xffff;
        if receipt.result_code() == 0 && ordinal == 6 {
            if self.record.closed
                || self.record.revoked
                || receipt.header().timestamp_ms() >= self.record.expiry
            {
                return Err(Error::Window);
            }
            rollover(&mut self.record, receipt.header().timestamp_ms())?;
        }
        let mut binding = self.binding.clone();
        binding.owner_public_key = owner_key;
        binding.period_start_ms = self.record.period_start;
        binding.expiry_ms = self.record.expiry;
        if let Some(outcome) = EvidenceAuthority::native_budget_outcome(&binding, entry, receipt)? {
            if outcome.succeeded() {
                self.record.spent = self
                    .record
                    .spent
                    .checked_add(outcome.amount())
                    .ok_or(Error::Arithmetic)?;
                if self.record.spent > self.record.limit {
                    return Err(Error::Consumption);
                }
            }
            self.outcomes.push(outcome);
        }
        if receipt.result_code() != 0 {
            return Ok(());
        }
        let decoded = decode(entry.receipt().canonical_receipt()).map_err(|_| Error::Receipt)?;
        let protocol = decoded.protocol().ok_or(Error::Receipt)?;
        match ordinal {
            3 => {
                self.verify_amend(entry, receipt, &activity, protocol, owner_key)?;
                apply_amend(&mut self.record, &activity, protocol)?;
            }
            7 => apply_close(&mut self.record, &activity, protocol)?,
            2 | 4 | 5 | 6 => {}
            _ => return Err(Error::Activity),
        }
        Ok(())
    }

    fn verify_amend(
        &self,
        entry: &RawActivityReceiptEvidence,
        receipt: &VerifiedCumulativeReceipt,
        activity: &Activity,
        protocol: &ProtocolReceipt,
        owner_key: [u8; 32],
    ) -> Result<(), Error> {
        let authorization = self
            .authority
            .native_authorization(receipt.header().batch_number())?;
        let batch = AuthorizedBatch::new(
            protocol.batch_id(),
            protocol.asset(),
            protocol.previous_state_root(),
            protocol.resulting_state_root(),
            authorization.public_key(),
        );
        verify_native_owner_outcome(
            entry.receipt().canonical_receipt(),
            &batch,
            &NativeOwnerOutcomeContext {
                canonical_activity: entry.canonical_activity(),
                actor: self.binding.owner_did.as_bytes(),
                action_key: activity.idempotency_key(),
                activity_type: activity.activity_type(),
                owner_public_key: owner_key,
                network_id: receipt.header().network_id(),
            },
        )
        .map_err(|_| Error::Authority)?;
        Ok(())
    }
}

impl EvidenceAuthority {
    pub(super) fn native_budget_changes(
        &self,
        binding: &NativeBudgetBinding,
        evidence: &NativeBudgetRecoveryEvidence,
        receipts: &[VerifiedCumulativeReceipt],
        owner_keys: &[[u8; 32]],
    ) -> Result<(u128, Vec<NativeBudgetOutcome>), Error> {
        if receipts.len() != evidence.history.len() || receipts.len() != owner_keys.len() {
            return Err(Error::History);
        }
        let mut replay = Replay {
            authority: self,
            binding,
            record: BudgetRecord::decode(&evidence.baseline.canonical_record)
                .map_err(|_| Error::Record)?,
            outcomes: Vec::new(),
        };
        let mut next = 0;
        for maintenance in &evidence.maintenance {
            let header =
                decode_batch_header(maintenance.canonical_header()).map_err(|_| Error::History)?;
            while next < receipts.len()
                && receipts[next].header().batch_number() == header.batch_number()
            {
                replay.activity(&evidence.history[next], &receipts[next], owner_keys[next])?;
                next += 1;
            }
            if !replay.record.closed && header.timestamp_ms() < replay.record.expiry {
                rollover(&mut replay.record, header.timestamp_ms())?;
            }
        }
        let current =
            BudgetRecord::decode(&evidence.current.canonical_record).map_err(|_| Error::Record)?;
        if next != receipts.len() || replay.record != current {
            return Err(Error::Consumption);
        }
        Ok((current.spent, replay.outcomes))
    }
}
