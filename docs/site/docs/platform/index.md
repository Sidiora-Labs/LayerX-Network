# Developer platform

The platform specification (`spec/layerx-platform/spec.kvx`) adds four
pillars on top of the human control plane: the developer platform, LayerX
Programs, the interoperability gateway, and the multichain surface.

This section covers hosted developer surfaces that exist in
`platform/hosted/` and the public JSON-RPC gateway.

## Hosted surfaces

| Surface | Page |
| --- | --- |
| Public JSON-RPC (`POST /rpc`, `GET /rpc/ws`) | [Gateway RPC](gateway-rpc.md) |
| Hosted gateway binary and `/v1` routes | [Hosted gateway](hosted-gateway.md) |
| Faucet claims | [Faucet](faucet.md) |
| Program registry | [Registry](registry.md) |
| Webhooks | [Webhooks](webhooks.md) |
| Sequencer pod and colocated boundaries | [Hosted node](hosted-node.md) |
| Core TLS boundary | [Hosted core](hosted-core.md) |
| Receipt authority | [Hosted authority](hosted-authority.md) |
| Agent LNI boundary | [Agent boundary](agent-boundary.md) |
| Hosted identity | [Identity](identity.md) |
| Public payment transcript | [Public API](public-api.md) |
| Developer CLI | [CLI](cli.md) |
| Public relay / archive nodes | [Relay and archive](relay-archive.md) |

Public origins named in the wiki are
`https://api.layerx.network/rpc` and
`https://faucet.layerx.network`
([Getting started](../overview/getting-started.md)).
Access still requires credentials and an independently supplied verification
policy.

## Developer benchmark

The platform specification gates the developer platform on adding LayerX
payments in fewer than ten lines and completing a verified test payment
within five minutes from a clean environment, following only the published
quickstart. Those figures are CI-enforced gates over published artifacts,
not marketing copy. The beta specification (`spec/layerx-beta/spec.kvx`,
requirement 1) requires the published install commands to provision
sequencer seed and trust-anchor inputs explicitly. Treat that benchmark as
specified; this documentation does not record a passing measurement.
