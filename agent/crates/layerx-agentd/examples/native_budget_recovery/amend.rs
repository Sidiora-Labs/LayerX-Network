use super::{
    checked,
    fixture::{result_code, Fixture},
    scenarios::{now_ms, Session},
    Result,
};
use layerx_agentd::budget::{retrieve_native_budget_evidence, NativeBudgetScope};
use layerx_agentd::outbox::SubmissionState;
use layerx_wire::encode::Encoder;

fn amend(
    fixture: &mut Fixture,
    scope: &NativeBudgetScope,
    id: u8,
    limit: u128,
    expiry: u64,
) -> Result<()> {
    let mut payload = Encoder::new(75);
    checked(payload.u16(1))?;
    checked(payload.fixed(&scope.binding.budget_id))?;
    checked(payload.u128(limit))?;
    checked(payload.u128(7))?;
    checked(payload.u64(expiry))?;
    checked(payload.u8(2))?;
    let exact = fixture.owner_bytes(0x0003_0003, id, &payload.finish())?;
    let result = fixture.submit(&exact)?;
    assert_eq!(result_code(&result.0)?, 0);
    fixture.finalize(&result.1)
}

pub fn run(fixture: &mut Fixture) -> Result<()> {
    let scope = fixture.create_with_lifetime(0xa0, 3_600_000, 1_800_000)?;
    let original_expiry = scope.binding.expiry_ms;
    let mut session = Session::with_scope(fixture, 0xa0, scope)?;
    let previous = session.reconcile(fixture)?;
    let baseline = checked(retrieve_native_budget_evidence(
        &mut fixture.client,
        &fixture.authority,
        &session.scope.binding,
        None,
        &fixture.registry,
    ))?
    .current;
    let owner = session.scope.binding.owner_account;
    let exact = session.enqueue(fixture, 0x40, owner)?;
    session.transition(0x40, SubmissionState::Submitted)?;
    session.transition(0x40, SubmissionState::Unknown)?;
    amend(fixture, &session.scope, 0x41, 10, original_expiry + 60_000)?;
    let mut binding = session.scope.binding.clone();
    binding.expiry_ms = original_expiry + 60_000;
    let raw = checked(retrieve_native_budget_evidence(
        &mut fixture.client,
        &fixture.authority,
        &binding,
        Some(baseline),
        &fixture.registry,
    ))?;
    checked(
        fixture
            .authority
            .advance_native_budget(&binding, &raw, &previous),
    )?;
    assert!(fixture
        .authority
        .reconcile_native_budget(&binding, &raw)
        .is_err());
    let mut missing = raw.clone();
    missing.history.clear();
    assert!(fixture
        .authority
        .advance_native_budget(&binding, &missing, &previous)
        .is_err());
    session = session.restart(fixture)?;
    let lower = session.reconcile(fixture)?;
    assert_eq!(lower.remaining(), 10);
    assert_eq!(lower.binding().expiry_ms, binding.expiry_ms);
    session.assert_accounting(0, 25)?;
    assert!(session
        .runtime
        .authorize_preparation(
            &session.tenant,
            binding.budget_id,
            1,
            lower.period_end_ms(),
            now_ms()?,
            lower.observed_sequence()
        )
        .is_err());
    amend(fixture, &session.scope, 0x42, 80, original_expiry + 120_000)?;
    session = session.restart(fixture)?;
    let raised = session.reconcile(fixture)?;
    assert_eq!(raised.remaining(), 80);
    session.assert_accounting(0, 25)?;
    checked(session.runtime.authorize_preparation(
        &session.tenant,
        binding.budget_id,
        10,
        raised.period_end_ms(),
        now_ms()?,
        raised.observed_sequence(),
    ))?;
    assert_eq!(
        session
            .outbox
            .status([0x40; 32])
            .ok_or("unknown missing")?
            .state,
        SubmissionState::Unknown
    );
    assert_eq!(
        checked(session.outbox.exact_signed_bytes([0x40; 32]))?,
        exact
    );
    shorter_expiry(fixture)
}

fn shorter_expiry(fixture: &mut Fixture) -> Result<()> {
    let mut session = Session::open(fixture, 0xa1)?;
    let owner = session.scope.binding.owner_account;
    let exact = session.enqueue(fixture, 0x43, owner)?;
    session.transition(0x43, SubmissionState::Submitted)?;
    fixture.drop_response(&exact)?;
    session.transition(0x43, SubmissionState::Unknown)?;
    let activity_id = session
        .outbox
        .status([0x43; 32])
        .ok_or("unknown missing")?
        .activity_id;
    let receipt = fixture.receipt(activity_id)?;
    assert_eq!(result_code(&receipt.0)?, 0);
    let timestamp = checked(layerx_wire::receipt::decode_batch_header(
        &receipt.1.canonical_bytes,
    ))?
    .timestamp_ms();
    let shortened = timestamp.checked_sub(1).ok_or("expiry underflow")?;
    assert!(shortened > session.scope.binding.period_start_ms);
    amend(fixture, &session.scope, 0x44, 40, shortened)?;
    session = session.restart(fixture)?;
    let reconciled = session.reconcile(fixture)?;
    assert_eq!(reconciled.binding().expiry_ms, shortened);
    assert!(!reconciled.write_eligible());
    assert_eq!(
        session
            .outbox
            .status([0x43; 32])
            .ok_or("terminal missing")?
            .state,
        SubmissionState::Executed
    );
    session.assert_accounting(25, 0)?;
    session = session.restart(fixture)?;
    session.reconcile(fixture)?;
    session.assert_accounting(25, 0)?;
    assert_eq!(
        checked(session.outbox.exact_signed_bytes([0x43; 32]))?,
        exact
    );
    Ok(())
}
