# Interop

The interop plane translates at the edge. Adapters speak someone else's protocol on one side and hand typed, verified results to LayerX on the other - and they are structurally prevented from constructing LayerX payload bytes themselves. The plane's payload authorities are the only constructors, which is why an adapter bug cannot become a monetary bug.

## Adapters

| Adapter | Speaks | Direction |
|---|---|---|
| `layerx-x402` | x402 v2, pinned to an exact upstream spec commit and its SHA-256 | Both - buyer, seller and facilitator |
| `layerx-ap2` | AP2 mandates over JOSE | Inbound mandate verification, outbound signed evidence |
| `layerx-ucp` | UCP shopping checkout and order, pinned to a dated spec revision | Inbound |
| `layerx-visa-tap` | Visa Trusted Agent Protocol HTTP message signatures | Inbound |
| `layerx-fiat` | Certified fiat rails via opaque provider tokens | Both |
| `layerx-migrate` | Ethereum and Solana migration boundaries | Inbound |
| `layerx-mirror` | Ethereum and Solana publication of retrievable batch archives | Outbound |

Every adapter carries an `AdapterDescriptor` naming its id, its pinned specification and its conformance suite. Pinning is exact: `layerx-x402` names the upstream commit hash and the SHA-256 of the specification document it was written against. An upstream revision that has not been reviewed does not silently become the behaviour.

## Both directions are typed

Evidence leaves LayerX as a `PortableReceipt` - exact receipt bytes plus batch claims - which a counterparty verifies against an independently trusted batch authorisation. No node, gateway, daemon, database, clock or network connection is required to check it.

Evidence enters through `verify_external_evidence`. The adapter keeps ownership of its own protocol's cryptography and constraints; `layerx-portable` binds the adapter's typed result to the exact presentation and pinned specification that were verified. The result is that "this AP2 mandate was verified" is a statement about a specific presentation under a specific spec version, not a boolean.

## The gateway

`layerx-interop-gateway` is the one contract through which the interop plane reaches LayerX. It carries the principal, the trace and the translation request; it classifies translation status; and it redacts. Adapters do not reach past it.

## x402

x402 is the case most people meet first, because it is what the seller and buyer middleware speak over HTTP. The three headers are `PAYMENT-REQUIRED` on the `402`, `PAYMENT-SIGNATURE` on the retry, and `PAYMENT-RESPONSE` on the settled reply. The Rust crate exposes the same three roles - `Buyer`, `Seller`, `Facilitator` - for services that are not Node.

The relationship between the layers is worth stating plainly: x402 is the transport for the offer and the proof. It is not the settlement. The settlement is the LayerX move, and the thing that makes the settlement true is the receipt carried in the payment's extensions.

## Agent transports: MCP and A2A

Two more edges exist for the case where the counterparty is a model rather than a service. Both are served by the CLI and expose the same receipt and canonical-payment tools. Installation targets the hosted testnet or production gateway; it does not silently fall back to the emulator, whose route set does not include hosted key provisioning.

```
layerx environment use testnet --endpoint https://api.testnet.layerx.network --network-id <network-id>
layerx key create agent-runtime

layerx install mcp --environment testnet --key agent-runtime \
  --host claude-code --source-account <64-hex-funded-account> \
  --asset <64-hex-asset> --token-stdin

layerx install a2a --environment testnet --key agent-runtime \
  --source-account <64-hex-funded-account> --asset <64-hex-asset> \
  --listen 127.0.0.1:9433
```

Pipe the short-lived hosted identity session to the first command. It is used only to issue a gateway key scoped to `activity:write` and `receipt:read`; the runtime receives an opaque operating-system credential-store alias, never that identity session or the gateway secret. Repeating the command is deterministic. Use `--rotate` when the scoped gateway credential must be replaced.

`mcp serve` speaks the model context protocol over standard input and output. The supported host names are `layerx`, `claude-code`, `claude-desktop`, `cursor`, and `vscode`. `a2a serve` publishes a standard agent card and task interface on a loopback endpoint. `install a2a` writes the consumed runtime manifest and starts the managed Linux runtime; `layerx a2a status`, `layerx a2a stop`, and `layerx a2a start` provide its lifecycle. Host documents are snapshotted and restored if an installation step fails.

| Tool | Kind | Does |
|---|---|---|
| `receipt.get` | read | Fetches gateway-verified receipt material for one activity id |
| `activity.submit` | write | Builds, signs and submits an Asset SEND from the installation-bound source and asset |

`--read-only` serves only the receipt tool. It is not a prompt instruction or a policy note - the write tool is not in the surface at all, so a model that decides to spend money finds nothing to call. Every served tool declares its enforced hosted-gateway scope, whether it mutates, and what evidence it produces.

`activity.submit` never accepts a source account or an asset from a model. Those values are fixed when the runtime is installed. The call supplies the destination, exact integer amount, current account sequence, a validity interval no wider than five minutes, a fee limit and a 32-byte idempotency key. Pending and unknown gateway results remain pending or unknown; a transport failure or protocol refusal is never reported as a completed payment.

## Enforced by

| Capability | Layer | What that means here |
|---|---|---|
| Interop adapters hold no protocol authority | `protocol` | Adapters translate at the edge; payload authorities are the sole constructors of LayerX payload bytes. |
| Offline receipt verification | `protocol` | Portable receipts verify against an independently trusted batch authorisation. |
| Atomic settlement | `protocol` | A translated payment settles whole or not at all. |
| Replay refusal | `protocol` | Idempotency domains are separated per adapter and bound into the request digest. |
| Quote then commit | `service` | An x402 offer is accepted before it is paid. |
| Idempotent money moves | `service` | Fiat and migration boundaries derive keys from the request, not the attempt. |
| Capability attenuation | `agent-layer` | `--read-only` removes the write tools from the served surface entirely. |
| Agent tenancy isolation | `agent-layer` | A served transport is bound to one environment and one key. |
