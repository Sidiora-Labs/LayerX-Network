use layerx_program_sdk::{payments::PreparedProgramAccount, AccountId, Amount, AssetId};
use layerx_programs_runtime::{
    abi::{Capability, CapabilitySet},
    derive_program_account, ProgramId,
};

#[test]
fn sdk_preparation_matches_runtime_derivation_and_spend_decoder() {
    for seed in [b"".as_slice(), b"merchant", &[0xff; 128]] {
        let program = ProgramId::new([1; 32]).unwrap_or_else(|e| panic!("{e}"));
        let sdk_program =
            layerx_program_sdk::ProgramId::new(program.bytes()).unwrap_or_else(|e| panic!("{e}"));
        let asset = AssetId::new([2; 32]).unwrap_or_else(|e| panic!("{e}"));
        let to = AccountId::new([3; 32]).unwrap_or_else(|e| panic!("{e}"));
        let prepared =
            PreparedProgramAccount::new(sdk_program, seed, asset).unwrap_or_else(|e| panic!("{e}"));
        let derived = derive_program_account(program, seed).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(prepared.account().bytes(), derived.bytes());
        let payload = prepared
            .registration_payload()
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(&payload.as_slice()[..32], &program.bytes());
        assert_eq!(&payload.as_slice()[32..37], b"LXPA1");
        assert_eq!(&payload.as_slice()[37..69], &asset.bytes());
        assert_eq!(
            &payload.as_slice()[69..73],
            &u32::try_from(seed.len())
                .unwrap_or_else(|e| panic!("{e}"))
                .to_be_bytes()
        );
        assert_eq!(&payload.as_slice()[73..], seed);
        let encoded = prepared
            .spend_grant(to, Amount::from_u128(u128::MAX))
            .unwrap_or_else(|e| panic!("{e}"));
        let expected = Capability::ProgramSpend {
            owner_program: program,
            seed: seed.to_vec(),
            source_account: derived.bytes(),
            asset: asset.bytes(),
            to: to.bytes(),
            maximum_amount: u128::MAX,
        };
        assert_eq!(
            CapabilitySet::decode_v2_canonical(encoded.as_slice()),
            Ok(vec![expected.clone()])
        );
        assert_eq!(
            CapabilitySet::new([expected])
                .unwrap_or_else(|e| panic!("{e}"))
                .canonical_encoding(),
            encoded.as_slice()
        );
        assert!(prepared.spend_grant(to, Amount::ZERO).is_err());
        assert!(prepared.funding_grant(Amount::ZERO).is_err());
        assert!(prepared.deposit(Amount::ZERO).is_err());
        assert!(prepared.payment(to, Amount::ZERO).is_err());
        let mut corrupted = encoded.as_slice().to_vec();
        corrupted[37 + seed.len()] ^= 1;
        assert!(CapabilitySet::decode_v2_canonical(&corrupted).is_err());
        assert!(PreparedProgramAccount::new(sdk_program, &[0; 129], asset).is_err());
    }
}
