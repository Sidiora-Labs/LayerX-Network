# LayerX Agent Interface

The interaction layer has no protocol authority. Every state-changing operation is a canonical LayerX activity signed by protocol-recognised authority and submitted as the exact signed bytes; this workspace never invents, applies, or asserts protocol state.

The LayerX Node Interface is the sole boundary to the C17 core. Agent crates never open node storage, read append-only logs, bind private C layouts, or become build dependencies of the protocol runtime.

This Rust 2021 workspace owns agent-facing types, canonical encoding, cryptography, proof verification, the boundary client, daemon API, daemon, MCP server, and SDK. It builds independently from the C core through the `agent-*` Make targets at the repository root.

`make agent-check-boundary` enforces the node-interface boundary by rejecting forbidden storage dependencies, node-private paths, C-core linkage, generated bindings, and unapproved C-layout declarations. An exception is a protocol design change: it must be added to the published stable ABI allowlist through the specification process and cannot be suppressed with a source comment.

## MCP

The MCP server is [`crates/layerx-mcp`](crates/layerx-mcp/README.md): one tenant, one scope set, daemon-only routing. Interop MCP/A2A transports and the `layerx install mcp` / `layerx install a2a` CLI live next door in [`interop/`](../interop/README.md) and `platform/cli/`.
