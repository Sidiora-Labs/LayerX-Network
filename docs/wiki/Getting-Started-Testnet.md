# Getting started on testnet

The payment surfaces on this page are on the testnet branch. This checklist
uses the public gateway and faucet contracts; access still requires credentials
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
`still_checking` is not funding evidence.

## Read public state

Reads need no gateway key. For example:

```sh
curl --fail-with-body --silent --show-error "$RPC_URL" \
  --header 'Content-Type: application/json' \
  --data '{"jsonrpc":"2.0","id":1,"method":"lx_getBalances","params":["'"$WALLET_DID"'"]}'
```

Use `lx_getSequence([did, "identity"])` for the envelope sequence and
`lx_getSequence([account_id])` for a source-account sequence. They are
independent. The complete method and error contract is in
[Public JSON-RPC](PublicRpc.md).

## Submit and recover

Canonical submission requires a `LayerX-Key` with `activity:write` and any
route-specific Programs scope:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "lx_sendActivity",
  "params": ["<canonical activity hex>", "executed"]
}
```

Send this object to `POST /rpc` with `Content-Type: application/json` and the
stored gateway credential. Use exactly `executed`, `batched`, or `finalised`.
An unavailable requested level returns JSON-RPC `-32001` with `state: pending`;
retain the signed activity and recover by activity id with
`lx_getActivityStatus`, `lx_getReceipt`, and the required proof reads. Never
replace an uncertain payment with a newly signed debit.

The CLI performs canonical encoding, disclosure, signing, submission, and
receipt verification for its wallet/token commands. Full commands and the
token, LXT20, and HTTP 402 paths are in
[Payments developer path](PaymentsQuickstart.md).

[Home](Home.md)
