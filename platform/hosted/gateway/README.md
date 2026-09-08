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
  "modules": [{"module": 9, "ordinals": [1, 2, 7]}]
}
```

The asset shown is illustrative; provision the actual network asset ID and its
metadata. Both consumers reject the unversioned shape and version 1. Every asset
ID is nonzero lowercase 64-digit hex and unique. There must be 1..256 assets;
currency and symbol contain 1..32 UTF-8 bytes without control characters;
decimals is an unsigned integer from 0 through 38. Unknown fields refuse.

Gateway retains its existing module validation, eight-module bound and Programs
ordinal insertion. Authority returns the exact module registrations in the file
and its SHA-256 revision. The gateway only reads this file; the cluster renderer
must write the new shape. Authority requires a protected regular file, so its
mount must meet the authority README's ownership and path rules.
