# Payments developer path

This page covers local Ed25519 keys, faucet claims, payment submission,
native tokens, program deployment, and HTTP 402 payments, including
source limitations that prevent an end-to-end wallet write. Commands and HTTP routes that already exist on `main` are
cited here. Surfaces that exist only on unmerged payment lanes are marked
**on the testnet branch** and are not live on `main`.

Related pages: [CLI](Cli.md), [Assets](Assets.md),
[Public JSON-RPC](PublicRpc.md), [Commitment levels](CommitmentLevels.md),
[Hosted faucet](HostedFaucet.md), [Programs](Programs.md),
[x402 transport](X402Transport.md), [Testnet cluster quickstart](Quickstart.md).

Wallet and token commands are **on the testnet branch** `lane/pay-wallet-cli`
(draft PR #208; `platform/cli/src/wallet.rs`). It provides `wallet create`,
`import`, `list`, `balance`, `history`, `receipt`, `send`, `open-account`,
and `token create`, `mint`, `burn`, `transfer`, `info`, `list`.
Local emulator creation registers the DID and opens its main account.
Public registration and DID history remain unavailable. Token writes use
the SDK prepare/disclose/execute path and require authenticated identity
sequence reads plus native operation support. Send and token transfer
refuse before signing because debit-authorization signing is unavailable.
Token metadata reads forward to RPC and preserve upstream unavailability. The commands' presence does not establish a live payment.
The `main` key-only command below is `layerx key create`.

---

## 1. Create a wallet

For hosts without an OS Secret Service, follow
[headless credential storage](../../platform/cli/README.md#headless-credential-storage)
first.

```sh
layerx key create quickstart
```

`name` is 1–128 ASCII alnum/`-`/`_` (`platform/cli/src/credential.rs`).
Without `--did` the DID is `did:layerx:` plus the 64-hex Ed25519 public key.
Human output is `Created key {name} in credential storage` with
`{name, did, public_key}`. The seed is 32 OS-random bytes in the selected
credential backend.

Store a hosted session token (do not print the file contents) and select
the testnet profile as in [Testnet cluster quickstart](Quickstart.md#3-create-a-credential).

The native-asset account name for that DID is `agent:<DID>:main`. Per-asset
account names and ids are on [Assets](Assets.md).

---

## 2. Get test funds from the faucet

There is no `layerx faucet` command. Claim at `POST /v1/faucet/claims`
(`platform/hosted/faucet/src/main.rs`).

```sh
jq -n --arg did "$LAYERX_TEST_SOURCE_DID" --arg public_key "$LAYERX_TEST_SOURCE_PUBLIC_KEY" \
  '{did:$did, public_key:$public_key}' > faucet-request.json
curl --fail --silent --show-error --max-time 30 --cacert "$LAYERX_TEST_CA_FILE" \
  --header "Authorization: Bearer $(tr -d '\r\n' < "$LAYERX_TEST_AUTH_TOKEN_FILE")" \
  --request POST "$LAYERX_FAUCET_URL/v1/faucet/claims" \
  --header "Idempotency-Key: faucet-quickstart-01" \
  --header 'Content-Type: application/json' --data-binary @faucet-request.json
```

A 200 body has `funded` `true`, `funding_id`, `amount` as a decimal string
of `LAYERX_FAUCET_CLAIM_AMOUNT` (default `1000000`), and `network`
`layerx-testnet`. A 202 body is `still_checking` and is not a funded
balance. Confirm the claim before treating funds as spendable.
Details: [Hosted faucet](HostedFaucet.md).

---

## 3. Send

On `main`, a signed Asset SEND reaches the hosted gateway as
`POST /v1/activities` with `Authorization: LayerX-Key` and scope
`activity:write` (`platform/hosted/gateway/src/lib.rs`;
`platform/cli/src/toolset.rs`). MCP/A2A `activity.submit` posts
`{"activity": <hex>}` on that route.

Issue a gateway key with `layerx install mcp` or `layerx install a2a`
(hosted environment only). Payment-capable install requires
`--source-account` and `--asset` as 64-hex
(`platform/cli/src/install/mcp.rs`).

`layerx payment test` quotes `POST /v1/moves/quote` then commits
`POST /v1/moves` (`platform/cli/src/payment.rs`). Hosted
`production_route` does not accept those paths
(`platform/hosted/gateway/src/lib.rs`). Use `POST /v1/activities` for a
signed activity on `main`.

**On the testnet branch** `lane/pay-public-rpc`, submit the same canonical
hex through JSON-RPC:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "lx_sendActivity",
  "params": ["<canonical_hex>", "executed"]
}
```

`POST /rpc` on the gateway. `commitment` is `executed`, `batched`, or
`finalised`. An admission acknowledgement is never success. See
[Public JSON-RPC](PublicRpc.md) and [Commitment levels](CommitmentLevels.md).

**On the testnet branch** `lane/pay-signer-sdk`, MCP tools `wallet.send`
and `token.transfer` encode a payment and follow the daemon
prepare → disclose → sign → submit → track path
(`agent/crates/layerx-mcp/src/tools/write.rs`). Those tool names are not
on `main` (`agent/crates/layerx-mcp/README.md`).

Fetch and verify the receipt as in [Testnet cluster quickstart](Quickstart.md#7-fetch-the-receipt).

---

## 4. Create a token

Native token issuance is **on the testnet branch**. `main` decodes Asset
ordinals 1–8 and executes SEND; it does not define mint/burn activity
types (`include/layerx/lx_asset.h`).

The shared register/open/mint wire is:

1. **Register** (Asset ordinal 1). Native `asset_id32` is
   `SHA-256("LX:ASSET:v1" || issuer_did_id32 || salt32)`. Payload:
   `version:u16=1 || asset_id32 || salt32 || symbol_len:u8 || symbol || name_len:u8 || name || decimals:u8 || supply_cap:u128 || issuer_kind:u8 || custody_ref_len:u8 || custody_ref`.
2. **Open** the issuer's per-asset account (ordinal 4):
   `version:u16=1 || asset_id32`. Name:
   `agent:<DID>:asset:<lowercase hex64 asset_id>`.
3. **Mint** (ordinal 10):
   `version:u16=1 || asset_id32 || to_account32 || amount:u128`.
   Actor must be the issuer; destination must exist for that asset.

On `lane/pay-signer-sdk`, MCP `token.create` and `token.mint` submit those
activities through the daemon. `layerx token` is on the testnet branch
`lane/pay-wallet-cli`; token writes require the API support described above.

On `lane/pay-native`, register / account_open / mint / burn decode to the
payloads above and execute through `asset_execute_typed`
(`src/modules/asset/lx_asset_execution.h`). These source paths do not
establish deployment or public wallet integration.

Full field bounds: [Assets](Assets.md).

---

## 5. Deploy a program

On `main`:

```sh
layerx new quickstart-program
layerx --json program build --manifest-path quickstart-program/Cargo.toml
layerx --json program deploy \
  quickstart-program/target/wasm32-unknown-unknown/release/quickstart_program.wasm \
  --program-id <program_id> \
  --idempotency-key <idempotency_key> \
  --key quickstart \
  --account-sequence 0 \
  --not-before-ms <not_before_ms> \
  --expires-at-ms <expires_at_ms> \
  --previous-state-root <previous_state_root>
```

The command POSTs canonical signed bytes to `POST /v1/programs/deploy`
(`platform/cli/src/programs.rs`). Hosted deploy authenticates `LayerX-Key`
with scope `program:call`. Lifecycle flags and receipt shape:
[Testnet cluster quickstart](Quickstart.md#5-deploy-a-program) and
[Programs](Programs.md).

**On the testnet branch** `lane/pay-programs-tokens`, the Rust guest SDK
adds `lxt20` request codecs and `payments` program-account preparation
(`programs/sdk/rust/src/lxt20.rs`, `programs/sdk/rust/src/payments.rs`).
The merchant example builds with:

```sh
cargo build --manifest-path programs/sdk/rust/examples/payments-merchant/Cargo.toml \
  --target wasm32-unknown-unknown --release
```

Register the program account for seed `payments-merchant` and the chosen
asset before funding (`PreparedProgramAccount::registration_payload`).
Those files are not on `main`.

---

## 6. Pay a 402 endpoint

On `main`, x402 v2 uses headers `PAYMENT-REQUIRED`, `PAYMENT-SIGNATURE`,
and `PAYMENT-RESPONSE` (`interop/crates/layerx-x402/src/model.rs`). A
seller issues HTTP 402; a buyer returns a payload; settlement success
requires a gateway-verified canonical LayerX receipt. See
[x402 transport](X402Transport.md).

**On the testnet branch** `lane/pay-402lxp`, an offer may carry
`extra.layerx.commitment` ∈ `{executed, batched, finalised}`
(`spec/402lxp/protocol.md` on that branch). Missing `extra.layerx` on an
exact offer defaults to `executed`. Metered and subscription schemes
carry a canonical Asset receive (ordinal 6) in the payment payload; a
grant alone never releases the resource.

Buyer middleware on that branch parses `PAYMENT-REQUIRED`, construct the
payment, attaches `PAYMENT-SIGNATURE`, and accepts `PAYMENT-RESPONSE` only
after the requested commitment verifies
(`platform/middleware/seller/src/commitment.ts`).
`transaction` remains `lxp:<receipt_digest>`. HTTP 202 pending is not
proof of payment.

---

## What is on `main` vs the testnet branches

| Step | On `main` | On the testnet branch |
| --- | --- | --- |
| Create a key / DID | `layerx key create` | plus local `wallet create` on `lane/pay-wallet-cli` |
| Faucet claim | `POST /v1/faucet/claims` | same |
| Send a signed activity | `POST /v1/activities` | plus `lx_sendActivity` on `lane/pay-public-rpc` |
| Register / mint a token | not executable | execution on `lane/pay-native`; MCP tools on `lane/pay-signer-sdk`; wallet token writes require identity reads; send/transfer refuse signing |
| Deploy a program | `layerx program deploy` | plus LXT-20 / merchant example on `lane/pay-programs-tokens` |
| Pay HTTP 402 | `layerx-x402` receipt binding | plus commitment extras and receive codecs on `lane/pay-402lxp` |
| Public JSON-RPC | not present | `POST /rpc`, `GET /rpc/schema`, `GET /rpc/ws` on `lane/pay-public-rpc` |

[Home](Home.md)
