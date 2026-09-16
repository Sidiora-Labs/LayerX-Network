# Interoperability

Adapters at the edge translate someone else's protocol into evidence shaped
for LayerX. They never write balances. `402LXP` remains the only balance
writer. Custody and withdrawal guarantees stay on Paxeer
(`interop/README.md`; `spec/layerx-platform/spec.kvx`, decision
`interop_edge`).

The platform specification (requirement 32) requires first-class x402 v2
buyer, seller, and facilitator roles; AP2 checkout and payment mandates; UCP
merchant profiles and order flows; A2A and MCP payment transports; and Visa
Trusted Agent credentials. Requirement 33 covers Ethereum and Solana
migration plus card, bank, and real-time-payment adapter interfaces.
Requirement 34 covers batch mirrors, the ramp toolkit, and domain-tagged
claim vocabulary.

## Surfaces in this tree

| Surface | Tree | Page |
| --- | --- | --- |
| x402 v2 | `interop/crates/layerx-x402` | [x402](x402.md) |
| AP2 | `interop/crates/layerx-ap2` | [AP2](ap2.md) |
| UCP | `interop/crates/layerx-ucp` | [UCP](ucp.md) |
| Visa TAP | `interop/crates/layerx-visa-tap` | [Visa TAP](visa-tap.md) |
| Portable receipts | `interop/crates/layerx-portable` | [Portable receipts](portable-receipts.md) |
| Fiat rails | `interop/crates/layerx-fiat` | [Ramps and fiat](ramps.md) |
| Market-maker ramps | `platform/ramps` | [Ramps and fiat](ramps.md) |
| Ethereum / Solana mirrors | `interop/crates/layerx-mirror` | [Mirrors](mirrors.md) |
| ETH / Solana migration | `interop/crates/layerx-migrate` | See `interop/crates/layerx-migrate/OPERATIONS.md` |

There is no standalone `layerx-a2a` crate. A2A is a transport on the gateway
and on x402, plus the CLI installer in `platform/cli`.

## Honesty rule

When an external protocol's flow claims an outcome, the adapter may present
it only as far as backing evidence supports: a verified LayerX receipt, a
verified mandate, or verified external settlement
(`spec/layerx-platform/spec.kvx`, requirement 32 `ac_6`).
