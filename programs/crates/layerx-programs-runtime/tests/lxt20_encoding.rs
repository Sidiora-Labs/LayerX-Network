use layerx_program_sdk::{lxt20::Request, AccountId, Amount, Principal};
use layerx_programs_runtime::abi::Calldata;

#[test]
fn every_method_matches_runtime_canonical_calldata_and_refuses_mutations() {
    let owner = Principal::new([1; 32]).unwrap_or_else(|e| panic!("{e}"));
    let spender = Principal::new([2; 32]).unwrap_or_else(|e| panic!("{e}"));
    let to = AccountId::new([3; 32]).unwrap_or_else(|e| panic!("{e}"));
    let amount = Amount::from_u128(u128::MAX);
    let requests = [
        Request::Transfer { to, amount },
        Request::Approve {
            spender,
            amount: Amount::ZERO,
        },
        Request::TransferFrom { owner, to, amount },
        Request::BalanceOf { owner },
        Request::Allowance { owner, spender },
        Request::TotalSupply,
        Request::Metadata,
    ];
    for (request, fixture) in requests
        .into_iter()
        .zip(include_str!("../../../sdk/rust/vectors/lxt20-requests.txt").lines())
    {
        let encoded = request.encode().unwrap_or_else(|e| panic!("{e}"));
        let hex = hex::encode(encoded.as_slice());
        assert_eq!(hex, fixture);
        assert_eq!(Request::decode(encoded.as_slice()), Ok(request));
        assert!(Calldata::from_bytes(&encoded.as_slice()[4..]).is_ok());
        for length in 0..encoded.len() {
            assert!(Request::decode(&encoded.as_slice()[..length]).is_err());
        }
        let mut trailing = encoded.as_slice().to_vec();
        trailing.push(0);
        assert!(Request::decode(&trailing).is_err());
        for offset in [0, 1, 2, 3, 4, 5, 6, 7, 8, 9] {
            let mut wrong = encoded.as_slice().to_vec();
            wrong[offset] = 0xff;
            assert!(Request::decode(&wrong).is_err());
        }
    }
    assert!(Request::Transfer {
        to,
        amount: Amount::ZERO
    }
    .encode()
    .is_err());
    assert!(Request::TransferFrom {
        owner,
        to,
        amount: Amount::ZERO
    }
    .encode()
    .is_err());
}
