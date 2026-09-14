use layerx_proof::signed_authority::SignedAuthorityHistory;
use layerx_wire::activity::decode_signed;
use layerx_wire::handover::decode_genesis_trust;

const GENESIS: &[u8] = include_bytes!("fixtures/signed-authority/genesis.bin");
const HEADERS: &[u8] = include_bytes!("fixtures/signed-authority/headers.bin");
const ACTIVITY: &[u8] = include_bytes!("fixtures/signed-authority/handover.activity");

fn must<T, E: std::fmt::Debug>(value: Result<T, E>) -> T {
    value.unwrap_or_else(|error| panic!("native signed authority: {error:?}"))
}

#[test]
fn native_governance_history_retains_signed_ranges_and_refuses_changed_or_missing_parents() {
    let genesis = must(decode_genesis_trust(GENESIS));
    let activity = must(decode_signed(ACTIVITY, &genesis.registry));
    let make = || {
        must(SignedAuthorityHistory::from_genesis(
            genesis.network_id,
            genesis.canonical_state_root,
            genesis.initial_sequencer_key,
            genesis.governance_witness,
        ))
    };
    let mut history = make();
    assert_eq!(HEADERS.len(), 15 * 418);
    let second = &HEADERS[418..836];
    assert!(history
        .advance(&second[..354], &must(second[354..].try_into()), None)
        .is_err());
    assert!(history.verified_head().is_none());
    for (index, record) in HEADERS.chunks_exact(418).enumerate() {
        let header = &record[..354];
        let signature: [u8; 64] = must(record[354..].try_into());
        assert!(history.verify_header(header, &signature).is_err());
        let before = history.clone();
        let packet = (index >= 13).then_some(activity.payload());
        for end in [0, 1, 353] {
            assert!(history.advance(&header[..end], &signature, packet).is_err());
            assert_eq!(history, before);
        }
        let mut forged = header.to_vec();
        forged[100] ^= 1;
        assert!(history.advance(&forged, &signature, packet).is_err());
        assert_eq!(history, before);
        if index == 13 {
            assert!(history.advance(header, &signature, None).is_err());
            for end in [0, 1, activity.payload().len() - 1] {
                assert!(history
                    .advance(header, &signature, Some(&activity.payload()[..end]))
                    .is_err());
            }
            let mut changed = activity.payload().to_vec();
            changed[100] ^= 1;
            assert!(history.advance(header, &signature, Some(&changed)).is_err());
            assert_eq!(history, before);
        }
        must(history.advance(header, &signature, packet));
        must(history.verify_header(header, &signature));
        assert!(history.advance(header, &signature, packet).is_err());
    }
    assert_eq!(history.intervals().len(), 2);
    assert_eq!(
        (
            history.intervals()[0].first_batch(),
            history.intervals()[0].last_batch()
        ),
        (1, 13)
    );
    assert_eq!(
        (
            history.intervals()[1].first_batch(),
            history.intervals()[1].last_batch()
        ),
        (14, 15)
    );
    for record in HEADERS.chunks_exact(418) {
        must(history.verify_header(&record[..354], &must(record[354..].try_into())));
    }
    let mut wrong_root = genesis.canonical_state_root;
    wrong_root[0] ^= 1;
    assert!(SignedAuthorityHistory::from_genesis(
        genesis.network_id,
        wrong_root,
        genesis.initial_sequencer_key,
        genesis.governance_witness
    )
    .is_err());
    assert!(make()
        .verify_header(&HEADERS[..354], &must(HEADERS[354..418].try_into()))
        .is_err());
}
