use super::{EvidenceAuthority, VerifiedCumulativeReceipt};
use std::collections::BTreeSet;
use crate::budget::{
    NativeAccountCandidate, NativeBudgetBinding, NativeBudgetCandidate, NativeBudgetError as Error,
    NativeBudgetOutcome, NativeBudgetReconciliation, NativeBudgetRecoveryEvidence,
};
use layerx_client::evidence::{
    verify_account_evidence, verify_module_evidence, AccountEvidencePolicy, RootSelector,
    VerifiedAccountEvidence,
};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_types::verify::VerificationLevel;
use layerx_wire::activity::{decode_signed, encode_signed};
use layerx_wire::hash::activity_id;
use layerx_wire::native_budget::BudgetRecord;
use layerx_wire::receipt::{decode, BatchHeader};
use sha2::{Digest, Sha256};

struct State {
    record: BudgetRecord,
    header: BatchHeader,
    remaining: u128,
    period_end: u64,
}

fn account(
    candidate: &NativeAccountCandidate, id: [u8; 32], asset: Option<[u8; 32]>,
    policy: AccountEvidencePolicy, header: &[u8],
) -> Result<VerifiedAccountEvidence, Error> {
    let value = verify_account_evidence(&candidate.canonical_value,
        &candidate.proof_material, id, asset, policy).map_err(|_| Error::AccountProof)?;
    if value.level() < VerificationLevel::CHECKPOINT_FINALISED
        || value.signed_header().canonical_bytes != header {
        return Err(Error::Checkpoint);
    }
    Ok(value)
}

impl EvidenceAuthority {
    /// # Errors
    /// Refuses unprotected, malformed or ambiguous configured sequencer authority.
    pub fn native_budget_authority(protocol_version: u16, network_id: u32,
        source: &std::path::Path) -> Result<Self, super::VerifierPolicyError> {
        Ok(Self::new(super::ProtocolEvidenceVerifier::load_source(protocol_version, network_id, source)?))
    }

    pub(crate) fn native_authorization(&self, batch: u64)
        -> Result<layerx_proof::inclusion::SequencerAuthorization, Error> {
        let value = self.verifier.sequencers.iter().find(|value|
            batch >= value.first_batch_number && value.active_last_batch().is_some_and(|last| batch <= last))
            .ok_or(Error::Authority)?;
        Ok(layerx_proof::inclusion::SequencerAuthorization::new(value.sequencer_id,
            value.public_key, value.first_batch_number, value.active_last_batch().ok_or(Error::Authority)?))
    }

    fn native_budget_state(&self, binding: &NativeBudgetBinding,
        raw: &NativeBudgetCandidate) -> Result<State, Error> {
        if binding.budget_id == [0; 32] || binding.owner_account == [0; 32]
            || binding.budget_account == [0; 32] || binding.asset == [0; 32]
            || binding.owner_public_key == [0; 32] || binding.period_length_ms == 0
            || binding.expiry_ms <= binding.period_start_ms || raw.checkpoint_id == [0; 32] {
            return Err(Error::Binding);
        }
        let (header, authorization) = self.verifier.authorization_for(&raw.canonical_header)
            .map_err(|_| Error::Authority)?;
        let policy = AccountEvidencePolicy {
            expected_protocol_version: self.verifier.expected_protocol_version,
            expected_network_id: self.verifier.expected_network_id,
            handshake_sequencer_key: authorization.public_key(),
            root_selector: RootSelector::Checkpoint(raw.checkpoint_id),
        };
        let key = [b"budget:".as_slice(), &binding.budget_id].concat();
        let module = verify_module_evidence(&raw.canonical_record, &raw.module_proof_material,
            3, &key, policy).map_err(|_| Error::StateProof)?;
        if module.level() < VerificationLevel::CHECKPOINT_FINALISED
            || module.checkpoint_id() != Some(raw.checkpoint_id)
            || module.signed_header().canonical_bytes != raw.canonical_header {
            return Err(Error::Checkpoint);
        }
        let record = BudgetRecord::decode(&raw.canonical_record).map_err(|_| Error::Record)?;
        if record.id != binding.budget_id || record.owner != binding.owner_account
            || record.account != binding.budget_account || record.asset != binding.asset
            || record.period_start != binding.period_start_ms
            || record.period_length != binding.period_length_ms || record.expiry != binding.expiry_ms
            || record.revocation == 0 || record.revocation > header.last_sequence()
            || record.closed || record.revoked {
            return Err(Error::Binding);
        }
        let period_end = record.period_start.checked_add(record.period_length)
            .ok_or(Error::Arithmetic)?.min(record.expiry);
        if header.timestamp_ms() < record.period_start || header.timestamp_ms() >= period_end {
            return Err(Error::Window);
        }
        let owner = account(&raw.owner, binding.owner_account, None, policy, &raw.canonical_header)?;
        let owner_name = [b"agent:".as_slice(), binding.owner_did.as_bytes(), b":main"].concat();
        if owner.account().name != owner_name || owner.account().kind != 1
            || owner.account().authority_key != Some(binding.owner_public_key) || owner.account().frozen {
            return Err(Error::Binding);
        }
        let budget = account(&raw.account, binding.budget_account, Some(binding.asset),
            policy, &raw.canonical_header)?;
        let budget_name = [b"agent:".as_slice(), binding.owner_did.as_bytes(), b":budget:",
            layerx_programs::hex::encode(&binding.budget_id).as_bytes()].concat();
        if budget.account().name != budget_name || budget.account().kind != 2
            || budget.account().frozen || budget.state_root() != module.state_root()
            || owner.state_root() != module.state_root() {
            return Err(Error::Binding);
        }
        match (record.source, &raw.source) {
            (Some(id), Some(candidate)) if id != record.owner => {
                let source = account(candidate, id, Some(binding.asset), policy, &raw.canonical_header)?;
                let source_name = [b"agent:".as_slice(), binding.owner_did.as_bytes(), b":asset:",
                    layerx_programs::hex::encode(&binding.asset).as_bytes()].concat();
                if source.account().kind != 1 || source.account().name != source_name
                    || source.account().authority_key != Some(binding.owner_public_key)
                    || source.account().frozen || source.state_root() != module.state_root() {
                    return Err(Error::Binding);
                }
            }
            (None, None) => {
                if owner.account().asset_id() != binding.asset { return Err(Error::Binding); }
            }
            (Some(id), None) if id == record.owner => {
                if owner.account().asset_id() != binding.asset { return Err(Error::Binding); }
            }
            _ => return Err(Error::Binding),
        }
        let remaining = record.remaining(budget.account().balance(), header.timestamp_ms(), false);
        Ok(State { record, header, remaining, period_end })
    }

    /// Reconciles native Budget state against a complete authenticated period history.
    ///
    /// # Errors
    /// Refuses identity or authority substitution, malformed proofs, stale periods,
    /// nonzero baselines, missing or replayed history and unrelated spend accounting.
    pub fn reconcile_native_budget(&self, binding: &NativeBudgetBinding,
        evidence: &NativeBudgetRecoveryEvidence) -> Result<NativeBudgetReconciliation, Error> {
        let baseline = self.native_budget_state(binding, &evidence.baseline)?;
        let current = self.native_budget_state(binding, &evidence.current)?;
        if baseline.record.spent != 0 || baseline.header.timestamp_ms() > current.header.timestamp_ms()
            || baseline.record.revocation != current.record.revocation {
            return Err(Error::Baseline);
        }
        let mut spent = 0_u128;
        let mut outcomes = Vec::new();
        let verified = self.native_budget_history(&baseline.header, &current.header, current.period_end, evidence)?;
        {
            let kinds = (1..=7).map(|ordinal| ActivityType::new(ModuleId::Budget, ordinal)
                .map_err(|_| Error::Activity)).collect::<Result<Vec<_>, _>>()?;
            let registration = ModuleRegistration::new(ModuleId::Budget, &kinds)
                .map_err(|_| Error::Activity)?;
            let registry = ModuleRegistry::new(&[registration]).map_err(|_| Error::Activity)?;
            for (entry, receipt) in evidence.history.iter().zip(verified) {
                let header = receipt.header();
                if receipt.module_id() != 3 { continue; }
                let activity = decode_signed(entry.canonical_activity(), &registry).map_err(|_| Error::Activity)?;
                if activity.protocol_version() != header.protocol_version()
                    || activity.network_id() != header.network_id()
                    || encode_signed(&activity).map_err(|_| Error::Activity)? != entry.canonical_activity()
                    || activity_id(&activity).map_err(|_| Error::Activity)? != receipt.activity_id() {
                    return Err(Error::Activity);
                }
                if activity.activity_type().value() != 0x0003_0006 { continue; }
                let payload = activity.payload();
                if payload.len() != 82 || payload[..2] != [0, 1] { return Err(Error::Activity); }
                if payload[2..34] != binding.budget_id { continue; }
                let amount = u128::from_be_bytes(payload[66..82].try_into().map_err(|_| Error::Activity)?);
                if amount == 0 || payload[34..66] == [0; 32] { return Err(Error::Activity); }
                let decoded = decode(entry.receipt().canonical_receipt()).map_err(|_| Error::Receipt)?;
                let protocol = decoded.protocol().ok_or(Error::Receipt)?;
                let succeeded = receipt.result_code() == 0;
                if succeeded {
                    if protocol.asset() != binding.asset || protocol.from() != binding.budget_account
                        || protocol.to().as_slice() != &payload[34..66] || protocol.amount() != amount {
                        return Err(Error::Receipt);
                    }
                    spent = spent.checked_add(amount).ok_or(Error::Arithmetic)?;
                }
                outcomes.push(NativeBudgetOutcome { binding: binding.clone(),
                    activity_id: receipt.activity_id(),
                    receipt_digest: Sha256::digest(entry.receipt().canonical_receipt()).into(),
                    amount, succeeded, timestamp_ms: header.timestamp_ms(), sequence: receipt.global_sequence(), idempotency_key: activity.idempotency_key(),
                    result_code: receipt.result_code(), canonical_receipt: entry.receipt().canonical_receipt().to_vec() });
            }
        }
        if spent != current.record.spent { return Err(Error::Consumption); }
        Ok(NativeBudgetReconciliation { binding: binding.clone(), authority: self.clone(),
            spent, remaining: current.remaining, observed_sequence: current.header.last_sequence(),
            timestamp_ms: current.header.timestamp_ms(), period_end_ms: current.period_end,
            checkpoint_id: evidence.current.checkpoint_id, outcomes })
    }
    fn native_budget_history(&self, baseline: &BatchHeader, current: &BatchHeader,
        period_end: u64, evidence: &NativeBudgetRecoveryEvidence) -> Result<Vec<VerifiedCumulativeReceipt>, Error> {
        let length = current.last_sequence().checked_sub(baseline.last_sequence()).ok_or(Error::History)?;
        if length > 4096 || evidence.history.len() > 4096 || evidence.maintenance.len() > 4096 {
            return Err(Error::History);
        }
        if length == 0 {
            return if evidence.baseline == evidence.current && evidence.history.is_empty() && evidence.maintenance.is_empty() {
                Ok(Vec::new())
            } else { Err(Error::History) };
        }
        let mut previous = baseline.clone();
        let mut entries = evidence.history.iter();
        let mut result = Vec::new();
        let mut activities = BTreeSet::new();
        let mut receipts = BTreeSet::new();
        for maintenance in &evidence.maintenance {
            let (header, authorization) = self.verifier.authorization_for(maintenance.canonical_header())
                .map_err(|_| Error::Authority)?;
            if previous.batch_number().checked_add(1) != Some(header.batch_number())
                || previous.last_sequence().checked_add(1) != Some(header.first_sequence())
                || previous.resulting_state_root() != header.previous_state_root()
                || header.timestamp_ms() < previous.timestamp_ms() || header.timestamp_ms() >= period_end {
                return Err(Error::History);
            }
            let count = header.last_sequence().checked_sub(header.first_sequence())
                .and_then(|value| u32::try_from(value).ok()).ok_or(Error::History)?;
            let leaf_count = count.checked_add(1).ok_or(Error::Arithmetic)?;
            if maintenance.proof().leaf_index() != count || maintenance.proof().leaf_count() != leaf_count {
                return Err(Error::History);
            }
            layerx_wire::batch_maintenance::decode_maintenance(maintenance.canonical_receipt())
                .map_err(|_| Error::History)?.verify_header(&header).map_err(|_| Error::History)?;
            layerx_proof::inclusion::verify_receipt(maintenance.canonical_receipt(),maintenance.proof(),
                maintenance.canonical_header(),&maintenance.header_signature(),&authorization)
                .map_err(|_| Error::History)?;
            for index in 0..count {
                let raw = entries.next().ok_or(Error::History)?;
                let verified = self.verifier.verify_signed_receipt_inclusion(raw.receipt()).map_err(|_| Error::Receipt)?;
                if verified.header() != &header || raw.receipt().proof().leaf_index() != index
                    || raw.receipt().proof().leaf_count() != leaf_count
                    || header.first_sequence().checked_add(u64::from(index)) != Some(verified.global_sequence())
                    || !activities.insert(verified.activity_id())
                    || !receipts.insert(Sha256::digest(raw.receipt().canonical_receipt()).to_vec()) {
                    return Err(Error::History);
                }
                result.push(verified);
            }
            previous=header;
        }
        if &previous != current || entries.next().is_some() { return Err(Error::History); }
        Ok(result)
    }

}
