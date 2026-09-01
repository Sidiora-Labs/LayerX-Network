#[allow(dead_code)]
mod support;

use std::fs;

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_agent_api::subscription::{
    Cursor as ApiCursor, EventDelivery, EventIdentity, ReceiptReference,
};
use layerx_agent_api::track::ReceiptRef;
use layerx_agent_api::verify::Level;
use layerx_agent_api::Sequence;
use layerx_human_service::activity::detail::{
    DetailError, EvidenceKind, FinalityReference, ReceiptActual, StageState,
};
use layerx_human_service::activity::{
    ActivityKind, AgentActivity, DepositStage, EntryDetail, Feed, PendingStatus, VerifiedStatus,
    WithdrawalStage,
};
use layerx_human_service::notify::ActivityEntryId;
use layerx_proof::receipt::{verify, AuthorizedBatch, VerifiedReceipt};
use layerx_types::verify::VerificationLevel;
use sha2::{Digest as _, Sha256};

#[derive(Clone)]
struct ReceiptFields {
    activity_id: [u8; 32],
    previous_state_root: [u8; 32],
    resulting_state_root: [u8; 32],
    batch_id: [u8; 32],
    asset: [u8; 32],
}

fn push_bytes(output: &mut Vec<u8>, value: &[u8]) {
    let length = u32::try_from(value.len()).unwrap_or_else(|_| panic!("receipt field overflow"));
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(value);
}

fn encode_receipt(fields: &ReceiptFields, signature: Option<[u8; 64]>) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&layerx_wire::limits::PROTOCOL_VERSION.to_be_bytes());
    bytes.extend_from_slice(&0x5201_u16.to_be_bytes());
    bytes.extend_from_slice(&layerx_wire::limits::PROTOCOL_VERSION.to_be_bytes());
    push_bytes(&mut bytes, &fields.activity_id);
    bytes.extend_from_slice(&17_u64.to_be_bytes());
    push_bytes(&mut bytes, &fields.previous_state_root);
    push_bytes(&mut bytes, &fields.resulting_state_root);
    push_bytes(&mut bytes, &[0x81; 32]);
    bytes.extend_from_slice(&0_i32.to_be_bytes());
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    bytes.extend_from_slice(&1_u128.to_be_bytes());
    push_bytes(&mut bytes, &fields.batch_id);
    bytes.extend_from_slice(&1_u16.to_be_bytes());
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.push(1);
    push_bytes(&mut bytes, &fields.asset);
    bytes.extend_from_slice(&25_u128.to_be_bytes());
    push_bytes(&mut bytes, &[0x91; 32]);
    bytes.extend_from_slice(&100_u128.to_be_bytes());
    bytes.extend_from_slice(&75_u128.to_be_bytes());
    bytes.extend_from_slice(&1_u64.to_be_bytes());
    push_bytes(&mut bytes, &[0x92; 32]);
    bytes.extend_from_slice(&10_u128.to_be_bytes());
    bytes.extend_from_slice(&35_u128.to_be_bytes());
    push_bytes(&mut bytes, &[0x93; 32]);
    push_bytes(&mut bytes, &[0x94; 32]);
    push_bytes(&mut bytes, &[0x95; 32]);
    bytes.extend_from_slice(&1_000_u64.to_be_bytes());
    bytes.push(u8::from(signature.is_some()));
    if let Some(signature) = signature {
        push_bytes(&mut bytes, &signature);
    }
    bytes
}

fn verified_receipt() -> VerifiedReceipt {
    let fields = ReceiptFields {
        activity_id: [0x11; 32],
        previous_state_root: [0x12; 32],
        resulting_state_root: [0x13; 32],
        batch_id: [0x14; 32],
        asset: [0x15; 32],
    };
    let signing_key = SigningKey::from_bytes(&[0x16; 32]);
    let unsigned = encode_receipt(&fields, None);
    let mut digest = Sha256::new();
    digest.update(b"LXP/v1/receipt\0");
    digest.update(&unsigned);
    let signature = signing_key.sign(&<[u8; 32]>::from(digest.finalize()));
    let canonical = encode_receipt(&fields, Some(signature.to_bytes()));
    let authorised = AuthorizedBatch::new(
        fields.batch_id,
        fields.asset,
        fields.previous_state_root,
        fields.resulting_state_root,
        signing_key.verifying_key().to_bytes(),
    );
    let receipt = verify(&canonical, &authorised)
        .unwrap_or_else(|error| panic!("receipt verification: {error:?}"));
    assert_eq!(receipt.level(), VerificationLevel::SEQUENCER_SIGNED);
    receipt
}

fn descriptor(index: usize, kind: ActivityKind) -> AgentActivity {
    let pending = match kind {
        ActivityKind::Deposit => PendingStatus::Deposit(DepositStage::Crediting),
        ActivityKind::Withdrawal => {
            PendingStatus::Withdrawal(WithdrawalStage::WaitingForSettlement)
        }
        ActivityKind::Approval => PendingStatus::WaitingForYou,
        ActivityKind::Movement | ActivityKind::AgentAction | ActivityKind::Security => {
            PendingStatus::Processing
        }
    };
    let verified = match kind {
        ActivityKind::Deposit => VerifiedStatus::DepositDone,
        ActivityKind::Withdrawal => VerifiedStatus::WithdrawalPaidOut,
        ActivityKind::Movement
        | ActivityKind::AgentAction
        | ActivityKind::Approval
        | ActivityKind::Security => VerifiedStatus::Done,
    };
    AgentActivity::new(
        ActivityEntryId::new(format!("act_detail{index}"))
            .unwrap_or_else(|error| panic!("entry id: {error}")),
        kind,
        (!matches!(kind, ActivityKind::Security)).then(|| "did:layerx:agent-a".to_owned()),
        100 + u64::try_from(index).unwrap_or(0),
        pending,
        verified,
    )
    .unwrap_or_else(|error| panic!("activity descriptor: {error}"))
}

#[test]
fn assembles_every_activity_class_from_verified_receipt_actuals() {
    let root = support::directory("activity-detail-classes");
    let tenancy = support::tenancy(&[("alice", "tenant-a")]);
    let (mut store, _) =
        support::install_and_open(&root, &tenancy, support::retention_uniform(100));
    let mut scope = store
        .principal(&support::principal("alice"))
        .unwrap_or_else(|error| panic!("scope: {error}"));
    let receipt = verified_receipt();
    let actual = ReceiptActual::from_verified(&receipt)
        .unwrap_or_else(|error| panic!("receipt actual: {error}"));
    let receipt_ref = ReceiptRef::new(actual.reference())
        .unwrap_or_else(|error| panic!("receipt reference: {error:?}"));
    let kinds = [
        ActivityKind::Deposit,
        ActivityKind::Withdrawal,
        ActivityKind::Movement,
        ActivityKind::AgentAction,
        ActivityKind::Approval,
        ActivityKind::Security,
    ];

    for (index, kind) in kinds.into_iter().enumerate() {
        let finalised = kind == ActivityKind::Movement;
        let level = if finalised {
            Level::CheckpointFinalised
        } else {
            Level::SequencerSigned
        };
        let sequence = u64::try_from(index).unwrap_or(0).saturating_add(1);
        let delivery = EventDelivery::new(
            EventIdentity::new([u8::try_from(sequence).unwrap_or(1); 32]),
            receipt.canonical_bytes().to_vec(),
            ApiCursor(Sequence(sequence)),
            ReceiptReference::Verified {
                receipt_ref: receipt_ref.clone(),
                verification_level: level,
            },
        )
        .unwrap_or_else(|error| panic!("delivery: {error:?}"));
        let entry = Feed::record_agent_event(
            &mut scope,
            &descriptor(index, kind),
            &delivery,
            200 + sequence,
        )
        .unwrap_or_else(|error| panic!("feed entry: {error}"));
        let finality = finalised.then(|| {
            FinalityReference::new([0x77; 32], Level::CheckpointFinalised)
                .unwrap_or_else(|error| panic!("finality: {error}"))
        });
        let detail = EntryDetail::assemble(&entry, vec![actual.clone()], finality)
            .unwrap_or_else(|error| panic!("detail: {error}"));

        assert_eq!(detail.kind(), kind);
        assert!(detail.sentence().ends_with('.'));
        assert!(!detail.stages().is_empty());
        assert_eq!(
            detail
                .stages()
                .last()
                .unwrap_or_else(|| panic!("timeline empty"))
                .state(),
            StageState::Complete
        );
        assert_eq!(detail.actuals()[0].amount(), 25);
        assert_eq!(detail.actuals()[0].fee(), 1);
        assert_eq!(detail.actuals()[0].asset(), [0x15; 32]);
        assert_eq!(detail.evidence()[0].kind(), EvidenceKind::Receipt);
        assert!(detail.evidence()[0]
            .path()
            .starts_with("/explorer/receipts/"));
        if finalised {
            assert_eq!(detail.evidence().len(), 2);
            assert_eq!(detail.evidence()[1].kind(), EvidenceKind::Checkpoint);
            assert!(detail.evidence()[1]
                .path()
                .starts_with("/explorer/checkpoints/"));
        }
    }
    fs::remove_dir_all(root).unwrap_or_else(|error| panic!("cleanup: {error}"));
}

#[test]
fn refuses_unbacked_actuals_and_states_refusal_funds_disposition() {
    let root = support::directory("activity-detail-refusal");
    let tenancy = support::tenancy(&[("alice", "tenant-a")]);
    let (mut store, _) =
        support::install_and_open(&root, &tenancy, support::retention_uniform(100));
    let mut scope = store
        .principal(&support::principal("alice"))
        .unwrap_or_else(|error| panic!("scope: {error}"));
    let receipt = verified_receipt();
    let actual = ReceiptActual::from_verified(&receipt)
        .unwrap_or_else(|error| panic!("receipt actual: {error}"));
    let completed_delivery = EventDelivery::new(
        EventIdentity::new([1; 32]),
        receipt.canonical_bytes().to_vec(),
        ApiCursor(Sequence(1)),
        ReceiptReference::Verified {
            receipt_ref: ReceiptRef::new(actual.reference())
                .unwrap_or_else(|error| panic!("receipt reference: {error:?}")),
            verification_level: Level::SequencerSigned,
        },
    )
    .unwrap_or_else(|error| panic!("delivery: {error:?}"));
    let completed = Feed::record_agent_event(
        &mut scope,
        &descriptor(1, ActivityKind::Movement),
        &completed_delivery,
        1,
    )
    .unwrap_or_else(|error| panic!("completed entry: {error}"));
    assert_eq!(
        EntryDetail::assemble(&completed, Vec::new(), None),
        Err(DetailError::ReceiptMaterialMismatch)
    );
    assert_eq!(
        EntryDetail::assemble(
            &completed,
            vec![actual],
            Some(
                FinalityReference::new([9; 32], Level::CheckpointFinalised)
                    .unwrap_or_else(|error| panic!("finality: {error}")),
            ),
        ),
        Err(DetailError::UnjustifiedFinalityEvidence)
    );

    let refusal_delivery = EventDelivery::new(
        EventIdentity::new([2; 32]),
        vec![1, 2, 3],
        ApiCursor(Sequence(2)),
        ReceiptReference::None,
    )
    .unwrap_or_else(|error| panic!("refusal delivery: {error:?}"));
    let refusal_descriptor = AgentActivity::new(
        ActivityEntryId::new("act_refused").unwrap_or_else(|error| panic!("entry id: {error}")),
        ActivityKind::Movement,
        None,
        2,
        PendingStatus::DidntGoThrough { money_left: true },
        VerifiedStatus::Done,
    )
    .unwrap_or_else(|error| panic!("refusal descriptor: {error}"));
    let refused = Feed::record_agent_event(&mut scope, &refusal_descriptor, &refusal_delivery, 2)
        .unwrap_or_else(|error| panic!("refused entry: {error}"));
    let detail = EntryDetail::assemble(&refused, Vec::new(), None)
        .unwrap_or_else(|error| panic!("refusal detail: {error}"));
    assert_eq!(detail.refusal_money_left(), Some(true));
    assert!(detail.sentence().contains("money had already left"));
    assert!(detail
        .stages()
        .iter()
        .any(|stage| stage.state() == StageState::Failed));
    fs::remove_dir_all(root).unwrap_or_else(|error| panic!("cleanup: {error}"));
}
