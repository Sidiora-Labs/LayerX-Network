use layerx_programs::{
    swap::{reference_interface, Pool, PoolRefusal, BASIS_POINTS, NET_BASIS_POINTS, SHARE_CEILING},
    InterfaceCapability, ProgramInterface,
};
use layerx_programs_runtime::ProgramId;

#[test]
fn real_swap_module_binds_the_published_interface_fixture() {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pay5");
    let wasm = std::fs::read(directory.join("swap-cpmm.wasm")).unwrap_or_else(|e| panic!("{e}"));
    let program = ProgramId::new([0x55; 32]).unwrap_or_else(|e| panic!("{e}"));
    let interface = reference_interface(&wasm, program).unwrap_or_else(|e| panic!("{e}"));
    let published =
        std::fs::read(directory.join("swap-cpmm.interface")).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(interface.canonical_encoding(), published);
    assert_eq!(ProgramInterface::decode(&published), Ok(interface));
}

#[test]
fn only_the_liquidity_entries_move_share_backing_and_the_read_entries_move_nothing() {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pay5");
    let wasm = std::fs::read(directory.join("swap-cpmm.wasm")).unwrap_or_else(|e| panic!("{e}"));
    let program = ProgramId::new([0x55; 32]).unwrap_or_else(|e| panic!("{e}"));
    let interface = reference_interface(&wasm, program).unwrap_or_else(|e| panic!("{e}"));
    let names: Vec<&str> = interface
        .entries()
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "add_liquidity",
            "quote",
            "remove_liquidity",
            "reserves",
            "swap_exact_in"
        ]
    );
    for entry in interface.entries() {
        let spends = entry
            .capabilities
            .iter()
            .filter(|capability| {
                matches!(
                    capability,
                    InterfaceCapability::CallerAuthorizedSpend { .. }
                )
            })
            .count();
        let calls = entry
            .capabilities
            .iter()
            .filter(|capability| matches!(capability, InterfaceCapability::Call { .. }))
            .count();
        let expected = match entry.name.as_str() {
            "add_liquidity" | "remove_liquidity" => (1, 2),
            "swap_exact_in" => (0, 2),
            _ => (0, 0),
        };
        assert_eq!((spends, calls), expected, "{}", entry.name);
        assert!(!entry
            .capabilities
            .iter()
            .any(|capability| matches!(capability, InterfaceCapability::Transfer402 { .. })));
    }
}

#[test]
fn randomised_sequence_conserves_reserves_and_fees() {
    let mut pool = Pool::default();
    let mut random = 0x2b28_c8f5_6d5f_9e17_u64;
    let mut paid_in_a = 0_u128;
    let mut paid_in_b = 0_u128;
    let mut paid_out_a = 0_u128;
    let mut paid_out_b = 0_u128;
    let mut minted = 0_u128;
    let mut burned = 0_u128;
    let mut holdings = 0_u128;
    let mut fees_a = 0_u128;
    let mut fees_b = 0_u128;
    let mut swaps = 0_u32;
    let mut mints = 0_u32;
    let mut burns = 0_u32;

    for _ in 0..4096 {
        let choice = next(&mut random) % 4;
        match choice {
            0 | 1 => {
                let amount_in = 1 + u128::from(next(&mut random) % 4096);
                let a_to_b = next(&mut random) % 2 == 0;
                let quoted = pool.quote(a_to_b, amount_in);
                match pool.swap_exact_in(a_to_b, amount_in, 0) {
                    Ok(amount_out) => {
                        assert_eq!(Ok(amount_out), quoted);
                        let fee = amount_in - amount_in * NET_BASIS_POINTS / BASIS_POINTS;
                        if a_to_b {
                            paid_in_a += amount_in;
                            paid_out_b += amount_out;
                            fees_a += fee;
                        } else {
                            paid_in_b += amount_in;
                            paid_out_a += amount_out;
                            fees_b += fee;
                        }
                        swaps += 1;
                    }
                    Err(refusal) => {
                        assert!(matches!(
                            refusal,
                            PoolRefusal::Amount | PoolRefusal::Overflow | PoolRefusal::Slippage
                        ));
                        assert!(quoted.is_err());
                    }
                }
            }
            2 => {
                let shares = 1 + u128::from(next(&mut random) % 512);
                let maximum_a = u128::from(u32::MAX);
                let maximum_b = 1 + u128::from(next(&mut random) % 65_536);
                match pool.add_liquidity(shares, maximum_a, maximum_b) {
                    Ok((amount_a, amount_b)) => {
                        paid_in_a += amount_a;
                        paid_in_b += amount_b;
                        minted += shares;
                        holdings += shares;
                        mints += 1;
                    }
                    Err(refusal) => assert!(matches!(
                        refusal,
                        PoolRefusal::Amount
                            | PoolRefusal::Ceiling
                            | PoolRefusal::Overflow
                            | PoolRefusal::Slippage
                    )),
                }
            }
            _ => {
                let shares = 1 + u128::from(next(&mut random) % 512);
                let shares = if holdings == 0 {
                    shares
                } else {
                    shares % (holdings + 1)
                };
                match pool.remove_liquidity(shares, 0, 0) {
                    Ok((amount_a, amount_b)) => {
                        paid_out_a += amount_a;
                        paid_out_b += amount_b;
                        burned += shares;
                        holdings -= shares;
                        burns += 1;
                    }
                    Err(refusal) => assert!(matches!(
                        refusal,
                        PoolRefusal::Amount | PoolRefusal::Overflow | PoolRefusal::Slippage
                    )),
                }
            }
        }

        assert_eq!(pool.reserve_a, paid_in_a - paid_out_a);
        assert_eq!(pool.reserve_b, paid_in_b - paid_out_b);
        assert_eq!(pool.shares, minted - burned);
        assert_eq!(pool.shares, holdings);
        assert!(pool.shares <= SHARE_CEILING);
        assert!(pool.shares == 0 || (pool.reserve_a > 0 && pool.reserve_b > 0));
    }

    assert!(swaps > 0 && mints > 0 && burns > 0);
    assert!(fees_a + fees_b > 0);
    assert_eq!(pool.reserve_a + paid_out_a, paid_in_a);
    assert_eq!(pool.reserve_b + paid_out_b, paid_in_b);
}

fn next(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}
