# Getting started on testnet

This checklist uses the public gateway and faucet contracts; access still requires credentials
and independently supplied verification policy.

## Endpoints and trust

```sh
export RPC_URL=https://api.testnet.layerx.network/rpc
export FAUCET_URL=https://faucet.testnet.layerx.network
```

Store a gateway `LayerX-Key` credential under the CLI alias `testnet`, keep the
faucet bearer session out of command history, and obtain the network id and
receipt-policy trust pins independently of any response you are verifying.

For the disposable local beta cluster, follow [Testnet quickstart](Quickstart.md)
and source `build/beta-cluster/env`. That file exports the local origins,
credential-file paths, and `LAYERX_TEST_CA_FILE` at
`build/beta-cluster/ca/ca.crt`. Add `--cacert "$LAYERX_TEST_CA_FILE"` only to
requests for those local cluster endpoints.

## Wallet and faucet

For a public testnet identity, create an ordinary signing key. Public
`wallet create` is deliberately unavailable because the gateway exposes no
DID-registration route:

```sh
layerx key create alice
layerx key default alice
layerx wallet list
```

Request test funds with the wallet's public DID and 64-hex Ed25519 key. The
faucet body accepts no other fields:

```sh
jq -n --arg did "$WALLET_DID" --arg public_key "$WALLET_PUBLIC_KEY" \
  '{did:$did, public_key:$public_key}' > faucet-request.json

curl --fail-with-body --silent --show-error \
  --request POST "$FAUCET_URL/v1/faucet/claims" \
  --header 'Content-Type: application/json' \
  --header "Authorization: Bearer $FAUCET_SESSION_TOKEN" \
  --header "Idempotency-Key: $FAUCET_REQUEST_ID" \
  --data-binary @faucet-request.json > faucet-response.json
```

Reuse the same idempotency key only for the same claim. Accept funding only
after a `funded: true` response and a confirming account read. HTTP 202
`still_checking` is not funding evidence. The exact pending body is:

```json
{"state":"still_checking","retry":"after","retry_after_seconds":10}
```

A faucet refusal is an HTTP 4xx/5xx body shaped as
`{"error":{"code":"<typed-code>","retry":"after|never"}}`; when retry is
`after`, `retry_after_seconds` is present. The source refuses missing or invalid
idempotency, authentication, malformed DID/key input, quota, persistence, and
funding independently (`platform/hosted/faucet/src/main.rs:1025-1098,
1012-1035`).

## Run the register, open, mint, send flow

The tracked real-process flow performs these writes in order:
faucet claim, Asset register, per-Asset account open, mint, then native-Asset
SEND. Register, open, mint, and SEND are four fresh signed canonical
activities. Each is submitted through the same exact JSON-RPC method and waits
for `executed` before the next activity is built
(`platform/hosted/gateway/tests/local/lifecycle.rs:1614-1647, 1650-1813,
1888-1931`).

Read the identity sequence before constructing each activity. SEND also
consumes the source-account sequence. Set each activity variable only after
those reads, using freshly encoded, disclosed, and signed bytes. Do not copy
canonical bytes from a previous run: sequence, timestamps, and idempotency are
bound into the signature.

```sh
rpc_read() {
  jq -cn --argjson id "$1" --arg method "$2" --argjson params "$3" \
    '{jsonrpc:"2.0",id:$id,method:$method,params:$params}' |
    curl --fail-with-body --silent --show-error "$RPC_URL" \
      --header 'Content-Type: application/json' --data-binary @-
}

rpc_write() {
  jq -cn --argjson id "$1" --arg canonical "$2" \
    '{jsonrpc:"2.0",id:$id,method:"lx_sendActivity",params:[$canonical,"executed"]}' |
    curl --fail-with-body --silent --show-error "$RPC_URL" \
      --header 'Content-Type: application/json' \
      --header "Authorization: LayerX-Key $GATEWAY_KEY_ID:$GATEWAY_KEY_SECRET" \
      --data-binary @-
}

rpc_read 90 lx_getSequence \
  "$(jq -cn --arg did "$WALLET_DID" '[$did,"identity"]')" \
  > register-sequence.json
export REGISTER_ACTIVITY_HEX='<fresh signed Asset register activity>'
rpc_write 101 "$REGISTER_ACTIVITY_HEX" > register-response.json

# After verifying register-response.json and reading the fresh sequence:
rpc_read 91 lx_getSequence \
  "$(jq -cn --arg did "$WALLET_DID" '[$did,"identity"]')" \
  > open-sequence.json
export OPEN_ACTIVITY_HEX='<fresh signed Asset account-open activity>'
rpc_write 102 "$OPEN_ACTIVITY_HEX" > open-response.json

# After verifying open-response.json and reading the fresh sequence:
rpc_read 92 lx_getSequence \
  "$(jq -cn --arg did "$WALLET_DID" '[$did,"identity"]')" \
  > mint-sequence.json
export MINT_ACTIVITY_HEX='<fresh signed Asset mint activity>'
rpc_write 103 "$MINT_ACTIVITY_HEX" > mint-response.json

# After verifying mint-response.json and reading both required sequences:
rpc_read 93 lx_getSequence \
  "$(jq -cn --arg did "$WALLET_DID" '[$did,"identity"]')" \
  > send-identity-sequence.json
rpc_read 94 lx_getSequence \
  "$(jq -cn --arg account "$SOURCE_ACCOUNT_ID" '[$account]')" \
  > send-account-sequence.json
export SEND_ACTIVITY_HEX='<fresh signed Asset SEND activity>'
rpc_write 104 "$SEND_ACTIVITY_HEX" > send-response.json
```

Every successful write must have `result.commitment == "executed"`, a nonempty
`result.receipt`, and `result.result_code == 0`. Verify the sequencer signature
and exact activity binding under independently supplied policy before using
the result. The register payload fixes the Asset ID, salt, symbol, name,
decimals, supply cap, issuer kind, and custody reference. Account open names
that Asset; mint names its derived per-Asset account and amount; SEND names the
source/destination DIDs, native Asset, amount, source-account sequence, validity
window, idempotency key, and fee limit. Use the canonical builders described in
[Assets and tokens](Assets.md) rather than hand-assembling these payloads.

## Read sequences, balances, and receipts

Reads need no gateway key. This helper preserves the exact positional method
contract; it is the same `rpc_read` helper used above:

```sh
rpc_read 201 lx_getAccount \
  "$(jq -cn --arg account "$SOURCE_ACCOUNT_ID" '[$account]')"
rpc_read 202 lx_getSequence \
  "$(jq -cn --arg did "$WALLET_DID" '[$did,"identity"]')"
rpc_read 203 lx_getSequence \
  "$(jq -cn --arg account "$SOURCE_ACCOUNT_ID" '[$account]')"
rpc_read 204 lx_getBalance \
  "$(jq -cn --arg account "$SOURCE_ACCOUNT_ID" '[$account]')"
rpc_read 205 lx_getBalance \
  "$(jq -cn --arg account "$DESTINATION_ACCOUNT_ID" '[$account]')"
rpc_read 206 lx_getBalances \
  "$(jq -cn --arg did "$WALLET_DID" '[$did]')"
rpc_read 207 lx_listAssets '[]'
rpc_read 208 lx_getAsset \
  "$(jq -cn --arg asset "$ASSET_ID" '[$asset]')"

export SEND_ACTIVITY_ID="$(jq -r '.result.activity_id' send-response.json)"
rpc_read 209 lx_getReceipt \
  "$(jq -cn --arg activity "$SEND_ACTIVITY_ID" '[$activity]')" \
  > receipt-response.json
rpc_read 210 lx_getActivityStatus \
  "$(jq -cn --arg activity "$SEND_ACTIVITY_ID" '[$activity]')" \
  > status-response.json
```

Use `lx_getSequence([did, "identity"])` for the envelope sequence and
`lx_getSequence([account_id])` for a source-account sequence; they are
independent. Build a new activity only after reading the sequence it consumes.
The flow test reads the source-account sequence before SEND, then proves that
the `lx_getReceipt` and `lx_getActivityStatus` receipt bytes equal the receipt
returned by the write
(`platform/hosted/gateway/tests/local/lifecycle.rs:971-988, 1777-1885`).

`lx_getBalances` fails closed if the native DID-account listing cannot be fully
decoded or comes back with no entries, because an empty listing carries no proof
of absence. JSON-RPC code `-32001` with
`error.data.error.code == "did_account_listing_unavailable"` is unavailable
evidence, not a partial or empty balance list.

## Error and recovery contract

JSON-RPC responses use HTTP 200 even when they contain `error`. The exact
failure envelope is:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "error": {
    "code": -32602,
    "message": "Invalid params"
  }
}
```

Invalid params are `-32602`; missing/invalid authorization or scope is
`-32002`; an unavailable read or submission is `-32001`; rate/capacity refusal
is `-32005`; invalid upstream data is `-32603`. Proxied typed failures are kept
under `error.data` (`platform/hosted/gateway/src/rpc.rs:153-184, 242-256`).

For a requested `executed`, `batched`, or `finalised` level that is not yet
available, the response is `-32001` and `error.data` includes
`state: "pending"`, `requested_commitment`, and either current `evidence` or
the upstream pending body (`platform/hosted/gateway/src/rpc.rs:219-224,
334-357`). Retain the exact signed activity and recover by activity id with
`lx_getActivityStatus`, `lx_getReceipt`, and the required `lx_getProof` reads.
Never replace an uncertain payment with a newly signed debit.

The complete 15-method contract and error codes are in
[Public JSON-RPC](PublicRpc.md). The exact unshortened register/open/mint/SEND
requests and balance/receipt responses captured by the tracked flow are in
[Public payment API](PublicAPI.md). Full wallet, token, LXT-20, and HTTP 402
commands are in [Payments developer path](PaymentsQuickstart.md).

[Home](Home.md)
