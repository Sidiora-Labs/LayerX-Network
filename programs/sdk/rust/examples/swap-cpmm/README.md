# Constant-product swap reference

This ABI-v2 reference pools two LXT-20 token programs and issues pool shares as
an LXT-20 token embedded in the swap program itself. The two pooled tokens move
only through `program_call` into their own LXT-20 programs, using the canonical
`lxt20::Request` calldata; the embedded share token moves only through
`transfer_program_402` between accounts derived for the swap program. The swap
program never calls `transfer_402` or `fund_program_402`, so it holds no
authority over any account other than the ones derived from its own program id.

The pooled tokens are `TOKEN_A` and `TOKEN_B`, each an LXT-20 program with its
own registered backing asset. The pool's own balance in each token lives in the
account that token program derives from the swap program's id32. A provider's
balance lives in the account that token program derives from the provider's
id32, exactly as in the LXT-20 settlement reference. Share balances live in the
swap program's shared storage and are backed one-for-one by units of
`SHARE_ASSET` held in accounts the swap program derives, the treasury account
under the seed `lx.ref.swap.treasury` and each holder's account under its id32.

The five entries use the discriminator `LXS` followed by the entry ordinal and
the canonical `LayerX` bounded-bytes envelope `[1, 0x20] || length:u32 ||
payload`:

- `add_liquidity` mints an exact share count. The caller supplies the share
  recipient, the share count and the maximum it will pay in each token; the
  program charges `ceil(shares * reserve / supply)` of each token and refuses
  when either charge exceeds the supplied maximum. The first deposit sets the
  initial price: it charges the share count in token A and the supplied maximum
  in token B.
- `remove_liquidity` burns an exact share count, returning
  `floor(shares * reserve / supply)` of each token and refusing when either
  payout is below the supplied minimum. The caller supplies the treasury
  account, which the program rederives before the share units move back.
- `swap_exact_in` charges the full input, keeps `FEE_BASIS_POINTS` of it in the
  pool and pays out
  `floor(reserve_out * net_in / (reserve_in + net_in))`, refusing below the
  caller's `min_out`. Both products are evaluated at 256-bit width through the
  `bigint_mul_256`, `bigint_div_256` and `bigint_rem_256` host functions, so
  `x * y = k` never wraps.
- `quote` runs the same output computation without moving value.
- `reserves` returns `reserve_a || reserve_b || total_shares`.

Every fee stays in the reserves, so the reserve of each token is exactly the
sum of everything ever paid in minus everything ever paid out.

Build:

```
cargo build --manifest-path programs/sdk/rust/examples/swap-cpmm/Cargo.toml --target wasm32-unknown-unknown --release
```

`layerx-programs-registry::swap::reference_interface` binds the real module to
all five exports and their exact capability masks. The registry example
`swap_interface` writes the canonical interface and the registry state value for
a supplied program id and prints its digest. The committed fixtures use program
id `55` repeated 32 times.
