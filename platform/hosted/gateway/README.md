# Hosted gateway

Receipt-verifying public ingress. Routes, scopes, and TLS are on
[`docs/wiki/HostedGateway.md`](../../../docs/wiki/HostedGateway.md).

# Public JSON-RPC

`POST /rpc` is JSON-RPC 2.0. `GET /rpc/schema` serves
[`openrpc.json`](openrpc.json). `GET /rpc/ws` is the authenticated
WebSocket upgrade for `lx_subscribe`. Method names, parameter order,
commitment levels (`executed`, `batched`, `finalised`), and error
codes are defined in that document. Reads forward to the public core
URL (`LAYERX_GATEWAY_PUBLIC_CORE_URL`). Native DID enumeration, asset
listing/detail, and canonical fee estimation use authenticated core reads and
return explicit JSON-RPC unavailability when evidence is absent, never
fabricated results.
The public gateway origin is `https://api.testnet.layerx.network`.
See [Hosted gateway](../../../docs/wiki/HostedGateway.md).

`lx_sendActivity` accepts the strictly decoded native Asset operations register
(1), account_open (4), send (5), receive (6), grant_issue (7), grant_revoke (8),
mint (10), and burn (11). Pause/unpause (2/3) are not authenticated activity
surfaces, and Asset ordinal 9 is reserved and refused. The same method carries
Programs deploy (1), upgrade (2), call (3), transfer (5), account registration
(6), and wind-down (7). `lx_estimateFee` accepts exactly those activity types
and prices their canonical envelope bytes against the authenticated committed
schedule; it fails closed if the schedule requires execution or storage units
that the request cannot supply.

Method list: [`docs/wiki/PublicRpc.md`](../../../docs/wiki/PublicRpc.md).
Commitment parameter: [`docs/wiki/CommitmentLevels.md`](../../../docs/wiki/CommitmentLevels.md).
Exact real-process request and response pairs:
[`docs/wiki/PublicAPI.md`](../../../docs/wiki/PublicAPI.md).

Reads are unauthenticated. `lx_sendActivity` requires a `LayerX-Key` with
`activity:write` and the route scope for Programs operations. It admits Asset
ordinals `1`, `4`, `5`, `6`, `7`, `8`, `10`, `11` and Programs ordinals `1`,
`2`, `3`, `5`, `6`, `7`. It returns success only with a verified receipt and
the exact requested `executed`, `batched`, or `finalised` evidence.

# Canonical module registry

`LAYERX_GATEWAY_MODULE_REGISTRY_FILE` names a JSON file shared with the human
receipt authority. The required shape is:

```json
{
  "schema_version": 2,
  "assets": [{
    "asset": "0202020202020202020202020202020202020202020202020202020202020202",
    "currency": "USD",
    "decimals": 6,
    "symbol": "$"
  }],
  "modules": [
    {"module": 1, "ordinals": [1, 4, 5, 6, 7, 8, 10, 11]},
    {"module": 9, "ordinals": [1, 2, 3, 5, 6, 7]}
  ]
}
```

The asset shown is illustrative; provision the actual network asset ID and its
metadata. Both consumers reject the unversioned shape and version 1. Every asset
ID is nonzero lowercase 64-digit hex and unique. There must be 1..256 assets;
currency and symbol contain 1..32 UTF-8 bytes without control characters;
decimals is an unsigned integer from 0 through 38. Unknown fields refuse.

Gateway retains its existing module validation, eight-module bound and Programs
lifecycle-ordinal insertion. The public activity registry must contain the exact
Asset and Programs admission sets shown above; it must not include Asset 2, 3,
or reserved ordinal 9. Authority returns the exact module registrations in the
file and its SHA-256 revision. The gateway only reads this file; the cluster
renderer must write the new shape. Authority requires a protected regular file,
so its mount must meet the authority README's ownership and path rules.
