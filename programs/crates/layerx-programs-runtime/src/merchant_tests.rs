use super::lxt20_tests::{invoke_guest, program};
use crate::{derive_program_account, Capability, Storage, TransferSource};
use layerx_program_sdk::{lxt20::REFERENCE_ISSUER, Principal};

fn calldata(gross: u128, fee: u128) -> Vec<u8> {
    let mut input = vec![0, 1];
    input.extend([0x44; 32]);
    input.extend([0x22; 32]);
    input.extend([0x33; 32]);
    input.extend(gross.to_be_bytes());
    input.extend(fee.to_be_bytes());
    input
}

fn grants() -> Vec<Capability> {
    let source = derive_program_account(program(), b"payments-merchant")
        .unwrap_or_else(|e| panic!("{e}"))
        .bytes();
    vec![
        Capability::Transfer402 {
            asset: [0x44; 32],
            to: source,
            maximum_amount: 100,
        },
        Capability::ProgramSpend {
            owner_program: program(),
            seed: b"payments-merchant".to_vec(),
            source_account: source,
            asset: [0x44; 32],
            to: [0x22; 32],
            maximum_amount: 75,
        },
        Capability::ProgramSpend {
            owner_program: program(),
            seed: b"payments-merchant".to_vec(),
            source_account: source,
            asset: [0x44; 32],
            to: [0x33; 32],
            maximum_amount: 25,
        },
    ]
}

fn execute(input: &[u8], grants: Vec<Capability>) -> crate::CandidateAuthorizedExecutionRecord {
    let wasm = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/pay5/payments-merchant.wasm"
    ))
    .unwrap_or_else(|e| panic!("merchant fixture: {e}"));
    let caller = Principal::new(REFERENCE_ISSUER).unwrap_or_else(|e| panic!("{e}"));
    let mut storage = Storage::new();
    let before = storage.clone();
    let record = invoke_guest(&wasm, &mut storage, caller, "layerx_call", input, grants);
    assert_eq!(storage, before);
    record
}

#[test]
fn merchant_stages_one_funding_leg_and_two_exact_owner_payouts() {
    let record = execute(&calldata(100, 25), grants());
    assert!(record.response().is_some());
    let transfers = &record
        .effects()
        .unwrap_or_else(|| panic!("merchant effects"))
        .transfers;
    assert_eq!(transfers.len(), 3);
    assert_eq!(
        transfers.iter().map(|leg| leg.amount).collect::<Vec<_>>(),
        [100, 75, 25]
    );
    assert!(matches!(
        transfers[0].source(),
        TransferSource::ProgramFunding { .. }
    ));
    let source = derive_program_account(program(), b"payments-merchant")
        .unwrap_or_else(|e| panic!("{e}"))
        .bytes();
    assert_eq!(transfers[0].to, source);
    for (leg, to) in transfers[1..].iter().zip([[0x22; 32], [0x33; 32]]) {
        let TransferSource::Program(authority) = leg.source() else {
            panic!("program source required");
        };
        assert_eq!(authority.owner_program(), program());
        assert_eq!(authority.source_account(), source);
        assert_eq!(authority.seed(), b"payments-merchant");
        assert_eq!(leg.to, to);
        assert_eq!(leg.asset, [0x44; 32]);
    }
}

#[test]
fn merchant_discards_all_staged_legs_when_either_payout_is_refused() {
    for missing in 0..3 {
        let mut authority = grants();
        authority.remove(missing);
        let record = execute(&calldata(100, 25), authority);
        assert!(record.response().is_none());
        assert!(record.effects().is_none());
    }
    for limited in 0..3 {
        let mut authority = grants();
        match &mut authority[limited] {
            Capability::Transfer402 { maximum_amount, .. }
            | Capability::ProgramSpend { maximum_amount, .. } => *maximum_amount -= 1,
            _ => panic!("payment grant required"),
        }
        let record = execute(&calldata(100, 25), authority);
        assert!(record.response().is_none());
        assert!(record.effects().is_none());
    }
}

#[test]
fn merchant_refuses_zero_legs_overdrawn_split_and_malformed_input() {
    for input in [
        calldata(0, 0),
        calldata(100, 0),
        calldata(100, 100),
        calldata(100, 101),
        vec![],
        vec![0; 130],
        calldata(100, 25)[..129].to_vec(),
    ] {
        let record = execute(&input, grants());
        assert!(record.response().is_none());
        assert!(record.effects().is_none());
    }
}
