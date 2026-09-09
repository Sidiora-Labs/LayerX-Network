use crate as layerx_programs_runtime;
use layerx_program_sdk::{
    lxt20::{Request, REFERENCE_ASSET, REFERENCE_ISSUER, REFERENCE_METADATA, REFERENCE_SUPPLY},
    AccountId, Amount, Principal,
};
use layerx_programs_runtime::{
    abi::UnavailableReceiptOracle, derive_program_account, AuthorizationContext,
    AuthorizedExecutionRequest, CandidateAuthorizedExecutionRecord, Capability, CapabilitySet,
    CompositionContext, Executor, PrincipalId, ProgramId, Storage, TransferSource, WasmEngine,
};

fn program() -> ProgramId {
    ProgramId::new([0x55; 32]).unwrap_or_else(|e| panic!("{e}"))
}
fn principal(bytes: [u8; 32]) -> Principal {
    Principal::new(bytes).unwrap_or_else(|e| panic!("{e}"))
}
fn account(owner: Principal) -> AccountId {
    AccountId::new(
        derive_program_account(program(), &owner.bytes())
            .unwrap_or_else(|e| panic!("{e}"))
            .bytes(),
    )
    .unwrap_or_else(|e| panic!("{e}"))
}
fn base_grants() -> Vec<Capability> {
    vec![
        Capability::SharedStorageRead,
        Capability::SharedStorageWrite,
    ]
}
fn spend(owner: Principal, to: AccountId, amount: u128) -> Capability {
    Capability::ProgramSpend {
        owner_program: program(),
        seed: owner.bytes().to_vec(),
        source_account: account(owner).bytes(),
        asset: REFERENCE_ASSET,
        to: to.bytes(),
        maximum_amount: amount,
    }
}
fn invoke(
    storage: &mut Storage,
    caller: Principal,
    name: &str,
    input: &[u8],
    grants: Vec<Capability>,
) -> CandidateAuthorizedExecutionRecord {
    let wasm = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/pay5/token-lxt20.wasm"
    ))
    .unwrap_or_else(|e| panic!("built reference fixture: {e}"));
    let module = WasmEngine::declared()
        .unwrap_or_else(|e| panic!("{e}"))
        .validate_v2(&wasm)
        .unwrap_or_else(|e| panic!("{e}"));
    Executor::declared()
        .for_abi(2)
        .execute_authorized_v2_with_budget(
            storage,
            AuthorizedExecutionRequest {
                module: &module,
                program: program(),
                authorization: AuthorizationContext::new(
                    PrincipalId::new(caller.bytes()).unwrap_or_else(|e| panic!("{e}")),
                    CapabilitySet::new(grants).unwrap_or_else(|e| panic!("{e}")),
                ),
                receipts: &UnavailableReceiptOracle,
                entrypoint: name,
                calldata: input,
                composition: CompositionContext::isolated(),
                response_capacity: 70,
            },
            crate::ResourceBudget::declared(),
            None,
            Some(
                crate::ExecutionContext::authenticated(1, 1, 1, 2, 1)
                    .unwrap_or_else(|e| panic!("{e:?}")),
            ),
            crate::AccessDeclaration::absent(),
        )
        .unwrap_or_else(|e| panic!("execute {name}: {e}"))
}
fn call(
    storage: &mut Storage,
    caller: Principal,
    request: Request,
    extra: Vec<Capability>,
) -> CandidateAuthorizedExecutionRecord {
    let mut grants = base_grants();
    grants.extend(extra);
    invoke(
        storage,
        caller,
        request.method(),
        request
            .encode()
            .unwrap_or_else(|e| panic!("{e}"))
            .as_slice(),
        grants,
    )
}
fn amount(record: &CandidateAuthorizedExecutionRecord) -> u128 {
    let response = &record
        .response()
        .unwrap_or_else(|| panic!("missing response: {record:?}"))
        .bytes;
    assert_eq!(&response[..6], &[1, 0x20, 0, 0, 0, 16]);
    u128::from_be_bytes(response[6..].try_into().unwrap_or_else(|e| panic!("{e}")))
}

fn initialized_storage() -> Storage {
    let issuer = principal(REFERENCE_ISSUER);
    let mut storage = Storage::new();
    assert_eq!(
        amount(&call(&mut storage, issuer, Request::TotalSupply, vec![])),
        0
    );
    let mut grants = base_grants();
    grants.push(Capability::Transfer402 {
        asset: REFERENCE_ASSET,
        to: account(issuer).bytes(),
        maximum_amount: REFERENCE_SUPPLY,
    });
    let initialized = invoke(
        &mut storage,
        issuer,
        "initialize",
        &[b'L', b'X', 20, 0, 1, 0x20, 0, 0, 0, 0],
        grants,
    );
    assert!(initialized.response().is_some());
    let effects = initialized
        .effects()
        .unwrap_or_else(|| panic!("missing initialization effects"));
    assert_eq!(effects.transfers.len(), 1);
    assert_eq!(effects.transfers[0].amount, REFERENCE_SUPPLY);
    assert!(matches!(
        effects.transfers[0].source(),
        TransferSource::ProgramFunding { .. }
    ));
    assert_eq!(
        amount(&call(&mut storage, issuer, Request::TotalSupply, vec![])),
        REFERENCE_SUPPLY
    );
    assert_eq!(
        amount(&call(
            &mut storage,
            issuer,
            Request::BalanceOf { owner: issuer },
            vec![]
        )),
        REFERENCE_SUPPLY
    );
    let metadata = call(&mut storage, issuer, Request::Metadata, vec![]);
    assert_eq!(
        &metadata
            .response()
            .unwrap_or_else(|| panic!("metadata"))
            .bytes[6..],
        REFERENCE_METADATA
    );
    storage
}

fn prepare_allowance(
    storage: &mut Storage,
    issuer: Principal,
    recipient: Principal,
    spender: Principal,
) {
    assert!(call(
        storage,
        recipient,
        Request::Approve {
            spender,
            amount: Amount::ZERO
        },
        vec![]
    )
    .response()
    .is_some());
    assert!(call(
        storage,
        issuer,
        Request::Approve {
            spender,
            amount: Amount::from_u128(100)
        },
        vec![]
    )
    .response()
    .is_some());
    assert_eq!(
        amount(&call(
            storage,
            spender,
            Request::Allowance {
                owner: issuer,
                spender
            },
            vec![]
        )),
        100
    );
}

fn transfer_back(storage: &mut Storage, issuer: Principal, recipient: Principal) {
    let transfer = Request::Transfer {
        to: account(issuer),
        amount: Amount::from_u128(15),
    };
    assert!(call(
        storage,
        recipient,
        transfer,
        vec![spend(recipient, account(issuer), 15)]
    )
    .response()
    .is_some());
    assert_eq!(
        amount(&call(
            storage,
            recipient,
            Request::BalanceOf { owner: recipient },
            vec![]
        )),
        25
    );
    assert_eq!(
        amount(&call(
            storage,
            issuer,
            Request::BalanceOf { owner: issuer },
            vec![]
        )),
        REFERENCE_SUPPLY - 25
    );
    assert_eq!(
        amount(&call(storage, issuer, Request::TotalSupply, vec![])),
        REFERENCE_SUPPLY
    );
}

fn revoke_and_refuse(
    storage: &mut Storage,
    issuer: Principal,
    recipient: Principal,
    spender: Principal,
) {
    assert!(call(
        storage,
        issuer,
        Request::Approve {
            spender,
            amount: Amount::ZERO
        },
        vec![]
    )
    .response()
    .is_some());
    let before = storage.clone();
    assert!(call(
        storage,
        spender,
        Request::TransferFrom {
            owner: issuer,
            to: account(recipient),
            amount: Amount::from_u128(1)
        },
        vec![spend(issuer, account(recipient), 1)]
    )
    .response()
    .is_none());
    assert_eq!(*storage, before);
}

#[test]
fn real_reference_executes_methods_and_rolls_back_allowance_on_refusal() {
    let issuer = principal(REFERENCE_ISSUER);
    let recipient = principal([0x22; 32]);
    let spender = principal([0x33; 32]);
    let mut storage = initialized_storage();
    prepare_allowance(&mut storage, issuer, recipient, spender);
    let transfer = Request::TransferFrom {
        owner: issuer,
        to: account(recipient),
        amount: Amount::from_u128(40),
    };
    let before = storage.clone();
    let refused = call(&mut storage, spender, transfer, vec![]);
    assert!(refused.response().is_none());
    assert!(refused.effects().is_none());
    assert_eq!(storage, before);
    let record = call(
        &mut storage,
        spender,
        transfer,
        vec![spend(issuer, account(recipient), 40)],
    );
    assert!(record.response().is_some());
    assert_eq!(
        record
            .effects()
            .unwrap_or_else(|| panic!("transfer effects"))
            .transfers
            .len(),
        1
    );
    assert_eq!(
        amount(&call(
            &mut storage,
            spender,
            Request::Allowance {
                owner: issuer,
                spender
            },
            vec![]
        )),
        60
    );
    assert_eq!(
        amount(&call(
            &mut storage,
            recipient,
            Request::BalanceOf { owner: recipient },
            vec![]
        )),
        40
    );
    transfer_back(&mut storage, issuer, recipient);
    revoke_and_refuse(&mut storage, issuer, recipient, spender);
}

#[test]
fn real_reference_refuses_malformed_methods_and_unauthorized_initialization() {
    let issuer = principal(REFERENCE_ISSUER);
    let mut storage = Storage::new();
    for method in [
        "initialize",
        "transfer",
        "approve",
        "transfer_from",
        "balance_of",
        "allowance",
        "total_supply",
        "metadata",
    ] {
        let before = storage.clone();
        let record = invoke(&mut storage, issuer, method, &[0; 10], base_grants());
        assert!(record.response().is_none(), "{method}");
        assert!(record.effects().is_none(), "{method}");
        assert_eq!(storage, before, "{method}");
    }
    let input = [b'L', b'X', 20, 0, 1, 0x20, 0, 0, 0, 0];
    for caller in [issuer, principal([0x22; 32])] {
        let before = storage.clone();
        let record = invoke(&mut storage, caller, "initialize", &input, base_grants());
        assert!(record.response().is_none());
        assert!(record.effects().is_none());
        assert_eq!(storage, before);
    }
}

#[test]
fn token_refusal_boundaries_preserve_all_storage_and_effects() {
    let issuer = principal(REFERENCE_ISSUER);
    let recipient = principal([0x22; 32]);
    let spender = principal([0x33; 32]);
    let unknown = principal([0x66; 32]);
    let mut storage = initialized_storage();
    prepare_allowance(&mut storage, issuer, recipient, spender);
    for (caller, request, grant) in [
        (
            issuer,
            Request::Transfer {
                to: account(unknown),
                amount: Amount::from_u128(1),
            },
            spend(issuer, account(unknown), 1),
        ),
        (
            recipient,
            Request::Transfer {
                to: account(issuer),
                amount: Amount::from_u128(1),
            },
            spend(recipient, account(issuer), 1),
        ),
        (
            issuer,
            Request::Transfer {
                to: account(recipient),
                amount: Amount::from_u128(100_001),
            },
            spend(issuer, account(recipient), 100_001),
        ),
        (
            spender,
            Request::TransferFrom {
                owner: issuer,
                to: account(recipient),
                amount: Amount::from_u128(101),
            },
            spend(issuer, account(recipient), 101),
        ),
        (
            unknown,
            Request::TransferFrom {
                owner: issuer,
                to: account(recipient),
                amount: Amount::from_u128(1),
            },
            spend(issuer, account(recipient), 1),
        ),
        (
            spender,
            Request::TransferFrom {
                owner: issuer,
                to: account(recipient),
                amount: Amount::from_u128(1),
            },
            spend(issuer, account(recipient), 1),
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (caller, request, mut grant))| {
        if index == 5 {
            if let Capability::ProgramSpend { asset, .. } = &mut grant {
                *asset = [0x77; 32];
            }
        }
        (caller, request, grant)
    }) {
        let before = storage.clone();
        let record = call(&mut storage, caller, request, vec![grant]);
        assert!(record.response().is_none());
        assert!(record.effects().is_none());
        assert_eq!(storage, before);
    }
    let before = storage.clone();
    let mut grants = base_grants();
    grants.push(Capability::Transfer402 {
        asset: REFERENCE_ASSET,
        to: account(issuer).bytes(),
        maximum_amount: REFERENCE_SUPPLY,
    });
    let record = invoke(
        &mut storage,
        issuer,
        "initialize",
        &[b'L', b'X', 20, 0, 1, 0x20, 0, 0, 0, 0],
        grants,
    );
    assert!(record.response().is_none());
    assert!(record.effects().is_none());
    assert_eq!(storage, before);
}
