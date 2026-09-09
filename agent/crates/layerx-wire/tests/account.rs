use layerx_types::account::AccountId;
use layerx_wire::account::account_name_for_asset;
use layerx_wire::hash::account_id_for_protocol;

#[test]
fn shared_builder_preserves_canonical_names_and_rejects_mismatch() {
    let did = "did:layerx:alice";
    let native = account_name_for_asset(did, [0; 32], [0; 32]).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(native.canonical(), "agent:did:layerx:alice:main");
    let token =
        account_name_for_asset(did, [0xab; 32], [0; 32]).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        token.canonical(),
        format!("agent:{did}:asset:{}", "ab".repeat(32))
    );
    assert_ne!(
        account_id_for_protocol(&native, 3),
        account_id_for_protocol(&token, 3)
    );
    assert!(token.matches_asset(did, [0xab; 32], [0; 32]).is_ok());
    assert!(token.matches_asset(did, [0xac; 32], [0; 32]).is_err());
    assert!(token
        .matches_asset("did:layerx:bob", [0xab; 32], [0; 32])
        .is_err());
    for suffix in [
        "AB".repeat(32),
        "ab".repeat(31),
        "ab".repeat(33),
        "zz".repeat(32),
    ] {
        assert!(AccountId::parse(&format!("agent:{did}:asset:{suffix}")).is_err());
    }
    for did in [
        "",
        "did::alice",
        "did:layerx:ALICE",
        "did:layerx:alice:asset:aa",
    ] {
        assert!(account_name_for_asset(did, [1; 32], [0; 32]).is_err());
    }
}
