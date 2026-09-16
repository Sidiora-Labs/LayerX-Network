# Interoperability adapters

Interop crates translate foreign protocols into LayerX-shaped
evidence. They never write balances (`interop/README.md`). The
executable edge is `layerx-interop-gateway` plus
`layerx-interop-service`. State-changing routes must end in a
receipt-verified LayerX operation
(`interop/crates/layerx-interop-gateway`).

Adapter registration ids in the gateway are `x402`, `ap2`, `ucp`,
`visa-tap`, `fiat`, and `migration`. Ingress transports are `http`,
`mcp`, and `a2a` (`interop/crates/layerx-interop-gateway/src/server.rs`).

Evidence policies parsed by the gateway:
`layerx-receipt`, `verified-mandate+layerx-receipt`,
`trusted-agent-credential`, `external-settlement+layerx-receipt`.

`spec/layerx-platform/spec.kvx` marks x402, AP2, UCP, Visa TAP, fiat,
and portable verification tasks **done**. Migration tooling, mirror
publisher/verify, and the market-maker ramp toolkit are recorded as
**implemented** with production-client or reference-service gaps
noted in the spec `reality` fields. Parent rollup tasks 22–26 remain
**pending**. This page does not claim those rollups are finished.

---

## Gateway routes

`interop_gateway_routes()` in
`interop/crates/layerx-interop-gateway/src/server.rs`:

| Method | Path |
| --- | --- |
| GET | `/livez`, `/readyz` |
| GET | `/v1/adapters` |
| GET | `/v1/operations/{64-hex}` |
| GET | `/v1/{http\|mcp\|a2a}/x402/supported` or `.../facilitator/supported` |
| POST | `/v1/{http\|mcp\|a2a}/x402/buyer/build` |
| POST | `/v1/{http\|mcp\|a2a}/x402/seller/offer` |
| POST | `/v1/{http\|mcp\|a2a}/x402/verify` or `.../facilitator/verify` |
| POST | `/v1/{http\|mcp\|a2a}/x402/settle` or `.../{seller\|facilitator}/settle` |
| POST | `/v1/http/ap2/mandates/verify` |
| POST | `/v1/http/ap2/execute` |
| POST | `/v1/http/ucp/checkouts/complete` |
| POST | `/v1/http/visa-tap/intents/verify` |
| POST | `/v1/http/visa-tap/intents/execute` |
| POST | `/v1/http/fiat/card/callbacks` |
| POST | `/v1/http/fiat/bank/callbacks` |
| POST | `/v1/http/fiat/rtp/callbacks` |

The hosted service binary is `layerx-interop-service`. Required
adapters in its config are `x402`, `ap2`, `ucp`, `visa-tap`, `fiat`;
required transports are `http`, `mcp`, `a2a`.

---

## Pages

| Topic | Page | Crate |
| --- | --- | --- |
| x402 buyer, seller, facilitator | [x402 transport](X402Transport.md) | `layerx-x402` |
| AP2 mandates | [AP2 mandates](Ap2Mandates.md) | `layerx-ap2` |
| UCP checkout | [UCP](Ucp.md) | `layerx-ucp` |
| Visa TAP | [Visa TAP](VisaTap.md) | `layerx-visa-tap` |
| Portable receipts | [Portable receipt verifier](PortableVerifier.md) | `layerx-portable` |
| Fiat ramps | [Fiat ramps](FiatRamps.md) | `layerx-fiat` |
| Market-maker ramps | [Market-maker ramps](MarketMakerRamps.md) | `platform/ramps` (not under `interop/crates`) |
| Ethereum and Solana mirrors | [Mirrors](Mirrors.md) | `layerx-mirror` |
| Ethereum and Solana migration | [Migration](Migration.md) | `layerx-migrate` |

[Programs porting](Porting.md) is the Programs guest-code porting
guide under `programs/porting`. It is not `layerx-migrate`.

[Home](Home.md)
