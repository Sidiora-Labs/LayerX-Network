# Payments developer path

This path covers a wallet, faucet funding, an Asset transfer, token issuance,
an LXT20 program, and an HTTP 402 payment. The wallet/token CLI, native Asset
execution, public RPC, payment signer, and extended 402LXP flow described here
are served by this tree. The LXT20 example is not: neither
`programs/sdk/rust/examples` nor `programs/fixtures/pay5` is present here.

For a shorter environment checklist, start with
[Getting started on testnet](Getting-Started-Testnet.md). Wire details are in
[Assets](Assets.md), [Public JSON-RPC](PublicRpc.md), and
[Commitment levels](CommitmentLevels.md).

## 1. Configure trusted testnet inputs

The published gateway and faucet origins are:

```sh
export RPC_URL=https://api.testnet.layerx.network/rpc
export FAUCET_URL=https://faucet.testnet.layerx.network
```

You also need:

- a bearer session token for the faucet;
- a stored `LayerX-Key` gateway credential with the scopes required by the
  operations you will submit;
- the network id and a receipt policy obtained independently of the RPC
  response being verified;
- a fee limit in integer base units.

The receipt-policy JSON contains `protocol_version` (`3`), `network_id`,
64-hex `sequencer_id` and `sequencer_key`, inclusive `first_batch` and
`last_batch`, and `checkpoint_context_digest`. Use `null` for the checkpoint
digest when only execution or batch verification is required.

For the disposable local cluster, run the existing cluster quickstart and
source `build/beta-cluster/env`. Its exported `LAYERX_TEST_CA_FILE` is
`build/beta-cluster/ca/ca.crt`; use that CA only for the cluster endpoints it
was generated for. The public HTTPS hosts use their deployed certificate
chain.

## 2. Create or import a signing key

Build the testnet-branch CLI and put `platform/target/debug` on `PATH`. For a
public testnet identity, create an ordinary key and select it:

```sh
cargo build --manifest-path platform/cli/Cargo.toml
export PATH="$PWD/platform/target/debug:$PATH"
layerx key create alice
layerx key default alice
layerx wallet list
```

The OS keyring is the default. A headless host must configure the encrypted
file store before creating the wallet; see
[headless credential storage](../../platform/cli/README.md#headless-credential-storage).
`layerx wallet create` is emulator-only: it generates a key, registers the DID,
and opens the native main account in that emulator. Against a public endpoint
it returns `wallet_registration_unavailable` and generates no key. `wallet
import` reads one 32-byte hexadecimal seed from standard input and never
registers or funds the identity.

Record the wallet's public DID and Ed25519 public key without exposing its
seed:

```sh
export WALLET_DID='<did from layerx wallet list>'
export WALLET_PUBLIC_KEY='<64-hex public key>'
```

Public wallet registration and public DID history are not exposed by the CLI;
those attempts return typed unavailable errors.

## 3. Request test funds

The faucet accepts exactly `did` and `public_key`. It requires a bearer session,
`Content-Type: application/json`, and a unique `Idempotency-Key` of 1–128
letters, digits, `-`, `_`, `.`, or `:`. Preserve the key for retries.

```sh
jq -n --arg did "$WALLET_DID" --arg public_key "$WALLET_PUBLIC_KEY" \
  '{did:$did, public_key:$public_key}' > faucet-request.json

curl --fail-with-body --silent --show-error \
  --request POST "$FAUCET_URL/v1/faucet/claims" \
  --header 'Content-Type: application/json' \
  --header "Authorization: Bearer $FAUCET_SESSION_TOKEN" \
  --header "Idempotency-Key: $FAUCET_REQUEST_ID" \
  --data-binary @faucet-request.json > faucet-response.json

jq -e '.funded == true and (.funding_id | type == "string")' faucet-response.json
```

For a disposable cluster, add `--cacert "$LAYERX_TEST_CA_FILE"`. A successful
body contains `funded: true`, `funding_id`, optional `transaction_id`, decimal
string `amount`, and `network: "layerx-testnet"`. HTTP 202
`still_checking`, any refusal, or an unobserved response is not funding
confirmation. Retry only with the same idempotency key and body, then confirm
the account through `lx_getAccount`, `lx_getBalance`, or `lx_getBalances`.

## 4. Create and use an Asset

Set your locally trusted receipt policy and fee ceiling:

```sh
export RECEIPT_POLICY=/absolute/path/to/receipt-policy.json
export FEE_LIMIT='<maximum fee in base units>'
```

Create a native Asset:

```sh
layerx --rpc "$RPC_URL" --gateway-credential testnet token create \
  --symbol PAY --name 'Payment Token' --decimals 6 \
  --supply-cap 1000000000 --salt "$(openssl rand -hex 32)" \
  --receipt-policy "$RECEIPT_POLICY" --fee-limit "$FEE_LIMIT" \
  --wait executed
```

The disclosure prints the derived Asset id. The id is
`SHA-256("LX:ASSET:v1" || issuer_did_id32 || salt32)`. Store it, then open the
recipient's per-Asset account before minting or transferring:

```sh
export ASSET_ID='<64-hex Asset id>'
export RECIPIENT_DID='<recipient DID>'

layerx --rpc "$RPC_URL" --gateway-credential testnet wallet open-account \
  --asset "$ASSET_ID" --receipt-policy "$RECEIPT_POLICY" \
  --fee-limit "$FEE_LIMIT"

layerx --rpc "$RPC_URL" --gateway-credential testnet token mint \
  --asset "$ASSET_ID" --to "$RECIPIENT_DID" --amount 100 \
  --receipt-policy "$RECEIPT_POLICY" --fee-limit "$FEE_LIMIT"
```

`wallet open-account` acts for the selected local wallet; run it with the
recipient wallet selected when preparing the recipient account. Other writes
use the same path:

```sh
layerx --rpc "$RPC_URL" --gateway-credential testnet wallet send \
  --to "$RECIPIENT_DID" --asset "$ASSET_ID" --amount 10 \
  --wait executed --receipt-policy "$RECEIPT_POLICY" \
  --fee-limit "$FEE_LIMIT"

layerx --rpc "$RPC_URL" --gateway-credential testnet token transfer \
  --to "$RECIPIENT_DID" --asset "$ASSET_ID" --amount 10 \
  --wait finalised --receipt-policy "$RECEIPT_POLICY" \
  --fee-limit "$FEE_LIMIT"
```

The CLI obtains the identity sequence with
`lx_getSequence([did, "identity"])` and the source-account sequence
independently. For a debit it discloses and signs the native debit
authorization before disclosing and signing the outer activity. The signing
request is bound to the canonical bytes and disclosure; a changed disclosure,
stale sequence, invalid signature, missing scope, failed native receipt, or
missing commitment evidence exits nonzero.

The signer accepts a typed `SigningRequest`, not arbitrary approval text. The
activity disclosure names the activity type, actor, authority, payer and
recipient roles, transfer or spending-limit amounts, Asset, fee limit,
not-before/expiry bounds, idempotency key, optional EVM binding, and decoded
payment. A SEND debit disclosure separately binds `from`, `to`, Asset, amount,
source sequence, idempotency key, expiry, context, conditions, authorization
kind, network, and protocol. Both disclosures are re-encoded and checked
against the signature-message digest before a signature is released.

Every invocation creates a new idempotency key. If a write times out or remains
pending, retain its activity id and recover it rather than repeating the write:

```sh
layerx --rpc "$RPC_URL" --gateway-credential testnet wallet receipt \
  "$ACTIVITY_ID" --receipt-policy "$RECEIPT_POLICY" --wait finalised
```

`--timeout-seconds` is 1–300 and defaults to 60.

## 5. Inspect Assets, balances, and fees

```sh
layerx --rpc "$RPC_URL" --gateway-credential testnet wallet balance \
  --did "$WALLET_DID"
layerx --rpc "$RPC_URL" --gateway-credential testnet token info "$ASSET_ID"
layerx --rpc "$RPC_URL" --gateway-credential testnet token list
layerx --rpc "$RPC_URL" --gateway-credential testnet wallet estimate-fee \
  "$CANONICAL_HEX"
```

The corresponding RPC methods are `lx_getBalances`, `lx_getAsset`,
`lx_listAssets`, and `lx_estimateFee`. Snapshot-authentication labels are not
finality claims, and a fee estimate neither reserves the fee nor proves that
the activity will execute.

Live `receipts`, `checkpoints`, and `account` watches are unverified wake-ups.
Always reconcile with a verified read or `wallet receipt` after a notification
or stream closure.

## 6. Build and deploy the LXT20 example

The LXT20 example is a Programs ABI-v2 token backed by one native Asset. It is
not a second ledger and does not mint native units. It is not in this tree, so
the build below has no manifest to read here.

```sh
cargo build \
  --manifest-path programs/sdk/rust/examples/payments-merchant/Cargo.toml \
  --target wasm32-unknown-unknown --release
```

Before funding, derive and register the program account for the selected
Asset using `PreparedProgramAccount::registration_payload`. Include the
generated ABI-v2 interface in the native Programs deploy activity. A registry
record or state upload alone is not a deployment receipt.

The reference interface contains `initialize`, `transfer`, `approve`,
`transfer_from`, `balance_of`, `allowance`, `total_supply`, and `metadata`.
Recipients call `approve`, including approval of zero, once to register their
derived program-account storage before receiving. Initialization stages the
fixed supply from the issuer to the registered program account atomically.
There is no LXT20 mint, burn, permit, or nested-call surface.

For the exact account, grants, calldata, and receipt-read contract, see
[Programs](Programs.md).

## 7. Pay an HTTP 402 endpoint

LayerX uses x402 v2 headers:

1. The seller returns HTTP 402 with `PAYMENT-REQUIRED`.
2. The buyer selects one offered requirement without changing it.
3. The buyer returns `PAYMENT-SIGNATURE`.
4. The seller releases the resource only after verifying the payment and
   returns `PAYMENT-RESPONSE`.

For an `exact` offer, the buyer submits its canonical activity with
`lx_sendActivity`, verifies the requested commitment, and puts the canonical
receipt, receipt digest, and `sequencer-signed` verification label in the
payment payload.

For `metered` or `subscription`, the challenge also binds
`extra.layerx.payer`, `purposeHash`, and `commitment`; a subscription includes
`windowSeconds`. The payer first signs and submits the 346-byte ordinal-7
grant. The receiver prepares and signs a 733-byte ordinal-6 receive for each
draw. The payment payload carries that exact receive and its idempotency key.
The seller validates payer, recipient, Asset, amount, purpose, grant limits,
expiry, network, and receiver authorization before submitting the draw.

A metered grant is non-recurring with zero window length. A subscription grant
is recurring and its window length must equal `windowSeconds`. Every renewal is
a newly signed receive with current sequences and a distinct period
idempotency key. The allowance alone does not prove one renewal per period.

Pending draw state returns HTTP 202 without releasing the resource. The
persistent draw store recovers the registered activity by id and never creates
a replacement debit. Successful settlement is
`lxp:<SHA-256("LXP/v1/merkle-leaf\\0" || canonical_receipt)>` and is accepted
only after the requested `executed`, `batched`, or `finalised` evidence and all
payment facts verify.

See [x402 transport](X402Transport.md) for the header schemas and refusal
behavior.

## CLI and MCP boundaries

The payment MCP catalogue has 20 tools
(`agent/crates/layerx-mcp/src/server.rs:51`). Its payment writes are
`wallet.send`, `token.create`, `token.mint`, `token.transfer`, `grant.issue`,
and `grant.draw`; they use the daemon's ordinary prepare, disclose, sign,
submit, and track stages. Burn, account-open, Asset info/list, fee estimate,
receipt wait, and live watch are not in the catalogue and are not CLI commands
in this tree either (`platform/cli/src/main.rs:44-84`).

[Home](Home.md)
