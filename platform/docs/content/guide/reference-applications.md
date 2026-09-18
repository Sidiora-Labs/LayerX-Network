# Reference applications

Four cloneable applications live under `platform/examples`: a buyer agent that pays a metered API through buyer middleware, the paid API protected by seller middleware, a merchant shop with durable receipt-backed orders and signed settlement webhooks, and a marketplace implemented as a LayerX Network program.

The checked-in `platform/examples/reference-apps.json` is the launch manifest. It names the exact emulator and testnet command for every application. Public endpoints, the receipt protocol version (`protocolVersion`, `3` for both profiles) and the names of required environment variables are declared in each application's `layerx.example.json`; selecting a profile is the only way to select a network. A profile without a selectable `protocolVersion` is refused by name, and `run-reference-apps.mjs --check` refuses a profile whose declared version differs from the version the emulator scenario runs. Tokens remain server-side environment values and are never compiled into browser code.

```
npm ci
npm run build
npm run start:emulator --workspace @sidiora/layerx-example-buyer-agent
npm run start:emulator --workspace @sidiora/layerx-example-paid-api
npm run start:emulator --workspace @sidiora/layerx-example-merchant-shop
npm run start:emulator --workspace @sidiora/layerx-example-marketplace
```

`npm run build` is the one build command for every JavaScript workspace in this repository. It orders the workspaces by their declared dependencies, so the middleware an application imports is compiled before the application that imports it, and it needs no environment variable: the samples read their declared configuration on the first request, not at build time.

Replace `start:emulator` with `start:testnet` to use the testnet profile. `merchant-checkout` remains an alias package for existing consumers; `merchant-shop` is the reference manifest name.

All successful payment paths resolve live receipt authority and run independent receipt verification. A response declared Pending remains Pending, an indeterminate transport, omitted receipt, or authority response remains Unknown, and an explicit refusal remains Refused. The applications do not use a static `AuthorizedBatch` as their payment path.

The marketplace source is in `platform/examples/marketplace/program`. It stores listings and consumed receipt digests in program-shared storage and buys one only after the runtime exposes a verified, previously unused receipt matching its asset and minimum price. The transfer is a bounded 402LXP request to the listing's seller, and receipt consumption, listing deletion, transfer, and purchase event emission occur in the same atomic program call.

## Enforced by

| Capability | Layer | What that means here |
|---|---|---|
| Receipt-gated resource release | `service` | Seller and merchant resources are released only after middleware verifies a canonical receipt. |
| Exactly-once fulfilment | `service` | Durable fulfillment records bind idempotency keys, request digests, receipt bytes, batch authority, and released data. |
| Verified, replay-protected webhooks | `service` | Merchant settlement webhooks are signature checked, replay claimed, receipt resolved, and independently verified. |
| Programs never write balances | `protocol` | The marketplace can request only the bounded transfer authority supplied to its call. |
| Unknown is a real outcome | `agent-layer` | Applications expose Unknown separately and do not turn a transport ambiguity into success or refusal. |
