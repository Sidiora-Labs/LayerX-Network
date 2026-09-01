include!("journey_faults.rs");

use layerx_human_service::activity::{ActivityKind, Feed, FilterDraft, PageRequest};
use layerx_human_service::agents::{
    Reclaim, ReclaimAgentBoundary, ReclaimAgentContext, ReclaimError, ReclaimMechanism,
    ReclaimRequest, ReclaimStage,
};
use layerx_human_service::journeys::{MovementTerm, PayerGrantRoute, SendRoute};
use layerx_types::intent::{BudgetId, PayerGrantId};

impl ReclaimAgentBoundary for RealAgentLayer {
    fn reclaim_receipt(
        &mut self,
        action_key: [u8; 32],
        expected_activity_id: [u8; 32],
    ) -> Result<ReceiptMaterial, AgentBoundaryError> {
        let material = self
            .receipt_material
            .get(&action_key)
            .cloned()
            .ok_or(AgentBoundaryError::CorruptResponse)?;
        let verified =
            layerx_proof::receipt::verify(&material.canonical_bytes, &material.authorised_batch)
                .map_err(|_| AgentBoundaryError::CorruptResponse)?;
        let protocol = verified
            .receipt()
            .protocol()
            .ok_or(AgentBoundaryError::CorruptResponse)?;
        if protocol.activity_id() != expected_activity_id {
            return Err(AgentBoundaryError::CorruptResponse);
        }
        Ok(material)
    }
}

fn send_route(public_key: [u8; 32], key: u8) -> SendRoute {
    SendRoute {
        account_sequence: Sequence::from_u64(ACCOUNT_SEQUENCE),
        idempotency_key: IdempotencyKey::new([key; 32]),
        expires_at: TimestampSeconds::from_u64(1_010),
        context_hash: ContextHash::new([0x61; 32]),
        authorization: SendAuthorization::new(
            SendAuthorizationKind::Owner,
            PublicKey::new(public_key),
            AuthorizationSignature::new([0x62; 64]),
        ),
        network_id: NetworkId::new(NETWORK_ID).unwrap_or_else(|error| panic!("network: {error:?}")),
        protocol_version: ProtocolVersion::new(layerx_wire::limits::PROTOCOL_VERSION)
            .unwrap_or_else(|error| panic!("protocol: {error:?}")),
    }
}

fn reclaim_request(journey: &str, key: u8, mechanism: ReclaimMechanism) -> ReclaimRequest {
    ReclaimRequest {
        journey_id: JourneyId::new(journey)
            .unwrap_or_else(|error| panic!("reclaim journey: {error}")),
        idempotency_key: [key; 32],
        owner: account("agent:did:layerx:human:main"),
        agent_account: account("agent:did:layerx:worker:main"),
        asset: AssetId::new([0x33; 32]),
        amount: Amount::from_u128(1),
        mechanism,
        agent: ReclaimAgentContext {
            label: "worker".to_owned(),
            actor: AgentDid::new("did:layerx:worker")
                .unwrap_or_else(|error| panic!("actor: {error:?}")),
            authority: AuthorityRef::new("custody-human-primary")
                .unwrap_or_else(|error| panic!("authority: {error:?}")),
            custody_key: KeyId::new("human-primary")
                .unwrap_or_else(|error| panic!("custody key: {error}")),
            account_sequence: ACCOUNT_SEQUENCE,
            not_before: 995,
            not_after: 1_010,
            fee_limit: 7,
        },
    }
}

fn drive_reclaim(
    fixture: &Fixture,
    agent: &mut RealAgentLayer,
    journey_id: &JourneyId,
    start_time: u64,
) -> Reclaim {
    for offset in 0..24_u64 {
        let mut store = fixture.store();
        let mut scope = store
            .principal(&fixture.principal)
            .unwrap_or_else(|error| panic!("reclaim scope: {error}"));
        let mut reclaim = Reclaim::load(&scope, journey_id)
            .unwrap_or_else(|error| panic!("load reclaim: {error}"))
            .unwrap_or_else(|| panic!("reclaim disappeared"));
        let status = ready(reclaim.advance(
            &mut scope,
            &fixture.agent_contract,
            agent,
            &fixture.signer,
            &registry(),
            &fixture.trace,
            start_time.saturating_add(offset),
        ))
        .unwrap_or_else(|error| {
            panic!(
                "advance reclaim {} at transition {offset}: {error}",
                journey_id.as_str()
            )
        });
        drop(scope);
        if matches!(status.stage(), ReclaimStage::Done | ReclaimStage::Refused) {
            return reclaim;
        }
    }
    panic!("reclaim did not reach a receipt-gated terminal state")
}

#[test]
fn every_reclaim_mechanism_uses_real_agent_receipts_and_projects_activity() {
    let fixture = Fixture::new("agent-reclaim-mechanisms");
    let requests = [
        reclaim_request(
            "jrn_reclaimbudget",
            0x71,
            ReclaimMechanism::BudgetDefund {
                budget_account: account("agent:did:layerx:worker:budget:operations"),
                budget_id: BudgetId::new([0x41; 32]),
                revocation_sequence: Sequence::from_u64(4),
            },
        ),
        reclaim_request(
            "jrn_reclaimagent",
            0x72,
            ReclaimMechanism::AgentAuthorized(send_route(fixture.public_key, 0x72)),
        ),
        reclaim_request(
            "jrn_reclaimgrant",
            0x73,
            ReclaimMechanism::ReceiveUnderPayerGrant(PayerGrantRoute {
                payer_grant: PayerGrantId::new([0x43; 32]),
                receiver_sequence: Sequence::from_u64(ACCOUNT_SEQUENCE),
                idempotency_key: IdempotencyKey::new([0x73; 32]),
                context_hash: ContextHash::new([0x44; 32]),
            }),
        ),
    ];
    let mut store = fixture.store();
    let mut scope = store
        .principal(&fixture.principal)
        .unwrap_or_else(|error| panic!("start scope: {error}"));
    for request in &requests {
        let first = Reclaim::start(&mut scope, request, &registry(), 300)
            .unwrap_or_else(|error| panic!("start reclaim: {error}"));
        let repeated = Reclaim::start(&mut scope, request, &registry(), 300)
            .unwrap_or_else(|error| panic!("repeat reclaim: {error}"));
        assert_eq!(first, repeated);
        assert_eq!(
            layerx_human_service::agents::ReclaimStatus::movement_term(),
            MovementTerm::Return
        );
        assert_eq!(
            layerx_human_service::agents::ReclaimStatus::user_action(),
            "Move money"
        );
    }
    drop(scope);

    let modes = BTreeMap::from([
        ([0x71; 32], DeliveryMode::TrackThenReceipt),
        ([0x72; 32], DeliveryMode::TrackThenReceipt),
        ([0x73; 32], DeliveryMode::TrackThenReceipt),
    ]);
    let mut agent = RealAgentLayer::new(&fixture.agent_root, modes);
    for (offset, request) in requests.iter().enumerate() {
        let reclaim = drive_reclaim(
            &fixture,
            &mut agent,
            &request.journey_id,
            301 + u64::try_from(offset).unwrap_or(0) * 30,
        );
        let status = reclaim
            .status()
            .unwrap_or_else(|error| panic!("reclaim status: {error}"));
        assert_eq!(status.stage(), ReclaimStage::Done);
        let result = status
            .result()
            .unwrap_or_else(|| panic!("receipt result missing"));
        assert_eq!(result.asset(), [0x33; 32]);
        assert_eq!(result.amount(), 1);
        assert_eq!(result.fee_charged(), 1);
        assert_ne!(result.activity_id(), [0; 32]);
        assert_ne!(result.receipt_digest(), [0; 32]);
        assert_eq!(agent.effects.get(&request.idempotency_key), Some(&1));
        assert_eq!(agent.submit_calls.get(&request.idempotency_key), Some(&1));
    }

    let mut final_store = fixture.store();
    let final_scope = final_store
        .principal(&fixture.principal)
        .unwrap_or_else(|error| panic!("activity scope: {error}"));
    let filters = Feed::apply_filters(
        FilterDraft::new()
            .with_kinds([ActivityKind::Movement])
            .with_agent("worker"),
    )
    .unwrap_or_else(|error| panic!("activity filters: {error}"));
    let page = Feed::new(120)
        .unwrap_or_else(|error| panic!("activity feed: {error}"))
        .page(&final_scope, PageRequest::new(10, filters), 400, 0)
        .unwrap_or_else(|error| panic!("activity page: {error}"));
    assert_eq!(page.entries().len(), 3);
    assert!(page.entries().iter().all(|entry| {
        entry.kind() == ActivityKind::Movement
            && entry.agent() == Some("worker")
            && entry.receipts().len() == 1
    }));
}

#[test]
fn reclaim_contract_is_closed_to_returns_and_rejects_conflicting_reuse() {
    assert_eq!(Reclaim::signing_operation(), Operation::ProtocolMutation);
    assert_eq!(Reclaim::signing_operation().label(), "protocol-mutation");
    assert!(!Reclaim::signing_operation().label().contains("sweep"));
    let fixture = Fixture::new("agent-reclaim-closed-contract");
    let request = reclaim_request(
        "jrn_reclaimclosed",
        0x74,
        ReclaimMechanism::AgentAuthorized(send_route(fixture.public_key, 0x74)),
    );
    let mut store = fixture.store();
    let mut scope = store
        .principal(&fixture.principal)
        .unwrap_or_else(|error| panic!("closed scope: {error}"));
    Reclaim::start(&mut scope, &request, &registry(), 500)
        .unwrap_or_else(|error| panic!("closed start: {error}"));
    let mut conflict = request;
    conflict.amount = Amount::from_u128(2);
    assert!(matches!(
        Reclaim::start(&mut scope, &conflict, &registry(), 500),
        Err(ReclaimError::IdempotencyConflict)
    ));
}
