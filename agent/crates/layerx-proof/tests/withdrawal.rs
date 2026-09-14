use layerx_proof::receipt::{
    verify_outcome, verify_sequencer_signature, withdrawal, AuthorizedBatch,
};

#[test]
fn historical_withdrawal_without_ledger_binding_is_not_verified() {
    let receipt =
        include_bytes!("../../../../tests/fixtures/asset/unbound-native-withdrawal/receipt");
    let activity =
        include_bytes!("../../../../tests/fixtures/asset/unbound-native-withdrawal/activity.lxa");
    let key = *include_bytes!(
        "../../../../tests/fixtures/asset/unbound-native-withdrawal/sequencer.public"
    );
    let header = layerx_wire::receipt::decode_batch_header(include_bytes!(
        "../../../../tests/fixtures/asset/unbound-native-withdrawal/header"
    ))
    .expect("original native header");
    let signed = verify_sequencer_signature(receipt, key).expect("original native signature");
    let protocol = signed.protocol().expect("native receipt");
    assert_eq!(
        (
            protocol.module_id(),
            protocol.operation(),
            protocol.result_code()
        ),
        (1, 0, 0)
    );
    assert_eq!(protocol.fee_charged(), 17);
    assert!(protocol.effects().is_empty());
    let authorized = AuthorizedBatch::new(
        protocol.batch_id(),
        protocol.asset(),
        protocol.previous_state_root(),
        protocol.resulting_state_root(),
        key,
    );
    assert!(verify_outcome(receipt, &authorized).is_err());
    assert!(withdrawal::verify(receipt, &authorized, activity, header.network_id()).is_err());
}
