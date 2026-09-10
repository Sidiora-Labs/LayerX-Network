# LXT-20 settlement reference

This ABI-v2 reference holds a fixed registered backing asset in program-derived
accounts, one per owner DID id32 (the seed is exactly that id32). Transfers move
both the shared token balances and real 402 backing units between these accounts.
A recipient must call `approve` at least once, including an approval of zero, to
register its derived account in token storage. The deployment authority must
separately register that account with the native Programs account module before
it can receive backing units. Arbitrary external recipient accounts are refused.

The example configuration lives in `programs/sdk/rust/src/lxt20.rs`: the issuer is
`did:lxp:token-issuer`, the backing asset is `REFERENCE_ASSET`, the fixed supply is
`REFERENCE_SUPPLY`, and each payment is bounded by `REFERENCE_CEILING`. Configure
these source constants for another deployment and regenerate its interface.
There is no mint, burn, permit, fee-on-transfer, rebasing or unbounded approval
exception. Each successful `transfer_from` subtracts the exact amount, including
when the allowance was set to u128::MAX. An approval of zero revokes it.

The issuer initializes once with entry `initialize` and calldata
`4c581400012000000000`. Initialization stages a funding transfer of the complete
supply from the issuer into its registered program account and writes supply
and issuer balance in the same native transaction. The caller must authorize
that exact Transfer402 funding grant. No supply is created by queries or approvals.

The seven LXT-20 methods use `lxt20::Request` canonical calldata. Their response
schema is LayerX bounded bytes: `[1, 0x20] || length:u32 || payload`. Balances,
allowances and total supply carry a 16-byte big-endian u128 payload; mutations
return an empty payload; metadata carries `REFERENCE_METADATA`. The guest refuses
nested calls. `transfer` and `transfer_from` require the caller's exact ProgramSpend
grant for the owner program, owner seed, source account, backing asset, recipient
and sufficient ceiling, in addition to the token's balance and allowance checks.
The native transaction must commit or roll back the backing transfer and token
storage together. Runtime-only execution tests inspect staged transfer effects;
they do not establish native settlement.

Build:

```
cargo build --manifest-path programs/sdk/rust/examples/token-lxt20/Cargo.toml --target wasm32-unknown-unknown --release
```

`layerx-programs-registry::lxt20::reference_interface` binds the real module to all
eight exports. The transfer entries declare caller-authorized dynamic-spend
descriptors, not destination grants. The registry example `lxt20_interface`
writes the canonical interface and the registry state value for a supplied
program id and prints its digest. Include the interface in the native deployment
payload; generating a state value alone does not publish a deployment receipt.
The committed fixtures use program id `55` repeated 32 times.
