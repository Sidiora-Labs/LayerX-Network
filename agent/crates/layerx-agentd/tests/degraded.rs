use std::collections::BTreeSet;

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_agentd::cache::CacheValue;
use layerx_agentd::degraded::{
    enter, Controller, Mode, Observation, OperationError, ReadError, Reference,
};
use layerx_agentd::obs::health::{BoundaryConnectivity, DegradedMode, HealthInput, WriteReadiness};
use layerx_agentd::obs::metrics::{MetricKind, MetricLabel, Metrics};
use layerx_agentd::store::TenantId;
use layerx_proof::inclusion::{verify_state, SequencerAuthorization};
use layerx_proof::merkle::build_proof;
use layerx_types::verify::VerificationLevel;
use layerx_wire::encode::Encoder;
use layerx_wire::hash::batch_header_digest;

fn tenant() -> TenantId {
    TenantId::new("tenant-a").unwrap_or_else(|error| panic!("tenant: {error}"))
}

fn reference(head_sequence: u64, checkpoint: u8) -> Reference {
    Reference {
        head_sequence,
        checkpoint: [checkpoint; 32],
    }
}

fn header_bytes(state_root: [u8; 32], activity_root: [u8; 32], sequencer: [u8; 32]) -> Vec<u8> {
    let mut encoder = Encoder::new(354);
    assert_eq!(
        encoder.structure_header_version(0x1701, layerx_wire::limits::PROTOCOL_VERSION),
        Ok(())
    );
    assert_eq!(encoder.u8(15), Ok(()));
    for field in 1..=15 {
        assert_eq!(encoder.tag(field, 15), Ok(()));
        match field {
            1 => assert_eq!(encoder.u16(layerx_wire::limits::PROTOCOL_VERSION), Ok(())),
            2 => assert_eq!(encoder.u32(42), Ok(())),
            3 => assert_eq!(encoder.u64(2), Ok(())),
            4 => assert_eq!(encoder.u64(7), Ok(())),
            5 => assert_eq!(encoder.u64(9), Ok(())),
            6 => assert_eq!(encoder.u64(10), Ok(())),
            7 => assert_eq!(encoder.bytes(&[1; 32], 32), Ok(())),
            8 => assert_eq!(encoder.bytes(&state_root, 32), Ok(())),
            9 => assert_eq!(encoder.bytes(&activity_root, 32), Ok(())),
            10 => assert_eq!(encoder.bytes(&[2; 32], 32), Ok(())),
            11 => assert_eq!(encoder.bytes(&[3; 32], 32), Ok(())),
            12 => assert_eq!(encoder.bytes(&[4; 32], 32), Ok(())),
            13 => assert_eq!(encoder.bytes(&[5; 32], 32), Ok(())),
            14 => assert_eq!(encoder.u64(1_000), Ok(())),
            15 => assert_eq!(encoder.bytes(&sequencer, 32), Ok(())),
            _ => panic!("unreachable header field"),
        }
    }
    encoder.finish()
}

fn verified_cache() -> CacheValue {
    let state = b"core-state";
    let (proof, state_root) = build_proof(&[state.as_slice()], 0)
        .unwrap_or_else(|error| panic!("state proof: {error:?}"));
    let (_, activity_root) = build_proof(&[b"core-activity".as_slice()], 0)
        .unwrap_or_else(|error| panic!("activity proof: {error:?}"));
    let key = SigningKey::from_bytes(&[7; 32]);
    let sequencer = key.verifying_key().to_bytes();
    let header = header_bytes(state_root, activity_root, sequencer);
    let digest =
        batch_header_digest(&header).unwrap_or_else(|error| panic!("header digest: {error:?}"));
    let evidence = verify_state(
        state,
        &proof,
        &state_root,
        &header,
        &key.sign(&digest).to_bytes(),
        &SequencerAuthorization::new(sequencer, sequencer, 7, 7),
    )
    .unwrap_or_else(|error| panic!("state verification: {error:?}"));
    CacheValue::from_inclusion(state.to_vec(), &evidence, 100, [9; 32])
}

fn health_input() -> HealthInput {
    HealthInput {
        live: true,
        boundary: BoundaryConnectivity::Ready,
        audit_writable: true,
        recovery_complete: true,
        verification_backlog: 0,
        maximum_verification_backlog: 10,
        unknown_backlog: 0,
        maximum_unknown_backlog: 10,
        degraded_modes: BTreeSet::new(),
    }
}

#[test]
fn core_loss_mid_write_refuses_prepare_and_ack_but_resolves_unknown_after_reconnect() {
    let mut controller = Controller::default();
    enter(
        &mut controller,
        Observation::Ready {
            reference: reference(100, 9),
            maximum_verification: VerificationLevel::STATE_PROVEN,
        },
    );
    assert_eq!(controller.guard_preparation(|| 1), Ok(1));
    enter(&mut controller, Observation::Unreachable);

    let operation_ran = std::cell::Cell::new(false);
    assert_eq!(
        controller.guard_preparation(|| operation_ran.set(true)),
        Err(OperationError::LiveCoreRequired(Mode::Unreachable))
    );
    assert_eq!(
        controller.guard_submission_acknowledgement(|| operation_ran.set(true)),
        Err(OperationError::LiveCoreRequired(Mode::Unreachable))
    );
    assert!(!operation_ran.get());
    assert_eq!(
        controller.resolve_unknown_when_reachable(|| operation_ran.set(true)),
        Err(OperationError::ResolutionUnavailable)
    );
    assert!(!operation_ran.get());

    enter(
        &mut controller,
        Observation::Ready {
            reference: reference(101, 10),
            maximum_verification: VerificationLevel::STATE_PROVEN,
        },
    );
    assert_eq!(
        controller.resolve_unknown_when_reachable(|| operation_ran.set(true)),
        Ok(())
    );
    assert!(operation_ran.get());
}

#[test]
fn core_loss_mid_stream_serves_only_verified_cache_with_explicit_staleness_everywhere() {
    let cached = verified_cache();
    let mut controller = Controller::default();
    enter(
        &mut controller,
        Observation::Ready {
            reference: reference(100, 9),
            maximum_verification: VerificationLevel::STATE_PROVEN,
        },
    );
    let current = controller
        .serve_cached(&cached, VerificationLevel::STATE_PROVEN)
        .unwrap_or_else(|error| panic!("healthy cached read: {error:?}"));
    assert!(!current.staleness.stale);

    enter(&mut controller, Observation::Unreachable);
    let stream_ran = std::cell::Cell::new(false);
    assert_eq!(
        controller.guard_live_stream(|| stream_ran.set(true)),
        Err(OperationError::StreamUnavailable(Mode::Unreachable))
    );
    assert!(!stream_ran.get());
    let degraded = controller
        .serve_cached(&cached, VerificationLevel::STATE_PROVEN)
        .unwrap_or_else(|error| panic!("degraded cached read: {error:?}"));
    assert_eq!(degraded.canonical_core_bytes, b"core-state");
    assert_eq!(degraded.reported_level, VerificationLevel::STATE_PROVEN);
    assert!(degraded.staleness.stale);
    assert_eq!(degraded.staleness.value_head_sequence, 100);
    assert_eq!(degraded.staleness.value_checkpoint, [9; 32]);
    assert_eq!(
        degraded.staleness.observed_reference,
        Some(reference(100, 9))
    );

    let health = controller.health(health_input());
    assert_eq!(health.boundary, BoundaryConnectivity::Unavailable);
    assert!(health
        .degraded_modes
        .contains(&DegradedMode::CoreUnavailable));
    assert!(matches!(
        health.write_readiness,
        WriteReadiness::NotReady(_)
    ));
    let status = controller.status();
    assert_eq!(status.mode, Mode::Unreachable);
    assert!(!status.readiness.live_stream);
    assert_eq!(status.reference, Some(reference(100, 9)));

    let tenant = tenant();
    let mut metrics = Metrics::new(1).unwrap_or_else(|error| panic!("metrics: {error}"));
    metrics
        .register_tenant(tenant.clone())
        .unwrap_or_else(|error| panic!("register metrics tenant: {error}"));
    controller
        .record_metric(&mut metrics, &tenant)
        .unwrap_or_else(|error| panic!("record degraded metric: {error}"));
    let snapshot = metrics.snapshot(&tenant);
    assert!(snapshot.iter().any(|(key, point)| {
        key.kind == MetricKind::DegradedState
            && key.label == MetricLabel::BoundaryUnavailable
            && point.samples == 1
    }));
}

#[test]
fn lower_finality_on_return_is_visible_and_caps_previously_stronger_evidence() {
    let cached = verified_cache();
    let mut controller = Controller::default();
    enter(
        &mut controller,
        Observation::Ready {
            reference: reference(100, 9),
            maximum_verification: VerificationLevel::STATE_PROVEN,
        },
    );
    let status = enter(
        &mut controller,
        Observation::Ready {
            reference: reference(99, 8),
            maximum_verification: VerificationLevel::SEQUENCER_SIGNED,
        },
    );
    assert_eq!(status.mode, Mode::Behind);
    assert_eq!(
        status.maximum_verification,
        VerificationLevel::SEQUENCER_SIGNED
    );
    assert_eq!(
        controller.serve_cached(&cached, VerificationLevel::STATE_PROVEN),
        Err(ReadError::FinalityUnavailable {
            requested: VerificationLevel::STATE_PROVEN,
            supported: VerificationLevel::SEQUENCER_SIGNED,
        })
    );
    let lowered = controller
        .serve_cached(&cached, VerificationLevel::SEQUENCER_SIGNED)
        .unwrap_or_else(|error| panic!("lower-level cached read: {error:?}"));
    assert_eq!(lowered.held_level, VerificationLevel::STATE_PROVEN);
    assert_eq!(lowered.reported_level, VerificationLevel::SEQUENCER_SIGNED);
    assert!(lowered.staleness.stale);
    let health = controller.health(health_input());
    assert!(health.degraded_modes.contains(&DegradedMode::CoreBehind));
}

#[test]
fn halted_emergency_and_data_unavailable_modes_propagate_and_cap_reads() {
    let cases = [
        (
            Mode::Halted,
            Observation::Halted {
                reference: reference(100, 9),
                maximum_verification: VerificationLevel::BATCH_INCLUDED,
            },
            DegradedMode::CoreHalted,
            MetricLabel::CoreHalted,
        ),
        (
            Mode::Emergency,
            Observation::Emergency {
                reference: reference(100, 9),
                maximum_verification: VerificationLevel::SEQUENCER_SIGNED,
            },
            DegradedMode::Emergency,
            MetricLabel::CoreEmergency,
        ),
        (
            Mode::DataUnavailable,
            Observation::DataUnavailable {
                reference: reference(100, 9),
                maximum_verification: VerificationLevel::BATCH_INCLUDED,
            },
            DegradedMode::DataUnavailable,
            MetricLabel::DataUnavailable,
        ),
    ];
    for (mode, observation, health_mode, metric_label) in cases {
        let mut controller = Controller::default();
        enter(
            &mut controller,
            Observation::Ready {
                reference: reference(100, 9),
                maximum_verification: VerificationLevel::STATE_PROVEN,
            },
        );
        assert_eq!(enter(&mut controller, observation).mode, mode);
        assert!(controller
            .health(health_input())
            .degraded_modes
            .contains(&health_mode));
        assert_eq!(
            controller.serve_cached(&verified_cache(), VerificationLevel::STATE_PROVEN),
            Err(ReadError::FinalityUnavailable {
                requested: VerificationLevel::STATE_PROVEN,
                supported: controller.status().maximum_verification,
            })
        );
        let tenant = tenant();
        let mut metrics = Metrics::new(1).unwrap_or_else(|error| panic!("metrics: {error}"));
        metrics
            .register_tenant(tenant.clone())
            .unwrap_or_else(|error| panic!("register tenant: {error}"));
        controller
            .record_metric(&mut metrics, &tenant)
            .unwrap_or_else(|error| panic!("mode metric: {error}"));
        assert!(metrics
            .snapshot(&tenant)
            .iter()
            .any(|(key, _)| key.kind == MetricKind::DegradedState && key.label == metric_label));
    }
}
