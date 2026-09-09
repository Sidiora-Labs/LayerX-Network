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

#[test]
fn mixed_sdk_payment_grants_match_runtime_order_and_refuse_duplicates() {
    use layerx_program_sdk::{
        payments::{PaymentGrant, ProgramPaymentCapabilities},
        Capability as SdkCapability, ReceiptDigest,
    };
    let owner = ProgramId::new([1; 32]).unwrap_or_else(|e| panic!("{e}"));
    let sdk_owner =
        layerx_program_sdk::ProgramId::new(owner.bytes()).unwrap_or_else(|e| panic!("{e}"));
    let asset = AssetId::new([2; 32]).unwrap_or_else(|e| panic!("{e}"));
    let to = AccountId::new([3; 32]).unwrap_or_else(|e| panic!("{e}"));
    let second = AccountId::new([4; 32]).unwrap_or_else(|e| panic!("{e}"));
    let receipt = ReceiptDigest::new([5; 32]).unwrap_or_else(|e| panic!("{e}"));
    let account = PreparedProgramAccount::new(sdk_owner, b"merchant", asset)
        .unwrap_or_else(|e| panic!("{e}"));
    let grants = [
        PaymentGrant::Basic(SdkCapability::SharedStorageWrite),
        PaymentGrant::Basic(SdkCapability::receipt_read(receipt)),
        PaymentGrant::ProgramSpend {
            account,
            to: second,
            maximum: Amount::from_u128(5),
        },
        PaymentGrant::Basic(
            account
                .funding_grant(Amount::from_u128(20))
                .unwrap_or_else(|e| panic!("{e}")),
        ),
        PaymentGrant::Basic(SdkCapability::StorageRead),
        PaymentGrant::ProgramSpend {
            account,
            to,
            maximum: Amount::from_u128(15),
        },
    ];
    let expected = CapabilitySet::new([
        Capability::SharedStorageWrite,
        Capability::ReceiptRead {
            receipt_digest: receipt.bytes(),
        },
        Capability::ProgramSpend {
            owner_program: owner,
            seed: b"merchant".to_vec(),
            source_account: account.account().bytes(),
            asset: asset.bytes(),
            to: second.bytes(),
            maximum_amount: 5,
        },
        Capability::Transfer402 {
            asset: asset.bytes(),
            to: account.account().bytes(),
            maximum_amount: 20,
        },
        Capability::StorageRead,
        Capability::ProgramSpend {
            owner_program: owner,
            seed: b"merchant".to_vec(),
            source_account: account.account().bytes(),
            asset: asset.bytes(),
            to: to.bytes(),
            maximum_amount: 15,
        },
    ])
    .unwrap_or_else(|e| panic!("{e}"));
    for ordered in [grants.to_vec(), grants.into_iter().rev().collect()] {
        let mut set = ProgramPaymentCapabilities::<8>::empty();
        assert!(set.is_empty());
        for grant in ordered {
            set.insert(grant).unwrap_or_else(|e| panic!("{e}"));
        }
        assert_eq!(set.len(), 6);
        let mut encoded = [0; 1024];
        let used = set
            .encode_into(&mut encoded)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(&encoded[..used], expected.canonical_encoding());
        assert!(CapabilitySet::decode_v2_canonical(&encoded[..used]).is_ok());
        assert_eq!(used, set.encoded_len());
        assert_mixed_refusals(&mut set, account, to, second, asset);
    }
    let mut full = ProgramPaymentCapabilities::<1>::empty();
    full.insert(grants[0]).unwrap_or_else(|e| panic!("{e}"));
    let before = full;
    assert!(full.insert(grants[1]).is_err());
    assert_eq!(full, before);
}

#[test]
fn mixed_sdk_spend_keys_sort_seed_bytes_not_the_encoded_length_prefix() {
    use layerx_program_sdk::payments::{PaymentGrant, ProgramPaymentCapabilities};
    let owner = ProgramId::new([1; 32]).unwrap_or_else(|e| panic!("{e}"));
    let sdk_owner =
        layerx_program_sdk::ProgramId::new(owner.bytes()).unwrap_or_else(|e| panic!("{e}"));
    let asset = AssetId::new([2; 32]).unwrap_or_else(|e| panic!("{e}"));
    let to = AccountId::new([3; 32]).unwrap_or_else(|e| panic!("{e}"));
    let mut set = ProgramPaymentCapabilities::<3>::empty();
    let mut expected = Vec::new();
    for seed in [b"b".as_slice(), b"aa", b""] {
        let account =
            PreparedProgramAccount::new(sdk_owner, seed, asset).unwrap_or_else(|e| panic!("{e}"));
        set.insert(PaymentGrant::ProgramSpend {
            account,
            to,
            maximum: Amount::from_u128(7),
        })
        .unwrap_or_else(|e| panic!("{e}"));
        expected.push(Capability::ProgramSpend {
            owner_program: owner,
            seed: seed.to_vec(),
            source_account: account.account().bytes(),
            asset: asset.bytes(),
            to: to.bytes(),
            maximum_amount: 7,
        });
    }
    let mut encoded = [0; 1024];
    let used = set
        .encode_into(&mut encoded)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        &encoded[..used],
        CapabilitySet::new(expected)
            .unwrap_or_else(|e| panic!("{e}"))
            .canonical_encoding()
    );
}

fn assert_mixed_refusals<'a>(
    set: &mut layerx_program_sdk::payments::ProgramPaymentCapabilities<'a, 8>,
    account: PreparedProgramAccount<'a>,
    to: AccountId,
    second: AccountId,
    asset: AssetId,
) {
    use layerx_program_sdk::{payments::PaymentGrant, Capability as SdkCapability};
    let before = *set;
    assert!(set
        .insert(PaymentGrant::ProgramSpend {
            account,
            to,
            maximum: Amount::from_u128(99)
        })
        .is_err());
    assert!(set
        .insert(PaymentGrant::Basic(SdkCapability::StorageRead))
        .is_err());
    assert!(set
        .insert(PaymentGrant::ProgramSpend {
            account,
            to: second,
            maximum: Amount::ZERO
        })
        .is_err());
    assert!(set
        .insert(PaymentGrant::Basic(SdkCapability::Transfer402 {
            asset,
            to,
            maximum_amount: Amount::ZERO
        }))
        .is_err());
    assert_eq!(*set, before);
    let mut short = vec![0xa5; set.encoded_len() - 1];
    assert!(set.encode_into(&mut short).is_err());
    assert_eq!(short, vec![0xa5; set.encoded_len() - 1]);
}

#[test]
fn mixed_sdk_grant_count_and_maximum_seed_fit_the_frozen_transport_bound() {
    use layerx_program_sdk::payments::{PaymentGrant, ProgramPaymentCapabilities};
    const LIMIT: usize = layerx_program_sdk::MAX_CAPABILITIES;
    assert_eq!(LIMIT, layerx_programs_runtime::abi::MAX_CAPABILITIES);
    let owner = layerx_program_sdk::ProgramId::new([1; 32]).unwrap_or_else(|e| panic!("{e}"));
    let asset = AssetId::new([2; 32]).unwrap_or_else(|e| panic!("{e}"));
    let seed = [7; layerx_program_sdk::MAX_PROGRAM_ACCOUNT_SEED_BYTES];
    let account =
        PreparedProgramAccount::new(owner, &seed, asset).unwrap_or_else(|e| panic!("{e}"));
    let mut set = ProgramPaymentCapabilities::<{ LIMIT + 1 }>::empty();
    for index in 1..=LIMIT {
        let mut bytes = [0; 32];
        bytes[24..].copy_from_slice(
            &u64::try_from(index)
                .unwrap_or_else(|e| panic!("{e}"))
                .to_be_bytes(),
        );
        let to = AccountId::new(bytes).unwrap_or_else(|e| panic!("{e}"));
        set.insert(PaymentGrant::ProgramSpend {
            account,
            to,
            maximum: Amount::MAX,
        })
        .unwrap_or_else(|e| panic!("{e}"));
    }
    let before = set;
    assert!(set
        .insert(PaymentGrant::Basic(
            layerx_program_sdk::Capability::StorageRead
        ))
        .is_err());
    assert_eq!(set, before);
    let mut bytes = vec![0; set.encoded_len()];
    assert!(bytes.len() <= layerx_program_sdk::MAX_CAPABILITY_ENCODING_BYTES);
    assert_eq!(set.encode_into(&mut bytes), Ok(bytes.len()));
    let decoded = CapabilitySet::decode_v2_canonical(&bytes).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(decoded.len(), LIMIT);
}
