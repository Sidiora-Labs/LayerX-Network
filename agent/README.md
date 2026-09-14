# LayerX Agent Interface

The interaction layer has no protocol authority. Every state-changing operation is a canonical LayerX Network activity signed by protocol-recognised authority and submitted as the exact signed bytes; this workspace never invents, applies, or asserts protocol state.

The LayerX Node Interface is the sole boundary to the C17 core. Agent crates never open node storage, read append-only logs, bind private C layouts, or become build dependencies of the protocol runtime.

This Rust 2021 workspace owns agent-facing types, canonical encoding, cryptography, proof verification, the boundary client, daemon API, daemon, MCP server, and SDK. It builds independently from the C core through the `agent-*` Make targets at the repository root.

`make agent-check-boundary` enforces the node-interface boundary by rejecting forbidden storage dependencies, node-private paths, C-core linkage, generated bindings, and unapproved C-layout declarations. An exception is a protocol design change: it must be added to the published stable ABI allowlist through the specification process and cannot be suppressed with a source comment.

The full workspace tests and sanitizers require the real native daemon and a
disposable Paxeer chain. Install the pinned Rust and Go toolchains, Foundry
(`forge`, `cast`, and `anvil`), and `tests/bridge/requirements.txt` in a Python
environment on `PATH`; download `paxeer-network/go.mod` dependencies with
`go mod download` in that directory. Run
`sudo env "PATH=$PATH" "CARGO_HOME=$HOME/.cargo" "RUSTUP_HOME=$HOME/.rustup" "GOMODCACHE=$(go env GOMODCACHE)" "GOPATH=$(go env GOPATH)" sh agent/tools/run-real-node-tests.sh test`
from the repository root, or use `sanitizers` for both sanitizer variants.
These commands build the actual native fixtures, credit signer, custody proof
tool, and Paxd before running the tests. Root launches the daemon under its
separate test UID; chain state and keys belong to temporary test directories.
`make agent-test` and `make agent-test-sanitize` require the same environment
and build the same prerequisites. The session fee test requires nine actual
Rust Client reads across the native grant, charge, replacement, and restart
lifecycle; missing prerequisites fail the test.

## MCP

The MCP server is [`crates/layerx-mcp`](crates/layerx-mcp/README.md): one tenant, one scope set, daemon-only routing. Interop MCP/A2A transports and the `layerx install mcp` / `layerx install a2a` CLI live next door in [`interop/`](../interop/README.md) and `platform/cli/`.

Payment encodings and the 20-tool catalogue containing `wallet.*`, `token.*`
and `grant.*` operations use the same daemon prepare, disclose, sign, submit,
and track path as other writes. Developer path:
[`docs/wiki/PaymentsQuickstart.md`](../docs/wiki/PaymentsQuickstart.md).
