# LayerX Human Interface

The human control plane reports protocol state only from verified LayerX Network receipts or verified Paxeer finality evidence. A movement or mutation is never presented as Done from a local assertion, and an unknown outcome remains still checking until receipt lookup resolves it.

The product exposes five ideas: log in, add money, move money, manage agents, and see what happened. Protocol mechanisms stay behind those ideas, and the application never asks a person to choose or understand an internal route.

The Rust workspace contains the human service, the sole typed-intent payload authority, the Paxeer custody-boundary client, and the rebuildable explorer index. They consume the agent-layer crates through path dependencies and reach the LayerX core only through the agent contract.

## The single payload authority

`layerx-intents` is the only component in `human/` permitted to construct protocol payload bytes. Every other crate and the web application describe protocol effects as typed intents and receive canonical bytes back as opaque evidence; none of them may depend on `layerx-wire` or invoke its encoding entry points. The rule exists so the disclosure a person approves is provably the bytes that get signed: one audited compiler produces the payload, the disclosure round-trip gate proves it re-encodes byte-identically, and no second encoder can drift from it. `make human-check` runs the payload-authority gate over the crate tree, and CI additionally scans the built web bundle to prove the browser ships no payload-encoding code path.
