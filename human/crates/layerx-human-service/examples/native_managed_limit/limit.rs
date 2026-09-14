use super::{checked, fixture::Fixture, owner::Installed, Result};
use layerx_agentd::budget::{NativeBudgetRuntime, NativeBudgetScope};
use layerx_agentd::capability::NativeReservation;
use layerx_agentd::human::{HumanFinalizationEvidence, HumanOperations};
use layerx_agentd::human_runtime::HumanAuthorityBoundary;
use layerx_agentd::outbox::{Outbox, SubmissionState};
use layerx_agentd::receipt::{ReceiptLookupKey, ServedReceipt};
use layerx_human_service::agents::ProtocolEvidence;
use layerx_human_service::server::agent_creation::ProductionAgentCreation;
use layerx_intents::{Intent, IntentKind, NativeBudgetAmend};
use layerx_types::payload::ModuleId;
use layerx_types::verify::VerificationLevel;

fn now_ms() -> Result<u64> {
    use layerx_types::clock::Clock as _;
    Ok(
        layerx_client::runtime_clock::RuntimeClock::from_environment()?
            .sample(std::time::Duration::from_secs(1))?
            .unix_milliseconds,
    )
}
fn scope(installed: &Installed) -> Result<NativeBudgetScope> {
    checked(layerx_agentd::managed_agent::native_budget_scope(
        &*installed.store()?,
        &installed.tenant,
        installed.created.budget.budget_id,
    ))
}
fn accounting(runtime: &NativeBudgetRuntime, installed: &Installed, maximum: u128) -> Result<()> {
    let ceiling = runtime
        .ceiling(&installed.tenant, installed.created.budget.budget_id)
        .ok_or("managed native ceiling missing")?;
    let snapshot = checked(ceiling.snapshot())?;
    assert!(snapshot.reconciled);
    assert_eq!(snapshot.maximum, maximum);
    assert_eq!(snapshot.consumed, 0);
    assert_eq!(snapshot.held, 25);
    assert_eq!(snapshot.reservations, 1);
    Ok(())
}
fn reconcile(
    fixture: &mut Fixture,
    installed: &Installed,
    outbox: &mut Outbox,
    runtime: &mut NativeBudgetRuntime,
) -> Result<()> {
    let current = scope(installed)?;
    checked(runtime.reconcile(
        &mut *installed.store()?,
        &installed.tenant,
        &mut fixture.client,
        &fixture.registry,
        &current,
        outbox,
    ))?;
    checked(
        installed
            .owner
            .as_ref()
            .ok_or("owner missing")?
            .reconcile_native_budget(&installed.peer, current.binding.budget_id),
    )?;
    Ok(())
}
fn queued(
    fixture: &mut Fixture,
    installed: &Installed,
    outbox: &mut Outbox,
    runtime: &mut NativeBudgetRuntime,
) -> Result<Vec<u8>> {
    let current = scope(installed)?;
    let initial = checked(runtime.reconcile(
        &mut *installed.store()?,
        &installed.tenant,
        &mut fixture.client,
        &fixture.registry,
        &current,
        outbox,
    ))?;
    assert_eq!(initial.spent(), 0);
    let signed = fixture.spend(&current, 0xc7, 25, current.binding.owner_account)?;
    let activity = checked(layerx_wire::activity::decode_signed(
        signed.exact_bytes(),
        &fixture.registry,
    ))?;
    let exact = signed.exact_bytes().to_vec();
    checked(runtime.reserve(
        &installed.tenant,
        current.binding.budget_id,
        NativeReservation {
            id: signed.idempotency_key(),
            expected_activity_id: signed.activity_id(),
            amount: 25,
            expiry_ms: activity.timestamp_bound().not_after,
            unknown: false,
        },
        now_ms()?,
        initial.observed_sequence(),
    ))?;
    checked(outbox.enqueue(
        &mut *installed.store()?,
        installed.tenant.clone(),
        [0xc7; 32],
        signed,
    ))?;
    accounting(runtime, installed, 100)?;
    Ok(exact)
}
fn evidence(
    fixture: &mut Fixture,
    installed: &mut Installed,
    outbox: &mut Outbox,
    id: u8,
    amount: u128,
    expiry: u64,
) -> Result<HumanFinalizationEvidence> {
    let operation = NativeBudgetAmend {
        budget_id: installed.created.budget.budget_id,
        per_period_limit: amount,
        carry_cap: 0,
        expiry_ms: expiry,
        rollover: 1,
    };
    let compiled = checked(layerx_intents::compile(
        &Intent::v3(IntentKind::NativeBudgetAmend(operation)),
        &fixture.registry,
    ))?;
    let signed = fixture.signed(0x0003_0003, id, compiled.payload().as_bytes().to_vec())?;
    let exact = signed.exact_bytes().to_vec();
    let activity_id = signed.activity_id();
    checked(outbox.enqueue(
        &mut *installed.store()?,
        installed.tenant.clone(),
        [id; 32],
        signed,
    ))?;
    checked(outbox.transition(
        &mut *installed.store()?,
        [id; 32],
        SubmissionState::Submitted,
        "dispatch actual native AMEND after durable signed activity",
        None,
    ))?;
    let (receipt, header) = fixture.submit(&exact)?;
    assert_eq!(super::fixture::result_code(&receipt)?, 0);
    checked(outbox.transition(
        &mut *installed.store()?,
        [id; 32],
        SubmissionState::Acknowledged,
        "native sequencer acknowledged the exact AMEND",
        None,
    ))?;
    fixture.finalize(&header)?;
    installed.retain_receipt(&super::creation::Receipt {
        signed: exact.clone(),
        kind: 0x0003_0003,
        bytes: receipt.clone(),
        header,
    })?;
    installed.restart(fixture)?;
    checked(
        installed
            .owner
            .as_mut()
            .ok_or("owner missing")?
            .receipt_by_idempotency_key(&installed.peer, [id; 32], activity_id),
    )?;
    let served: ServedReceipt = checked(layerx_agentd::receipt::serve(
        &*installed.store()?,
        installed.tenant.clone(),
        ReceiptLookupKey::Idempotency([id; 32]),
    ))?;
    assert_eq!(served.canonical_bytes, receipt);
    assert!(served.metadata.verification_level >= VerificationLevel::CHECKPOINT_FINALISED);
    let authorized_batch = checked(installed.authority()?.authorized_activity(
        &installed.peer,
        &exact,
        activity_id,
    ))?;
    let proof = ProtocolEvidence {
        signed_activity: exact,
        actor: fixture.did.as_bytes().to_vec(),
        owner_public_key: fixture.public,
        network_id: 77,
        action_key: [id; 32],
        activity_id,
        receipt_bytes: receipt,
        authorized_batch,
        verification_level: served.metadata.verification_level,
    };
    let finalization = checked(ProductionAgentCreation::finalization_evidence(
        &proof,
        ModuleId::Budget,
        3,
        now_ms()? / 1000,
    ))?;
    assert_ne!(finalization.action_key, finalization.activity_id);
    Ok(HumanFinalizationEvidence {
        action_key: finalization.action_key,
        activity_id: finalization.activity_id,
        receipt_digest: finalization.receipt_digest,
        observed_sequence: finalization.observed_sequence,
        verification: finalization.verification,
        finalized_at: finalization.finalized_at,
    })
}
fn finalize(
    installed: &mut Installed,
    amount: u128,
    evidence: HumanFinalizationEvidence,
) -> Result<Vec<u8>> {
    Ok(checked(
        installed
            .owner
            .as_mut()
            .ok_or("owner missing")?
            .agent_limit(
                &installed.peer,
                &installed.seed.agent_id,
                amount,
                &installed.seed.currency,
                installed.created.budget.budget_id,
                evidence,
            ),
    )?
    .bytes()
    .to_vec())
}
fn assert_pending(installed: &Installed, expected: &[u8]) -> Result<()> {
    let mut restored = Outbox::default();
    checked(restored.restore(&*installed.store()?, installed.tenant.clone(), [0xc7; 32]))?;
    assert_eq!(checked(restored.exact_signed_bytes([0xc7; 32]))?, expected);
    assert_eq!(
        restored
            .status([0xc7; 32])
            .ok_or("pending Spend missing")?
            .state,
        SubmissionState::Queued
    );
    Ok(())
}

pub fn qualify(fixture: &mut Fixture, mut installed: Installed) -> Result<()> {
    let mut outbox = Outbox::default();
    let mut runtime = NativeBudgetRuntime::new(fixture.authority.clone());
    let pending = queued(fixture, &installed, &mut outbox, &mut runtime)?;
    let expiry = installed.created.budget.expiry_ms;
    let changed_expiry = evidence(
        fixture,
        &mut installed,
        &mut outbox,
        0xc8,
        80,
        expiry.checked_sub(1000).ok_or("expiry underflow")?,
    )?;
    assert!(finalize(&mut installed, 80, changed_expiry).is_err());
    assert_eq!(scope(&installed)?.maximum, 100);
    assert_pending(&installed, &pending)?;
    let accepted = evidence(fixture, &mut installed, &mut outbox, 0xc9, 80, expiry)?;
    assert!(finalize(&mut installed, 79, accepted).is_err());
    let mut changed_activity = accepted;
    changed_activity.activity_id[0] ^= 1;
    assert!(finalize(&mut installed, 80, changed_activity).is_err());
    assert_eq!(scope(&installed)?.maximum, 100);
    let response = finalize(&mut installed, 80, accepted)?;
    assert_eq!(finalize(&mut installed, 80, accepted)?, response);
    assert!(finalize(&mut installed, 81, accepted).is_err());
    let updated = scope(&installed)?;
    assert_eq!(updated.maximum, 80);
    assert_eq!(
        updated.binding.budget_id,
        installed.created.budget.budget_id
    );
    assert_eq!(updated.binding.asset, fixture.asset);
    assert_eq!(updated.binding.expiry_ms, expiry);
    assert_pending(&installed, &pending)?;
    reconcile(fixture, &installed, &mut outbox, &mut runtime)?;
    accounting(&runtime, &installed, 80)?;
    installed.restart(fixture)?;
    runtime = NativeBudgetRuntime::new(fixture.authority.clone());
    reconcile(fixture, &installed, &mut outbox, &mut runtime)?;
    accounting(&runtime, &installed, 80)?;
    assert_pending(&installed, &pending)?;
    assert_eq!(finalize(&mut installed, 80, accepted)?, response);
    assert!(finalize(&mut installed, 79, accepted).is_err());
    println!("real managed session: native creation/grant, capability, AMEND, idempotency, amount/expiry/activity refusals, pending hold and restart passed");
    Ok(())
}
