# LayerX developer CLI

Binary `layerx` (`platform/cli`). The complete payment path is
[`docs/wiki/PaymentsQuickstart.md`](../../docs/wiki/PaymentsQuickstart.md).

The command groups in this tree are `wallet`, `token`, `new`, `workspace`,
`environment`, `key`, `auth`, `account`, `payment`, `receipt`, `program`,
`emulator`, `install`, `mcp`, and `a2a` (`platform/cli/src/main.rs:53-97`).
The `wallet` and `token` groups carry the native key, account, transfer, token
and receipt operations documented below; `account` and `payment` remain the
hosted developer-account and hosted-move surface.

`--json`, `--rpc` and `--gateway-credential` are the global flags
(`platform/cli/src/main.rs:34-47`); `--gateway-credential` is additionally a
required per-command argument on `mcp serve` and `a2a serve`
(`platform/cli/src/main.rs:463-464, 482-483`). The signing arguments take
`--fee-limit`, defaulting to `0` (`platform/cli/src/main.rs:309-310,
1156-1157`).

`payment quote` and `payment commit` read the account sequence, sign the
canonical activity, and record the activity id so an uncertain outcome can be
recovered rather than retried blindly (`platform/cli/src/payment.rs:5-37`).

## Wallet quickstart

Build with `cargo build --manifest-path platform/cli/Cargo.toml`. The executable
is `platform/target/debug/layerx`; put that directory on your PATH.

Wallet creation on the emulator, imports, listing, balance reads and verified
RPC receipt waits are available. Send, token transfer, token creation, mint,
burn and asset-account opening use shared disclosure and signing with separate
identity and source-account sequences. Public writes require `--rpc`, a trusted
receipt policy and a fee limit. Funded public execution still requires a
configured authenticated deployment; local fixture tests do not establish it.
Public wallet registration and DID history are not published and return typed
unavailable errors without signing or inventing history.

### Create a local wallet

Use the OS keyring or configure the encrypted file store below. Create a private
profile and provision an emulator:

```bash
umask 077
export LAYERX_CONFIG="$HOME/.config/layerx/config.json"
mkdir -p "$(dirname "$LAYERX_CONFIG")"
layerx emulator provision
layerx emulator up --sequencer-seed-file "$HOME/.config/layerx/emulator/sequencer.seed"
```

In another terminal with the same configuration and credential-store settings:

```bash
layerx environment use emulator --endpoint http://127.0.0.1:9402 \
  --network-id 402 \
  --sequencer-trust-anchor-file "$HOME/.config/layerx/emulator/sequencer.anchor"
layerx wallet create alice
layerx wallet list
layerx wallet balance
```

Creation registers the local DID and opens its main account with zero units.
If registration fails after key creation, the key is retained; retry using the
same wallet name. Import an existing 32-byte hexadecimal seed with
`layerx wallet import alice`, supplying the seed on stdin. Importing does not
register or fund an identity.

### Request testnet funds

Use the faucet address and CA certificate supplied with your testnet access.
Set `FAUCET_URL`, `TESTNET_CA_FILE`, `WALLET_DID`, and `WALLET_PUBLIC_KEY`
to your endpoint and the public values from `layerx wallet list`.
Choose a unique `FAUCET_REQUEST_ID` of 16–128 letters, digits, dashes, or
underscores, and retain it for retries.

```bash
jq -n --arg did "$WALLET_DID" --arg public_key "$WALLET_PUBLIC_KEY" \
  '{did:$did, public_key:$public_key}' > faucet-request.json
curl --fail --silent --show-error --cacert "$TESTNET_CA_FILE" \
  --request POST "$FAUCET_URL/v1/faucet/claims" \
  --header 'Content-Type: application/json' \
  --header "Idempotency-Key: $FAUCET_REQUEST_ID" \
  --data-binary @faucet-request.json > faucet-response.json
jq -e '.funded == true and .funding_id != null' faucet-response.json
```

A failed or indeterminate claim is not funding confirmation.

### Send and create a token

Amounts are integer base units. `ASSET_ID` is the asset's 64-character
hexadecimal identifier; `RECIPIENT_DID` is the recipient's DID.
The recipient's account must already exist for that asset.

Configure your testnet environment with its network ID and store your gateway
credential under the `testnet` alias. Obtain a receipt policy
from an independently trusted operator; do not derive trust pins from the RPC
response being verified. The JSON file contains `protocol_version` (3),
`network_id`, `sequencer_id` and `sequencer_key` (64 hexadecimal characters each),
`first_batch` and `last_batch` (an inclusive authorized range), and
`checkpoint_context_digest` (a SHA-256 hexadecimal digest, required for finality).
Use `null` for the checkpoint digest when only execution or batch verification
is needed. Set `RECEIPT_POLICY` to this file and `FEE_LIMIT` to your maximum fee
in base units.

```bash
layerx --rpc "$RPC_URL" --gateway-credential testnet token create --symbol PAY --name 'Payment Token' \
  --decimals 6 --supply-cap 1000000000 --salt "$(openssl rand -hex 32)" \
  --receipt-policy "$RECEIPT_POLICY" --fee-limit "$FEE_LIMIT" --wait executed
layerx --rpc "$RPC_URL" --gateway-credential testnet wallet open-account --asset "$ASSET_ID" \
  --receipt-policy "$RECEIPT_POLICY" --fee-limit "$FEE_LIMIT"
layerx --rpc "$RPC_URL" --gateway-credential testnet token mint --asset "$ASSET_ID" \
  --to "$RECIPIENT_DID" --amount 100 \
  --receipt-policy "$RECEIPT_POLICY" --fee-limit "$FEE_LIMIT"
layerx --rpc "$RPC_URL" --gateway-credential testnet token burn --asset "$ASSET_ID" --amount 1 \
  --receipt-policy "$RECEIPT_POLICY" --fee-limit "$FEE_LIMIT"
```

Token creation prints its derived asset ID in the disclosure. Use that identifier
for subsequent commands. Writes print the complete disclosure to stderr before
signing. They submit canonical signed bytes and verify the receipt against the
locally computed activity ID. A successful result reports the activity ID,
receipt result and commitment reached. A failed native receipt exits nonzero.
An acknowledgement alone never counts as success. `--timeout-seconds` accepts
1–300 seconds and defaults to 60; pending outcomes retain the activity ID for
later receipt retrieval. Do not blindly repeat a pending write: each invocation
creates a new idempotency key.

The Send command syntax is:

```bash
layerx --rpc "$RPC_URL" --gateway-credential testnet wallet send --to "$RECIPIENT_DID" \
  --asset "$ASSET_ID" --amount 100 --wait executed \
  --receipt-policy "$RECEIPT_POLICY" --fee-limit "$FEE_LIMIT"
layerx --rpc "$RPC_URL" --gateway-credential testnet token transfer --to "$RECIPIENT_DID" \
  --asset "$ASSET_ID" --amount 10 --wait finalised \
  --receipt-policy "$RECEIPT_POLICY" --fee-limit "$FEE_LIMIT"
```

Send reads the identity sequence using `lx_getSequence([did, "identity"])`
and reads the source-account sequence independently. The source and DID
destination accounts are selected from the authenticated `lx_getBalances`
snapshot by exact asset ID, account name and recomputed `LX:ACCOUNT:v1` ID. This
supports a deployment-specific native custody asset without treating it as a
token account. The CLI discloses and signs the native debit authorization before
disclosing and signing the shared envelope. An unavailable identity/account
snapshot, ambiguous account, invalid signature or missing commitment evidence
produces an error. Emulator Send remains unavailable.

| Commitment | Required evidence |
| --- | --- |
| `executed` | A sequencer-signed receipt bound to the activity ID |
| `batched` | Receipt inclusion in an authorized, signed batch header |
| `finalised` | A verified guarantor checkpoint certificate for that same header |

Retrieve or wait for a receipt with:

```bash
layerx --rpc "$RPC_URL" --gateway-credential testnet wallet receipt "$ACTIVITY_ID" \
  --receipt-policy "$RECEIPT_POLICY" --wait finalised
```

Without `--rpc`, receipt retrieval retains the existing REST execution-signature
verification using the configured sequencer key. Stronger commitments require
RPC and the explicit receipt policy.

### Fee estimates and live notifications

Estimate fees using already encoded canonical activity bytes:

```bash
layerx --rpc "$RPC_URL" --gateway-credential testnet wallet estimate-fee "$CANONICAL_HEX"
layerx --rpc "$RPC_URL" --gateway-credential testnet wallet watch receipts
layerx --rpc "$RPC_URL" --gateway-credential testnet wallet watch checkpoints
layerx --rpc "$RPC_URL" --gateway-credential testnet wallet watch account --account-id "$ACCOUNT_ID"
```

Fee estimation forwards to the native fee schedule and preserves unavailable
errors. Each watch reads one notification over authenticated `/rpc/ws` and
exits; `--timeout-seconds` is bounded to 1–300 seconds. Receipts require
`receipt:read`; checkpoints and account topics require `state:read`.
Notifications are marked `verified: false`; they establish no commitment.
Use `wallet receipt` with your receipt policy to verify commitment. On timeout,
closure or feed loss, reconcile using RPC reads before starting another watch.
The stream provides no durable replay. Remote connections require validated TLS.

### Public JSON-RPC

Select a configured testnet profile, then pass the complete RPC endpoint:

```bash
layerx --rpc "$RPC_URL" --gateway-credential testnet wallet balance --did "$WALLET_DID"
layerx --rpc "$RPC_URL" --gateway-credential testnet wallet balance --did "$WALLET_DID" --asset "$ASSET_ID"
layerx --rpc "$RPC_URL" --gateway-credential testnet wallet receipt "$ACTIVITY_ID" --receipt-policy "$RECEIPT_POLICY"
layerx --rpc "$RPC_URL" --gateway-credential testnet token info "$ASSET_ID"
layerx --rpc "$RPC_URL" --gateway-credential testnet token list
```

`RPC_URL` must end in `/rpc`; remote endpoints require HTTPS. Requests use
positional parameters from the public API contract. Unknown methods, malformed
responses, and unavailable native evidence exit nonzero. DID enumeration,
asset listing/detail and fee estimation consume authenticated native snapshots;
they never invent an empty list or estimate when the upstream evidence is
unavailable. Token info and list call `lx_getAsset` and `lx_listAssets`. DID
listing errors retain the remote code, message and data.

## MCP payment surface

The MCP payment tools currently expose `wallet.send`, `token.create`,
`token.mint` and `token.transfer` through the daemon's ordinary signing and
scope checks. MCP does not expose the CLI's burn, open-account, token info or
token list operations through that payment surface. CLI and MCP command
availability differ; installing MCP does not enable these missing tools.

## Headless credential storage

The OS keyring is the default. On a headless Linux server, container, or CI
runner, explicitly select the encrypted file store before creating a key:

```bash
export LAYERX_CREDENTIAL_STORE=file
read -r -s -p 'Credential passphrase: ' LAYERX_CREDENTIAL_PASSPHRASE
export LAYERX_CREDENTIAL_PASSPHRASE
layerx --json key create quickstart
layerx --json key list
layerx --json auth status
```

Use a strong passphrase of 12–16384 bytes. In CI, supply
`LAYERX_CREDENTIAL_PASSPHRASE` through the CI secret environment. Keep it
available for subsequent CLI and MCP/A2A processes, and unset it when finished.
Secret imports still read stdin; the passphrase does not consume that input.
`auth status` reports whether an API token is stored, independently of keys;
store one with `layerx auth set` using the token on stdin.

The store writes `credentials/vault` beside the resolved CLI config file
(`LAYERX_CONFIG`, otherwise `$XDG_CONFIG_HOME/layerx/config.json`, otherwise
`$HOME/.config/layerx/config.json`). It encrypts all keys, tokens and gateway
credentials using AES-256-GCM and PBKDF2-HMAC-SHA256 with 600,000 iterations,
a fresh 16-byte salt and a fresh 12-byte nonce on every update. Vault and lock
files use mode 0600 inside a mode 0700 directory. Group/world-accessible files,
symlinks, incorrect ownership, wrong passphrases and damaged vaults are refused.
Updates use a process lock and atomic replacement. This backend currently
requires Unix file permissions.

There is no automatic fallback or migration between stores. Unset
`LAYERX_CREDENTIAL_STORE` (or set it to `os`) to use the OS keyring. Retain the
passphrase and encrypted vault together in your backup procedure: a lost
passphrase cannot be recovered. Changing the environment passphrase does not
rotate the vault password; it makes authentication fail.
