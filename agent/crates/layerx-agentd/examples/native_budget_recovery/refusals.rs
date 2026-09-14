use super::{
    checked,
    fixture::Fixture,
    scenarios::{reservation, Session},
    Result,
};
use layerx_agentd::budget::{
    retrieve_native_budget_evidence, NativeBudgetBinding, NativeBudgetRecoveryEvidence,
};
use layerx_agentd::capability::{CeilingError, NativeCeiling};
use layerx_agentd::protocol_evidence::{RawActivityReceiptEvidence, RawReceiptEvidence};

fn rejects(
    fixture: &Fixture,
    binding: &NativeBudgetBinding,
    evidence: &NativeBudgetRecoveryEvidence,
) {
    assert!(fixture
        .authority
        .reconcile_native_budget(binding, evidence)
        .is_err());
}

fn binding_refusals(
    fixture: &Fixture,
    binding: &NativeBudgetBinding,
    evidence: &NativeBudgetRecoveryEvidence,
) -> Result<()> {
    for field in 0..9 {
        let mut changed = binding.clone();
        match field {
            0 => changed.budget_id[0] ^= 1,
            1 => changed.owner_account[0] ^= 1,
            2 => changed.budget_account[0] ^= 1,
            3 => changed.asset[0] ^= 1,
            4 => changed.owner_did = checked(layerx_types::ids::Did::new(b"did:layerx:unrelated"))?,
            5 => changed.owner_public_key[0] ^= 1,
            6 => changed.period_start_ms += 1,
            7 => changed.period_length_ms += 1,
            8 => changed.expiry_ms += 1,
            _ => unreachable!(),
        }
        rejects(fixture, &changed, evidence);
    }
    let wrong_network = checked(
        layerx_agentd::protocol_evidence::EvidenceAuthority::native_budget_authority(
            3,
            78,
            std::path::Path::new(&std::env::var("LAYERX_TEST_NATIVE_BUDGET_AUTHORITY")?),
        ),
    )?;
    assert!(wrong_network
        .reconcile_native_budget(binding, evidence)
        .is_err());
    Ok(())
}

fn state_refusals(
    fixture: &Fixture,
    binding: &NativeBudgetBinding,
    evidence: &NativeBudgetRecoveryEvidence,
) {
    for field in 0..9 {
        let mut changed = evidence.clone();
        match field {
            0 => changed.current.canonical_record[162] ^= 1,
            1 => changed.current.module_proof_material[0] ^= 1,
            2 => changed.current.owner.canonical_value[0] ^= 1,
            3 => changed.current.owner.proof_material[0] ^= 1,
            4 => changed.current.account.canonical_value[0] ^= 1,
            5 => changed.current.account.proof_material[0] ^= 1,
            6 => changed.current.canonical_header[0] ^= 1,
            7 => changed.current.checkpoint_id[0] ^= 1,
            8 => changed.baseline = changed.current.clone(),
            _ => unreachable!(),
        }
        rejects(fixture, binding, &changed);
    }
}

fn history_refusals(
    fixture: &Fixture,
    binding: &NativeBudgetBinding,
    evidence: &NativeBudgetRecoveryEvidence,
) {
    assert!(evidence.history.len() >= 3);
    for field in 0..7 {
        let mut changed = evidence.clone();
        match field {
            0 => {
                changed.history.remove(0);
            }
            1 => changed.history.push(changed.history[0].clone()),
            2 => changed.history.reverse(),
            3 => changed.maintenance.clear(),
            4 => {
                let entry = &changed.history[0];
                let mut activity = entry.canonical_activity().to_vec();
                activity[0] ^= 1;
                changed.history[0] = RawActivityReceiptEvidence::from_signed_inclusion(
                    activity,
                    entry.receipt().clone(),
                );
            }
            5 | 6 => {
                let entry = &changed.history[0];
                let raw = entry.receipt();
                let mut receipt = raw.canonical_receipt().to_vec();
                let mut signature = raw.header_signature();
                if field == 5 {
                    receipt[0] ^= 1;
                } else {
                    signature[0] ^= 1;
                }
                changed.history[0] = RawActivityReceiptEvidence::from_signed_inclusion(
                    entry.canonical_activity().to_vec(),
                    RawReceiptEvidence::new(
                        receipt,
                        raw.proof().clone(),
                        raw.canonical_header().to_vec(),
                        signature,
                    ),
                );
            }
            _ => unreachable!(),
        }
        rejects(fixture, binding, &changed);
    }
}

fn capacity_refusals(
    fixture: &mut Fixture,
    session: &Session,
    evidence: &NativeBudgetRecoveryEvidence,
) -> Result<()> {
    let verified = checked(
        fixture
            .authority
            .reconcile_native_budget(&session.scope.binding, evidence),
    )?;
    let signed = fixture.spend(
        &session.scope,
        0x76,
        25,
        session.scope.binding.owner_account,
    )?;
    let mut held = reservation(&signed, &fixture.registry)?;
    held.unknown = true;
    let mut ceiling = checked(NativeCeiling::rebuild(
        100,
        verified.clone(),
        &[held.clone()],
    ))?;
    assert_eq!(checked(ceiling.snapshot())?.held, 25);
    assert_eq!(
        ceiling.cancel_unsubmitted(held.id),
        Err(CeilingError::Indeterminate)
    );
    assert!(ceiling
        .authorize_amount(
            1,
            held.expiry_ms,
            held.expiry_ms,
            verified.observed_sequence()
        )
        .is_err());
    assert_eq!(checked(ceiling.snapshot())?.held, 25);
    assert!(NativeCeiling::rebuild(100, verified.clone(), &[held.clone(), held]).is_err());
    let first = fixture.spend(
        &session.scope,
        0x77,
        u128::MAX,
        session.scope.binding.owner_account,
    )?;
    let second = fixture.spend(
        &session.scope,
        0x78,
        u128::MAX,
        session.scope.binding.owner_account,
    )?;
    let first = reservation(&first, &fixture.registry)?;
    let second = reservation(&second, &fixture.registry)?;
    assert!(matches!(
        NativeCeiling::rebuild(u128::MAX, verified, &[first, second]),
        Err(CeilingError::Overflow)
    ));
    Ok(())
}

fn spend_event_refusals(
    fixture: &Fixture,
    binding: &NativeBudgetBinding,
    evidence: &NativeBudgetRecoveryEvidence,
) -> Result<()> {
    let mut matched = false;
    for (index, entry) in evidence.history.iter().enumerate() {
        let receipt = checked(layerx_wire::receipt::decode(
            entry.receipt().canonical_receipt(),
        ))?;
        let protocol = receipt.protocol().ok_or("protocol receipt required")?;
        if protocol.module_id() != 3 || protocol.result_code() != 0 {
            continue;
        }
        let [effect] = protocol.effects() else {
            continue;
        };
        if effect.event_type() != 6 || effect.body().get(..32) != Some(binding.budget_id.as_slice())
        {
            continue;
        }
        assert_eq!(effect.body().len(), 80);
        let raw = entry.receipt();
        let locations: Vec<_> = raw
            .canonical_receipt()
            .windows(80)
            .enumerate()
            .filter_map(|(offset, bytes)| (bytes == effect.body()).then_some(offset))
            .collect();
        assert_eq!(locations.len(), 1);
        for field in [0, 32, 64] {
            let mut changed = evidence.clone();
            let mut bytes = raw.canonical_receipt().to_vec();
            bytes[locations[0] + field] ^= 1;
            changed.history[index] = RawActivityReceiptEvidence::from_signed_inclusion(
                entry.canonical_activity().to_vec(),
                RawReceiptEvidence::new(
                    bytes,
                    raw.proof().clone(),
                    raw.canonical_header().to_vec(),
                    raw.header_signature(),
                ),
            );
            rejects(fixture, binding, &changed);
        }
        matched = true;
    }
    assert!(matched);
    Ok(())
}

pub fn run(fixture: &mut Fixture) -> Result<()> {
    let mut session = Session::open(fixture, 0xb0)?;
    let baseline = checked(retrieve_native_budget_evidence(
        &mut fixture.client,
        &fixture.authority,
        &session.scope.binding,
        None,
        &fixture.registry,
    ))?;
    let other = fixture.create(0xb1)?;
    let first = fixture.spend(
        &session.scope,
        0x71,
        25,
        session.scope.binding.owner_account,
    )?;
    let first_id = first.activity_id();
    let first = fixture.submit(first.exact_bytes())?;
    fixture.finalize(&first.1)?;
    let other_spend = fixture.spend(&other, 0x72, 25, other.binding.owner_account)?;
    let other_id = other_spend.activity_id();
    let other_receipt = fixture.submit(other_spend.exact_bytes())?;
    fixture.finalize(&other_receipt.1)?;
    let evidence = checked(retrieve_native_budget_evidence(
        &mut fixture.client,
        &fixture.authority,
        &session.scope.binding,
        Some(baseline.baseline),
        &fixture.registry,
    ))?;
    let verified = checked(
        fixture
            .authority
            .reconcile_native_budget(&session.scope.binding, &evidence),
    )?;
    assert_eq!(verified.spent(), 25);
    assert_eq!(verified.remaining(), 75);
    assert_eq!(verified.outcomes().len(), 1);
    assert_eq!(verified.outcomes()[0].activity_id(), first_id);
    assert_ne!(verified.outcomes()[0].activity_id(), other_id);
    binding_refusals(fixture, &session.scope.binding, &evidence)?;
    state_refusals(fixture, &session.scope.binding, &evidence);
    history_refusals(fixture, &session.scope.binding, &evidence);
    spend_event_refusals(fixture, &session.scope.binding, &evidence)?;
    capacity_refusals(fixture, &session, &evidence)?;
    session.reconcile(fixture)?;
    session.scope.write_enabled = false;
    let paused = session.reconcile(fixture)?;
    assert!(session
        .runtime
        .authorize_preparation(
            &session.tenant,
            session.scope.binding.budget_id,
            1,
            paused.period_end_ms(),
            paused.timestamp_ms(),
            paused.observed_sequence()
        )
        .is_err());
    super::scenarios::rollover(fixture)?;
    super::expiry::run(fixture)?;
    Ok(())
}
