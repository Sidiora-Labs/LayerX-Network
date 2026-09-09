# Program-funded merchant split

This ABI-v2 guest stages one principal-funded deposit followed by two program
account payouts. The kernel must commit all three 402 legs atomically. A refusal
in either payout must abort the deposit too. This source is not evidence of a
successful native settlement round trip.

Before invoking, the deployment registration authority submits the payload from
`PreparedProgramAccount::registration_payload()` for seed `payments-merchant`
and the chosen registered asset. Wait for its verified receipt and asset-bound
account state. Derivation alone does not register or fund an account.

The caller authorizes a Transfer402 grant to the derived account for `gross`,
and ProgramSpend grants from that account to the merchant for `gross - fee`
and the fee collector for `fee`. Grants must be merged into one runtime-canonical
set, with duplicate keys refused. The SDK preparation helper produces individual
spend sets; concatenating those encodings is invalid. The destinations and split
are caller-approved inputs, not an authenticated merchant pricing policy.

Calldata, all integers big-endian:

`version:u16=1 || asset:32 || merchant_account:32 || collector_account:32 || gross:u128 || fee:u128`

The guest refuses nested calls, malformed input, reserved identifiers, zero legs,
fee greater than or equal to gross, missing capabilities, and exhausted ceilings.
The host must also refuse missing registration, wrong asset bindings, insufficient
funds and invalid receipts. One seed binds one asset; deploy a separate instance
for another asset.

Build with `cargo build --manifest-path programs/sdk/rust/examples/payments-merchant/Cargo.toml --target wasm32-unknown-unknown --release`.

The current native Programs source still equates the signer principal with the
source and sequence account ID. Integration with DID-derived per-asset accounts
requires the account-owner/identity sequence change recorded in
`spec/layerx-beta/qualification.kvx`. Do not substitute a DID for a derived
account ID.
