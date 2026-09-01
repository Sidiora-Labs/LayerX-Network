use layerx_agent_api::availability::{
    AvailabilityClass, AvailabilityCompletion, AvailabilityReport, ClassReport, ProviderRef,
    ProviderReport,
};
use layerx_agent_api::export::{FactRef, OfflineExport};
use layerx_agent_api::prepare::CanonicalBytes;
use layerx_agent_api::proof::ProofBundle;
use layerx_agent_api::read::{
    AccountRef, AccountValue, BalanceValue, BatchRef, BatchValue, CheckpointRef, CheckpointValue,
    Freshness, HistoryValue, ModuleStateValue, ProjectionResult, RelativeTo, VerifiedRead,
};
use layerx_agent_api::verify::Level;
use layerx_agent_api::{Amount, Sequence};

const SCHEMA: &str = include_str!("../../../schema/agent-api/read.kvx");

fn required<T>(result: Result<T, layerx_agent_api::identity::ContractError>) -> T {
    result.unwrap_or_else(|error| panic!("valid contract value: {error:?}"))
}

fn bytes(value: u8) -> CanonicalBytes {
    required(CanonicalBytes::new(vec![value]))
}

fn freshness() -> Freshness {
    Freshness {
        chain_head: Sequence(50),
        latest_sealed_batch: required(BatchRef::new("batch-4")),
        latest_finalised_checkpoint: required(CheckpointRef::new("checkpoint-3")),
        value_sequence: Sequence(47),
        relative_to: RelativeTo::Batch(required(BatchRef::new("batch-4"))),
    }
}

fn assert_verified<T: layerx_agent_api::read::CoreProduced>(value: &VerifiedRead<T>) {
    assert_eq!(value.achieved_verification_level, Level::StateProven);
    assert_eq!(value.freshness.chain_head, Sequence(50));
    assert_eq!(value.freshness.latest_sealed_batch.as_str(), "batch-4");
    assert_eq!(
        value.freshness.latest_finalised_checkpoint.as_str(),
        "checkpoint-3"
    );
}

#[test]
fn every_authoritative_read_shape_carries_level_and_freshness() {
    let level = Level::StateProven;
    let values = (
        VerifiedRead::new(AccountValue(bytes(1)), level, freshness()),
        VerifiedRead::new(ModuleStateValue(bytes(2)), level, freshness()),
        VerifiedRead::new(
            HistoryValue {
                records: vec![bytes(3)],
                next_cursor: None,
            },
            level,
            freshness(),
        ),
        VerifiedRead::new(BatchValue(bytes(4)), level, freshness()),
        VerifiedRead::new(CheckpointValue(bytes(5)), level, freshness()),
        VerifiedRead::new(
            ProofBundle {
                target: bytes(6),
                proofs: vec![bytes(7)],
            },
            level,
            freshness(),
        ),
    );
    assert_verified(&values.0);
    assert_verified(&values.1);
    assert_verified(&values.2);
    assert_verified(&values.3);
    assert_verified(&values.4);
    assert_verified(&values.5);
    assert!(SCHEMA.contains("required = [\"value\",\"achieved_verification_level\",\"freshness\"]"));
}

#[test]
fn balance_preserves_identity_amount_sequence_and_reference() {
    let balance = VerifiedRead::new(
        BalanceValue {
            account: required(AccountRef::new("account-1")),
            asset: required(layerx_agent_api::identity::Asset::new("LXP")),
            amount: Amount(9_007_199_254_740_993),
            canonical_state: bytes(8),
        },
        Level::StateProven,
        freshness(),
    );
    assert_eq!(balance.value.amount, Amount(9_007_199_254_740_993));
    assert_eq!(balance.freshness.value_sequence, Sequence(47));
    assert_verified(&balance);
}

#[test]
fn availability_reports_complete_provider_or_attributed_partials() {
    let provider = required(ProviderRef::new("provider-a"));
    let class = ClassReport {
        class: AvailabilityClass::Recovery,
        complete: false,
        verified_chunks: 2,
        verified_bytes: 128,
        failure: Some("withheld final chunk".to_owned()),
    };
    let report = AvailabilityReport {
        completion: AvailabilityCompletion::Partial,
        classes: vec![class.clone()],
        providers: vec![ProviderReport {
            provider,
            classes: vec![class],
            failure: Some("incomplete".to_owned()),
        }],
    };
    let read = VerifiedRead::new(report, Level::BatchIncluded, freshness());
    assert!(matches!(
        read.value.completion,
        AvailabilityCompletion::Partial
    ));
    assert_eq!(read.value.providers.len(), 1);
    assert!(SCHEMA.contains("bytes are never merged across providers"));
}

#[test]
fn offline_export_is_self_contained_and_projection_is_distinct() {
    let export = OfflineExport {
        facts: vec![required(FactRef::new("fact-1"))],
        receipts: vec![bytes(9)],
        proofs: vec![bytes(10)],
        certificates: vec![bytes(11)],
        headers: vec![bytes(12)],
    }
    .validate()
    .unwrap_or_else(|error| panic!("offline export: {error:?}"));
    let verified = VerifiedRead::new(export, Level::CheckpointFinalised, freshness());
    assert_eq!(verified.value.facts.len(), 1);

    let projection = ProjectionResult::new(Amount(22), "fee estimate only", freshness())
        .unwrap_or_else(|error| panic!("projection: {error:?}"));
    assert_eq!(projection.projected, Amount(22));
    assert!(SCHEMA.contains("forbidden_response = \"VerifiedRead\""));
    assert!(SCHEMA.contains("layerx-proof_without_daemon_or_network"));
}
