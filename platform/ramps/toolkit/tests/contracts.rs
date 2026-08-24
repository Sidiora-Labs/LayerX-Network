use std::fs::{self, OpenOptions};
use std::path::PathBuf;

use layerx_ramp_toolkit::journal::{Journal, TransitionEvidence, WorkflowStage};
use layerx_ramp_toolkit::{
    operator_send_authorization_message, AggregateStatus, AuthenticatedPrincipal, CreateOrder,
    OperatorIdentity, QuoteTerms, RampDirection, RampError, RampOrder, EXTERNAL_CUSTODY_LABEL,
};

fn order(direction: RampDirection, customer: &str) -> RampOrder {
    let payer_grant = match direction {
        RampDirection::OnRamp => None,
        RampDirection::OffRamp => Some([7; 32]),
    };
    RampOrder::bind(
        CreateOrder {
            order_id: "order-1".to_owned(),
            quote_id: "quote-1".to_owned(),
            payer_grant,
        },
        QuoteTerms {
            quote_id: "quote-1".to_owned(),
            direction,
            layerx_asset: [1; 32],
            layerx_amount: 1_000,
            external_currency: "EUR".to_owned(),
            external_amount_minor: 100,
            rate_numerator: 10,
            rate_denominator: 1,
            fee_minor: 2,
            maximum_slippage_bps: 25,
            context: [8; 32],
            provider_token: "product-eur".to_owned(),
            payout_token: "beneficiary-123".to_owned(),
            expires_at: 2_000,
        },
        AuthenticatedPrincipal {
            principal_id: customer.to_owned(),
            account: format!("agent:did:layerx:{customer}:main"),
        },
        OperatorIdentity {
            principal_id: "operator-1".to_owned(),
            account: "agent:did:layerx:operator-1:main".to_owned(),
            signer_key_handle: "kms.operator-1.primary".to_owned(),
        },
        1_000,
    )
    .unwrap_or_else(|error| panic!("bind order: {error:?}"))
}

fn journal_path(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("layerx-ramp-{name}-{}.jsonl", std::process::id()));
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)
        .unwrap_or_else(|error| panic!("create journal: {error}"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .unwrap_or_else(|error| panic!("secure journal: {error}"));
    }
    drop(file);
    path
}

#[test]
fn digest_binds_authenticated_customer_and_direction() {
    let first = order(RampDirection::OnRamp, "customer-a");
    let another_customer = order(RampDirection::OnRamp, "customer-b");
    let opposite_direction = order(RampDirection::OffRamp, "customer-a");
    assert_ne!(first.order_digest, another_customer.order_digest);
    assert_ne!(first.order_digest, opposite_direction.order_digest);
    assert_eq!(first.context, [8; 32]);
}

#[test]
fn payment_direction_selects_direct_send_or_customer_grant() {
    let on_ramp = order(RampDirection::OnRamp, "customer-a");
    let off_ramp = order(RampDirection::OffRamp, "customer-a");
    assert_eq!(on_ramp.payer_grant, None);
    assert_eq!(off_ramp.payer_grant, Some([7; 32]));
    let authorization = operator_send_authorization_message(&on_ramp, 9, 1, 1)
        .unwrap_or_else(|error| panic!("authorization message: {error:?}"));
    assert_eq!(authorization.len(), 266);
    assert_eq!(&authorization[..2], &0x5301_u16.to_be_bytes());
    assert_eq!(
        operator_send_authorization_message(&off_ramp, 9, 1, 1),
        Err(RampError::InvalidOrder)
    );
}

#[test]
fn done_requires_both_verified_legs_and_external_label() {
    let path = journal_path("done-gate");
    let bound = order(RampDirection::OnRamp, "customer-a");
    let digest = bound.order_digest;
    let mut journal = Journal::open(&path)
        .unwrap_or_else(|error| panic!("open journal: {error:?}"));
    journal
        .create_order(bound, 1_001)
        .unwrap_or_else(|error| panic!("create order: {error:?}"));
    journal
        .acquire_lease(digest, "test-worker", 1_001, 30)
        .unwrap_or_else(|error| panic!("lease: {error:?}"));
    journal
        .transition(
            digest,
            WorkflowStage::CompliancePending,
            WorkflowStage::AwaitingExternalCredit,
            TransitionEvidence::empty(),
            "test-worker",
            1_002,
        )
        .unwrap_or_else(|error| panic!("approve: {error:?}"));
    let direct_done = journal.transition(
        digest,
        WorkflowStage::AwaitingExternalCredit,
        WorkflowStage::Done,
        TransitionEvidence::empty(),
        "test-worker",
        1_003,
    );
    assert_eq!(direct_done, Err(RampError::IllegalTransition));
    let presentation = journal
        .order(&digest)
        .unwrap_or_else(|| panic!("order missing"))
        .presentation();
    assert_eq!(presentation.status, AggregateStatus::Pending);
    assert_eq!(presentation.external_custody_label, EXTERNAL_CUSTODY_LABEL);
    drop(journal);
    fs::remove_file(&path).unwrap_or_else(|error| panic!("remove journal: {error}"));
    #[cfg(unix)]
    {
        let mut lock = path.into_os_string();
        lock.push(".writer-lock");
        fs::remove_file(PathBuf::from(lock))
            .unwrap_or_else(|error| panic!("remove journal lock: {error}"));
    }
}
