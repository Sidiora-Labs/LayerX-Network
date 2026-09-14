use super::{EvidenceAuthority, VerifiedCumulativeReceipt};
use crate::budget::{
    NativeBudgetBinding, NativeBudgetError as Error, NativeBudgetRecoveryEvidence,
};
use layerx_client::evidence::{verify_module_evidence, AccountEvidencePolicy, RootSelector};
use layerx_crypto::rotation::OwnerRotation;
use layerx_proof::receipt::{
    verify_native_owner_outcome, AuthorizedBatch, NativeOwnerOutcomeContext,
};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_types::verify::VerificationLevel;
use layerx_wire::activity::decode_signed;
use layerx_wire::receipt::decode;

impl EvidenceAuthority {
    pub(super) fn native_budget_rotation_chain(
        &self,
        previous: &NativeBudgetBinding,
        current: &NativeBudgetBinding,
        evidence: &NativeBudgetRecoveryEvidence,
        receipts: &[VerifiedCumulativeReceipt],
        state_root: [u8; 32],
    ) -> Result<Vec<[u8; 32]>, Error> {
        let mut keys = Vec::with_capacity(receipts.len());
        let mut public_key = previous.owner_public_key;
        let mut last_record = None;
        for (entry, receipt) in evidence.history.iter().zip(receipts) {
            keys.push(public_key);
            if receipt.module_id() != 7 || receipt.result_code() != 0 {
                continue;
            }
            let decoded =
                decode(entry.receipt().canonical_receipt()).map_err(|_| Error::Receipt)?;
            let protocol = decoded.protocol().ok_or(Error::Receipt)?;
            let changes: Vec<_> = protocol
                .effects()
                .iter()
                .filter(|effect| effect.event_type() == 0x7142)
                .collect();
            if changes.is_empty() {
                continue;
            }
            if changes.len() != 1
                || changes[0].kind() != 3
                || changes[0].module_id() != 7
                || changes[0].monetary()
            {
                return Err(Error::Authority);
            }
            let kind = ActivityType::new(ModuleId::Governance, 2).map_err(|_| Error::Activity)?;
            let registration = ModuleRegistration::new(ModuleId::Governance, &[kind])
                .map_err(|_| Error::Activity)?;
            let registry = ModuleRegistry::new(&[registration]).map_err(|_| Error::Activity)?;
            let activity = decode_signed(entry.canonical_activity(), &registry)
                .map_err(|_| Error::Activity)?;
            if activity.actor_did() != previous.owner_did.as_bytes() {
                continue;
            }
            let OwnerRotation::Commit(commit) =
                OwnerRotation::from_activity(&activity).map_err(|_| Error::Authority)?
            else {
                return Err(Error::Authority);
            };
            let authorization = self.native_authorization(receipt.header().batch_number())?;
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
                    actor: previous.owner_did.as_bytes(),
                    action_key: activity.idempotency_key(),
                    activity_type: kind,
                    owner_public_key: public_key,
                    network_id: receipt.header().network_id(),
                },
            )
            .map_err(|_| Error::Authority)?;
            if commit.consent.current_public_key != public_key
                || receipt.activity_id() != protocol.activity_id()
                || receipt.global_sequence() != protocol.global_sequence()
            {
                return Err(Error::Authority);
            }
            let mut record = b"LXOR1".to_vec();
            record.extend_from_slice(
                &layerx_wire::hash::did_id_for_protocol(&previous.owner_did, 3)
                    .map_err(|_| Error::Binding)?,
            );
            record.extend_from_slice(&public_key);
            record.extend_from_slice(&commit.consent.pending_public_key);
            record.extend_from_slice(&receipt.global_sequence().to_be_bytes());
            record.extend_from_slice(&commit.consent.announcement);
            if changes[0].body() != record {
                return Err(Error::Authority);
            }
            public_key = commit.consent.pending_public_key;
            last_record = Some(record);
        }
        if public_key != current.owner_public_key {
            return Err(Error::Authority);
        }
        if let Some(record) = last_record {
            self.native_budget_rotation_state(current, evidence, &record, state_root)?;
        }
        Ok(keys)
    }

    fn native_budget_rotation_state(
        &self,
        binding: &NativeBudgetBinding,
        evidence: &NativeBudgetRecoveryEvidence,
        expected: &[u8],
        state_root: [u8; 32],
    ) -> Result<(), Error> {
        let current = &evidence.current;
        let history = current.owner_history.as_ref().ok_or(Error::Authority)?;
        if history.canonical_record != expected {
            return Err(Error::Authority);
        }
        let (_, authorization) = self
            .verifier
            .authorization_for(&current.canonical_header)
            .map_err(|_| Error::Authority)?;
        let did = layerx_wire::hash::did_id_for_protocol(&binding.owner_did, 3)
            .map_err(|_| Error::Binding)?;
        let key = [&[0x0a][..], &did].concat();
        let verified = verify_module_evidence(
            &history.canonical_record,
            &history.proof_material,
            7,
            &key,
            AccountEvidencePolicy {
                expected_protocol_version: self.verifier.expected_protocol_version,
                expected_network_id: self.verifier.expected_network_id,
                handshake_sequencer_key: authorization.public_key(),
                root_selector: RootSelector::Checkpoint(current.checkpoint_id),
            },
        )
        .map_err(|_| Error::StateProof)?;
        if verified.state_root() != state_root
            || verified.level() < VerificationLevel::CHECKPOINT_FINALISED
            || verified.checkpoint_id() != Some(current.checkpoint_id)
            || verified.signed_header().canonical_bytes != current.canonical_header
        {
            return Err(Error::Checkpoint);
        }
        Ok(())
    }
}
