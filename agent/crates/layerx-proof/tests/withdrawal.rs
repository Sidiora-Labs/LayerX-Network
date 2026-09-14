use layerx_proof::receipt::{
    verify_outcome, verify_sequencer_signature, withdrawal, AuthorizedBatch,
};

#[test]
fn historical_withdrawal_without_ledger_binding_is_not_verified() -> Result<(), String> {
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
    .map_err(|error| format!("original native header: {error:?}"))?;
    let signed = verify_sequencer_signature(receipt, key)
        .map_err(|error| format!("original native signature: {error:?}"))?;
    let protocol = signed.protocol().ok_or("native receipt required")?;
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
    Ok(())
}
