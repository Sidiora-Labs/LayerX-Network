use layerx_platform_core::{asset_registry, build_send, main_account, SendRequest};

#[test]
fn beta_send_uses_native_account_ids_in_signed_payload_and_authorization() {
    let expected = "efc9802f76722dfc48ebfed35bfd8b20dbc2775fe2f027d6cbd595aff1307454";
    let source = main_account("did:key:alice").unwrap_or_else(|error| panic!("{error}"));
    let encoded = layerx_platform_core::hex_encode(&source);
    assert_eq!(encoded, expected);
    let signed = build_send(
        &[9; 32],
        &SendRequest {
            network_id: 42,
            source_did: "did:key:alice".into(),
            destination_did: "did:key:bob".into(),
            asset: [2; 32],
            amount: 25,
            account_sequence: 0,
            idempotency_key: [3; 32],
            not_before_ms: 1000,
            expires_at_ms: 2000,
            fee_limit: 1000,
        },
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let (registry, _) = asset_registry().unwrap_or_else(|error| panic!("{error}"));
    let activity = layerx_wire::activity::decode_signed(&signed.canonical, &registry)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(signed.source_account, source);
    assert_eq!(&activity.payload()[4..36], source);
    assert_eq!(&activity.payload()[36..68], signed.destination_account);
    assert_eq!(&activity.payload()[198..230], source);
    assert_eq!(&signed.canonical[..2], &[0, 3]);
}
