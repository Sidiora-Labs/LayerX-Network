use layerx_paxeer_client::{account_address, account_address_for_protocol};
use layerx_types::account::AccountId;

#[test]
fn protocol_three_matches_native_ledger_account_vectors() {
    for (name, expected) in [
        (
            "agent:did:key:alice:main",
            "efc9802f76722dfc48ebfed35bfd8b20dbc2775fe2f027d6cbd595aff1307454",
        ),
        (
            "system:paxeer-reserve",
            "6e0e5cca5cfaa1b20ddd1c6174321eeaf00dd74bce2adcd78b61e15aa9e26f7c",
        ),
        (
            "system:paxeer-withdrawals",
            "36e5bfab4a0143c723aaac6a61d6eae554e684f94cfff2a24f5fdf0574c468f7",
        ),
    ] {
        let account = AccountId::parse(name).unwrap_or_else(|error| panic!("{error:?}"));
        let actual =
            account_address_for_protocol(&account, 3).unwrap_or_else(|error| panic!("{error:?}"));
        let expected_bytes: Vec<u8> = (0..expected.len())
            .step_by(2)
            .map(|offset| {
                u8::from_str_radix(&expected[offset..offset + 2], 16)
                    .unwrap_or_else(|error| panic!("{error}"))
            })
            .collect();
        assert_eq!(
            actual.as_slice(),
            expected_bytes,
            "native tests/ledger/test_account_id.c: {name}"
        );
        assert_ne!(actual, account_address(&account));
        assert_eq!(
            account_address_for_protocol(&account, 2),
            Ok(account_address(&account))
        );
    }
}
