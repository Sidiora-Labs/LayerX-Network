# LayerX developer CLI

## Wallet quickstart

Build with `cargo build --manifest-path platform/cli/Cargo.toml`. The executable
is `platform/target/debug/layerx`; put that directory on your PATH.

Local wallet creation, imports, listing, balance reads, and signed receipt
verification are available. Transfers and token writes currently stop before
signing because the gateway does not publish the identity sequence read they
require. Public wallet registration, token metadata/listing, and history are
also unavailable in the current API. The commands report these limitations with
a nonzero exit code.

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

```bash
layerx wallet send --to "$RECIPIENT_DID" --asset "$ASSET_ID" \
  --amount 100 --wait executed
layerx token create --symbol PAY --name 'Payment Token' --decimals 6 \
  --supply-cap 1000000000 --salt "$(openssl rand -hex 32)"
layerx wallet open-account --asset "$ASSET_ID"
layerx token mint --asset "$ASSET_ID" --to "$RECIPIENT_DID" --amount 100
layerx token burn --asset "$ASSET_ID" --amount 1
layerx token transfer --to "$RECIPIENT_DID" --asset "$ASSET_ID" --amount 10
```

These write commands currently refuse the unavailable identity-sequence read;
they do not submit a transaction. The wallet never substitutes an account
sequence for an identity sequence. Send also requires signer support for
independent envelope and source-account sequences.

The accepted wait levels are `executed`, `batched`, and `finalised`.
An admission acknowledgement does not establish any of these levels.
`layerx wallet receipt "$ACTIVITY_ID"` verifies the receipt signature against
the configured sequencer key and binds the receipt to the requested activity.
It reports the receipt result and `executed` commitment; a failed receipt exits
nonzero. It does not assert batch inclusion or finality.

| Commitment | Required evidence |
| --- | --- |
| `executed` | A sequencer-signed receipt bound to the activity ID |
| `batched` | Receipt inclusion in an authorized, signed batch header |
| `finalised` | A verified guarantor checkpoint certificate for that same header |

Choosing `--wait` does not make an unavailable write executable. None of the
write commands currently reaches these confirmation stages. Retain the
activity ID whenever a service returns one, even if its outcome is pending;
an activity ID alone is not a successful payment. A receipt with a nonzero
result is a failed operation even when its execution is verified.

### Public JSON-RPC

Select a configured testnet profile, then pass the complete RPC endpoint:

```bash
layerx --rpc "$RPC_URL" wallet balance --did "$WALLET_DID"
layerx --rpc "$RPC_URL" wallet balance --did "$WALLET_DID" --asset "$ASSET_ID"
layerx --rpc "$RPC_URL" wallet receipt "$ACTIVITY_ID"
layerx --rpc "$RPC_URL" token info "$ASSET_ID"
layerx --rpc "$RPC_URL" token list
```

`RPC_URL` must end in `/rpc`; remote endpoints require HTTPS. Requests use
positional parameters from the public API contract. Unknown methods, malformed
responses, and unavailable reads exit nonzero. DID enumeration may be
unavailable even when individual account reads work. Token info and list are
absent from the current contract.

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
