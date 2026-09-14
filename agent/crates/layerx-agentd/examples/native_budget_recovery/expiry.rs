use super::{
    checked,
    fixture::Fixture,
    scenarios::{now_ms, Session},
    Result,
};
use layerx_agentd::outbox::SubmissionState;

pub fn run(fixture: &mut Fixture) -> Result<()> {
    let scope = fixture.create_with_lifetime(0xd0, 120_000, 120_000)?;
    let expiry = scope.binding.expiry_ms;
    let mut session = Session::with_scope(fixture, 0xd0, scope)?;
    let id = 0x7b;
    let exact = session.enqueue(fixture, id, [0xee; 32])?;
    session.transition(id, SubmissionState::Submitted)?;
    fixture.drop_response(&exact)?;
    session.transition(id, SubmissionState::Unknown)?;
    let activity = session
        .outbox
        .status([id; 32])
        .ok_or("unknown missing")?
        .activity_id;
    let (receipt, header) = fixture.receipt(activity)?;
    assert_ne!(super::fixture::result_code(&receipt)?, 0);
    assert!(
        checked(layerx_wire::receipt::decode_batch_header(
            &header.canonical_bytes
        ))?
        .timestamp_ms()
            < expiry
    );
    session = session.restart(fixture)?;
    assert!(session.reconcile(fixture).is_err());
    while now_ms()? <= expiry {
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    assert_eq!(
        session
            .outbox
            .status([id; 32])
            .ok_or("unknown missing")?
            .state,
        SubmissionState::Unknown
    );
    assert_eq!(checked(session.outbox.exact_signed_bytes([id; 32]))?, exact);
    fixture.create(0xd1)?;
    let reconciled = session.reconcile(fixture)?;
    assert!(reconciled.timestamp_ms() >= expiry);
    assert!(!reconciled.write_eligible());
    assert_eq!(reconciled.remaining(), 0);
    assert_eq!(
        session
            .outbox
            .status([id; 32])
            .ok_or("terminal missing")?
            .state,
        SubmissionState::Failed
    );
    session.assert_accounting(0, 0)?;
    assert!(session
        .runtime
        .authorize_preparation(
            &session.tenant,
            session.scope.binding.budget_id,
            1,
            expiry,
            now_ms()?,
            reconciled.observed_sequence()
        )
        .is_err());
    session = session.restart(fixture)?;
    session.reconcile(fixture)?;
    session.assert_accounting(0, 0)?;
    let external = fixture.spend(
        &session.scope,
        0x7c,
        25,
        session.scope.binding.owner_account,
    )?;
    let rejected = fixture.submit(external.exact_bytes())?;
    assert!(
        checked(layerx_wire::receipt::decode_batch_header(
            &rejected.1.canonical_bytes
        ))?
        .timestamp_ms()
            >= expiry
    );
    assert_ne!(super::fixture::result_code(&rejected.0)?, 0);
    fixture.finalize(&rejected.1)?;
    let reconciled = session.reconcile(fixture)?;
    let outcome = reconciled
        .outcomes()
        .iter()
        .find(|value| value.activity_id() == external.activity_id())
        .ok_or("actual expired Budget refusal missing")?;
    assert!(!outcome.succeeded());
    assert_eq!(outcome.amount(), 25);
    assert!(!reconciled.write_eligible());
    session.assert_accounting(0, 0)?;
    closed(fixture)?;
    Ok(())
}

fn closed(fixture: &mut Fixture) -> Result<()> {
    let mut session = Session::open(fixture, 0xe0)?;
    let id = 0x7d;
    let exact = session.enqueue(fixture, id, [0xee; 32])?;
    session.transition(id, SubmissionState::Submitted)?;
    fixture.drop_response(&exact)?;
    session.transition(id, SubmissionState::Unknown)?;
    let activity = session
        .outbox
        .status([id; 32])
        .ok_or("unknown missing")?
        .activity_id;
    let receipt = fixture.receipt(activity)?;
    assert_ne!(super::fixture::result_code(&receipt.0)?, 0);
    fixture.close(&session.scope, 0xe1)?;
    session = session.restart(fixture)?;
    let current = session.reconcile(fixture)?;
    assert!(!current.write_eligible());
    assert_eq!(current.remaining(), 0);
    assert_eq!(
        session
            .outbox
            .status([id; 32])
            .ok_or("terminal missing")?
            .state,
        SubmissionState::Failed
    );
    session.assert_accounting(0, 0)?;
    assert!(session
        .runtime
        .authorize_preparation(
            &session.tenant,
            session.scope.binding.budget_id,
            1,
            current.period_end_ms(),
            now_ms()?,
            current.observed_sequence()
        )
        .is_err());
    let external = fixture.spend(
        &session.scope,
        0x7e,
        25,
        session.scope.binding.owner_account,
    )?;
    let result = fixture.submit(external.exact_bytes())?;
    assert_ne!(super::fixture::result_code(&result.0)?, 0);
    fixture.finalize(&result.1)?;
    let current = session.reconcile(fixture)?;
    let outcome = current
        .outcomes()
        .iter()
        .find(|value| value.activity_id() == external.activity_id())
        .ok_or("actual closed Budget refusal missing")?;
    assert!(!outcome.succeeded());
    session = session.restart(fixture)?;
    session.reconcile(fixture)?;
    session.assert_accounting(0, 0)?;
    assert_eq!(checked(session.outbox.exact_signed_bytes([id; 32]))?, exact);
    Ok(())
}
