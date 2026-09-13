use super::{
    decode, hex, native_checkpoint, native_complete_suffix, native_identity_state, native_u64,
    unavailable, Authority, Identity, PrincipalPolicy, Response, Verified,
};
use layerx_crypto::session::{decode_session_key, IssuedSessionKey};
use layerx_wire::receipt::ProtocolReceipt;
use std::collections::{BTreeMap, BTreeSet};

struct Registration {
    grant: IssuedSessionKey,
    expiry_sequence: u64,
}

impl Registration {
    fn decode(summary: &[u8], body: &[u8], state: &[u8], sequence: u64) -> Result<Self, Response> {
        let refused = || unavailable("identity_session_proof_invalid");
        let grant = decode_session_key(body).map_err(|_| refused())?;
        let minimum = grant
            .permitted_activity_types
            .iter()
            .map(|a| a.ordinal())
            .min()
            .unwrap_or(0);
        let maximum = grant
            .permitted_activity_types
            .iter()
            .map(|a| a.ordinal())
            .max()
            .unwrap_or(0);
        let modules = grant
            .permitted_activity_types
            .iter()
            .fold(0_u64, |mask, a| mask | (1_u64 << a.module() as u16));
        if summary.len() != 209
            || state.len() != 223
            || &summary[..5] != b"LXGS2"
            || summary[5..37] != grant.grant_id
            || summary[37..69] != grant.grantor
            || summary[37..69] != state[5..37]
            || summary[69..101] != state[37..69]
            || summary[101..133] == [0; 32]
            || summary[133..165] != grant.session_public_key
            || native_u64(summary, 173)? != modules
            || summary[181..183] != minimum.to_be_bytes()
            || summary[183..185] != maximum.to_be_bytes()
            || native_u64(summary, 185)? != grant.not_before
            || native_u64(summary, 193)? != grant.expires_at
            || native_u64(summary, 201)? != grant.revocation_sequence
            || grant.revocation_sequence != native_u64(state, 69)?
        {
            return Err(refused());
        }
        let expiry_sequence = native_u64(summary, 165)?;
        if expiry_sequence <= sequence {
            return Err(refused());
        }
        Ok(Self {
            grant,
            expiry_sequence,
        })
    }

    fn from_receipt(
        receipt: &ProtocolReceipt,
        state: &[u8],
        sequence: u64,
    ) -> Result<Option<Self>, Response> {
        let refused = || unavailable("identity_session_proof_invalid");
        let mut summary = None;
        let mut body = Vec::new();
        let mut previous_chunk = 0;
        for effect in receipt.effects().iter().filter(|e| e.module_id() == 7) {
            match effect.event_type() {
                0x7145 => {
                    if effect.kind() != 3
                        || effect.monetary()
                        || summary.replace(effect.body()).is_some()
                    {
                        return Err(refused());
                    }
                }
                0x7105 | 0x7125 => {
                    if effect.kind() != 3
                        || effect.monetary()
                        || summary.is_none()
                        || effect.body().is_empty()
                        || effect.body().len() > 256
                        || (effect.event_type() == 0x7105 && !body.is_empty())
                        || (effect.event_type() == 0x7125
                            && (body.is_empty() || previous_chunk != 256))
                        || body.len() + effect.body().len() > 1024
                    {
                        return Err(refused());
                    }
                    previous_chunk = effect.body().len();
                    body.extend_from_slice(effect.body());
                }
                _ => {}
            }
        }
        match summary {
            Some(summary) => Self::decode(summary, &body, state, sequence).map(Some),
            None if body.is_empty() => Ok(None),
            None => Err(refused()),
        }
    }

    fn active(&self, revision: u64, head: u64, now: u128) -> bool {
        self.grant.revocation_sequence == revision
            && self.expiry_sequence > head
            && now >= u128::from(self.grant.not_before)
            && now < u128::from(self.grant.expires_at)
    }
}

pub(super) fn current(
    identity: &Identity,
    anchor: &Verified,
    evidence: &[Verified],
    policy: &PrincipalPolicy,
) -> Result<(Vec<u8>, Vec<Authority>), Response> {
    let refused = || unavailable("identity_state_proof_unavailable");
    let anchor_state = native_identity_state(anchor, &identity.did)?;
    if identity.frozen
        || identity.revocation_sequence != native_u64(&anchor_state, 69)?
        || identity.authorities.len() != 1
        || identity.authorities[0].kind != "primary_key"
        || identity.authorities[0].id != hex::encode(&anchor_state[37..69])
    {
        return Err(refused());
    }
    let start = evidence
        .iter()
        .find(|item| native_identity_state(item, &identity.did).is_ok())
        .ok_or_else(refused)?;
    native_complete_suffix(start, evidence)?;
    let latest = evidence.last().ok_or_else(refused)?;
    native_checkpoint(latest)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| refused())?
        .as_millis();
    if now
        .checked_sub(u128::from(latest.timestamp_ms))
        .is_none_or(|age| age > u128::from(policy.maximum_age_seconds) * 1000)
    {
        return Err(unavailable("identity_head_evidence_stale"));
    }
    let mut state = Vec::new();
    let mut registered = BTreeMap::new();
    let mut revoked = BTreeSet::new();
    for item in evidence
        .iter()
        .filter(|item| item.facts.global_sequence >= start.facts.global_sequence)
    {
        let bytes = hex::decode(&item.record.receipt_hex).map_err(|_| refused())?;
        let receipt = decode(&bytes).map_err(|_| refused())?;
        let protocol = receipt.protocol().ok_or_else(refused)?;
        if protocol.module_id() != 7 || protocol.result_code() != 0 {
            continue;
        }
        let states: Vec<_> = protocol
            .effects()
            .iter()
            .filter(|e| e.module_id() == 7 && e.event_type() == 0x7110)
            .collect();
        if states.len() != 1 || states[0].body().len() != 223 {
            return Err(refused());
        }
        if states[0].body()[5..37] != anchor_state[5..37] {
            continue;
        }
        let next = native_identity_state(item, &identity.did)?;
        if !state.is_empty() && native_u64(&next, 69)? < native_u64(&state, 69)? {
            return Err(refused());
        }
        native_checkpoint(item)?;
        if let Some(registration) =
            Registration::from_receipt(protocol, &next, item.facts.global_sequence)?
        {
            let id = registration.grant.grant_id;
            if revoked.contains(&id) || registered.insert(id, registration).is_some() {
                return Err(refused());
            }
        }
        for effect in protocol
            .effects()
            .iter()
            .filter(|e| e.module_id() == 7 && e.event_type() == 0x7106)
        {
            let body = effect.body();
            if body.len() != 41
                || effect.kind() != 3
                || effect.monetary()
                || !(1..=5).contains(&body[32])
                || native_u64(body, 33)? != item.facts.global_sequence
                || native_u64(&next, 69)? != item.facts.global_sequence
            {
                return Err(refused());
            }
            let id: [u8; 32] = body[..32].try_into().map_err(|_| refused())?;
            if id == [0; 32] || !revoked.insert(id) {
                return Err(refused());
            }
            registered.remove(&id);
        }
        state = next;
    }
    let revision = native_u64(&state, 69)?;
    let mut authorities = vec![Authority {
        kind: "primary_key".to_owned(),
        id: hex::encode(&state[37..69]),
    }];
    for (id, registration) in registered {
        if registration.active(revision, latest.last_sequence, now) {
            authorities.push(Authority {
                kind: "session_key".to_owned(),
                id: hex::encode(&id),
            });
        }
    }
    Ok((state, authorities))
}

#[cfg(test)]
mod tests {
    use super::*;
    use layerx_crypto::authority_grant::NativeFeeBudget;
    use layerx_crypto::session::{issue_session_key, SessionKeyRequest, SessionPurpose};
    use layerx_types::payload::{ActivityType, ModuleId};

    fn issued(purpose: SessionPurpose, fee: bool) -> (IssuedSessionKey, Vec<u8>, Vec<u8>) {
        let grant = issue_session_key(&SessionKeyRequest {
            grantor: [1; 32],
            session_public_key: ed25519_dalek::SigningKey::from_bytes(&[2; 32])
                .verifying_key()
                .to_bytes(),
            not_before: 100,
            expires_at: Some(1000),
            permitted_activity_types: if purpose == SessionPurpose::Activity {
                vec![ActivityType::new(ModuleId::Asset, 5).unwrap_or_else(|_| panic!("activity"))]
            } else {
                Vec::new()
            },
            revocation_sequence: Some(3),
            fee_budget: fee.then_some(NativeFeeBudget {
                asset: [4; 32],
                maximum_per_activity: 5,
                maximum_total: 20,
                period_length: 100,
                maximum_per_period: 10,
                period_start: 100,
            }),
            purpose,
        })
        .unwrap_or_else(|_| panic!("canonical grant"));
        let mut state = vec![0; 223];
        state[..5].copy_from_slice(b"LXGI1");
        state[5..37].copy_from_slice(&grant.grantor);
        state[37..69].copy_from_slice(
            &ed25519_dalek::SigningKey::from_bytes(&[5; 32])
                .verifying_key()
                .to_bytes(),
        );
        state[69..77].copy_from_slice(&grant.revocation_sequence.to_be_bytes());
        state[215..223].copy_from_slice(&5_u64.to_be_bytes());
        let mut summary = b"LXGS2".to_vec();
        summary.extend_from_slice(&grant.grant_id);
        summary.extend_from_slice(&grant.grantor);
        summary.extend_from_slice(&state[37..69]);
        summary.extend_from_slice(&[6; 32]);
        summary.extend_from_slice(&grant.session_public_key);
        summary.extend_from_slice(&20_u64.to_be_bytes());
        summary.extend_from_slice(
            &(if purpose == SessionPurpose::Activity {
                2_u64
            } else {
                0
            })
            .to_be_bytes(),
        );
        let ordinal = if purpose == SessionPurpose::Activity {
            5_u16
        } else {
            0
        };
        summary.extend_from_slice(&ordinal.to_be_bytes());
        summary.extend_from_slice(&ordinal.to_be_bytes());
        summary.extend_from_slice(&grant.not_before.to_be_bytes());
        summary.extend_from_slice(&grant.expires_at.to_be_bytes());
        summary.extend_from_slice(&grant.revocation_sequence.to_be_bytes());
        (grant, summary, state)
    }

    #[test]
    fn summary_binds_canonical_activity_fee_and_authentication_grants() {
        for (purpose, fee) in [
            (SessionPurpose::Activity, false),
            (SessionPurpose::Activity, true),
            (SessionPurpose::Authentication, false),
        ] {
            let (grant, summary, state) = issued(purpose, fee);
            let registration =
                Registration::decode(&summary, &grant.registration_payload, &state, 5)
                    .unwrap_or_else(|_| panic!("bound registration"));
            assert_eq!(registration.grant, grant);
            assert!(registration.active(3, 19, 100));
            assert!(registration.active(3, 19, 999));
            assert!(!registration.active(4, 19, 100));
            assert!(!registration.active(3, 20, 100));
            assert!(!registration.active(3, 19, 99));
            assert!(!registration.active(3, 19, 1000));
        }
    }

    #[test]
    fn mismatched_identity_key_scope_grant_or_time_is_refused() {
        let (grant, summary, state) = issued(SessionPurpose::Activity, true);
        for offset in [0, 5, 37, 69, 133, 173, 181, 183, 185, 193, 201] {
            let mut changed = summary.clone();
            changed[offset] ^= 1;
            assert!(
                Registration::decode(&changed, &grant.registration_payload, &state, 5).is_err(),
                "summary offset {offset}"
            );
        }
        for offset in [5, 37, 69] {
            let mut changed = state.clone();
            changed[offset] ^= 1;
            assert!(
                Registration::decode(&summary, &grant.registration_payload, &changed, 5).is_err(),
                "state offset {offset}"
            );
        }
        let mut changed = summary.clone();
        changed[101..133].fill(0);
        assert!(Registration::decode(&changed, &grant.registration_payload, &state, 5).is_err());
        changed = summary.clone();
        changed[165..173].copy_from_slice(&5_u64.to_be_bytes());
        assert!(Registration::decode(&changed, &grant.registration_payload, &state, 5).is_err());
        for end in 0..grant.registration_payload.len() {
            assert!(
                Registration::decode(&summary, &grant.registration_payload[..end], &state, 5)
                    .is_err()
            );
        }
        for end in 0..summary.len() {
            assert!(
                Registration::decode(&summary[..end], &grant.registration_payload, &state, 5)
                    .is_err()
            );
        }
        let mut changed = grant.registration_payload.clone();
        changed.push(0);
        assert!(Registration::decode(&summary, &changed, &state, 5).is_err());
    }

    #[test]
    fn original_native_receipts_verify_and_supply_canonical_session_membership() {
        let cases: [(&[u8], &[u8; 32], bool); 2] = [
            (
                include_bytes!("../../tests/fixtures/native-sessions/legacy.receipt"),
                include_bytes!("../../tests/fixtures/native-sessions/legacy.sequencer-public"),
                false,
            ),
            (
                include_bytes!("../../tests/fixtures/native-sessions/fee.receipt"),
                include_bytes!("../../tests/fixtures/native-sessions/fee.sequencer-public"),
                true,
            ),
        ];
        for (bytes, key, paid) in cases {
            let receipt = layerx_proof::receipt::verify_sequencer_signature(bytes, *key)
                .unwrap_or_else(|error| panic!("original native signature: {error:?}"));
            let protocol = receipt
                .protocol()
                .unwrap_or_else(|| panic!("native protocol receipt"));
            assert_eq!(protocol.module_id(), 7);
            assert_eq!(protocol.result_code(), 0);
            let state = protocol
                .effects()
                .iter()
                .find(|effect| effect.module_id() == 7 && effect.event_type() == 0x7110)
                .unwrap_or_else(|| panic!("native identity event"))
                .body();
            let registration =
                Registration::from_receipt(protocol, state, protocol.global_sequence())
                    .unwrap_or_else(|_| panic!("native registration"))
                    .unwrap_or_else(|| panic!("missing native session"));
            assert_eq!(registration.grant.fee_budget.is_some(), paid);
            assert_eq!(registration.grant.grantor, state[5..37]);
            assert!(registration.active(
                registration.grant.revocation_sequence,
                protocol.global_sequence(),
                u128::from(registration.grant.not_before)
            ));
            assert!(!registration.active(
                registration.grant.revocation_sequence + 1,
                protocol.global_sequence(),
                u128::from(registration.grant.not_before)
            ));
            let mut changed = bytes.to_vec();
            let last = changed.len() - 1;
            changed[last] ^= 1;
            assert!(layerx_proof::receipt::verify_sequencer_signature(&changed, *key).is_err());
            let mut changed_key = *key;
            changed_key[0] ^= 1;
            assert!(layerx_proof::receipt::verify_sequencer_signature(bytes, changed_key).is_err());
        }
    }
}
