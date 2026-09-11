# Local gateway lifecycle qualification

This harness runs the existing `../lifecycle-boundary.sh` unchanged against real
gateway, identity, receipt-authority, TLS Redis, core boundary, sequencer and
authority-replica processes. It provisions a local identity through the identity
API and obtains a signer-bound API key through `POST /v1/keys`; it never inserts
gateway key records directly. All keys, tokens and certificates are generated
locally. An unrelated signer cannot obtain a key, issuance is replayed exactly,
and an unrelated TLS root cannot authenticate the gateway.

The build adapter includes the existing core `Cluster` fixture, preserving its
tests and assertions. Only documentation-comment syntax and the two compile-time
source/binary paths are adapted. No native build is performed by this adapter.
The exact test selection below is required: the included core tests remain
available but are not this gateway gate.

Prerequisites: Linux root (the real daemon runs under its separate UID), OpenSSL,
TLS-enabled `/usr/bin/redis-server`, curl, jq, xxd and Python 3. The parent must
first build `build/bin/layerxd`, `build/bin/layerx-genesis-build` and the platform
binaries `layerx-core-boundary`, `layerx-identity`, `layerx-receipt-authority`,
`layerx-gateway`. Use the actual absolute binary directory, not production service
paths. This does not require production credentials or any emulator.

After the parent authorizes serialized qualification, from the repository root:

```sh
export LAYERX_TEST_SERVICE_BIN_DIR=/absolute/path/to/platform/debug
export LAYERX_TEST_CORE_BIN="$LAYERX_TEST_SERVICE_BIN_DIR/layerx-core-boundary"
cargo test --manifest-path platform/hosted/gateway/tests/local/Cargo.toml \
  --test lifecycle local_gateway_lifecycle -- --exact --nocapture
```

The first Cargo invocation resolves this standalone test package's lockfile;
subsequent runs should use `--locked`. Keep its target directory separate from
concurrent platform/native qualification. Each real-node fixture, including the
funded cluster this harness builds from the included core fixture, owns the
core `TestState` guard and removes its generated state root when dropped; set
`LAYERX_TEST_RETAIN_STATE` to keep the tree for inspection.

The shell's six required variables are supplied by the harness. Its receipt
verifier is a separate executable using `layerx-proof`, a locally pinned
sequencer key, and the manifest of locally signed canonical activities. It
requires network 7332, protocol 3, Programs version 4, state operation 0, success,
absent call outcome, matching activity ID and a valid state proof. The verifier
fetches the real authority's batch-header signature and receipt inclusion proof
over authenticated TLS, pins replica/sequencer identities, verifies header
signature, network, sequence range and receipt inclusion, then resolves the
batch identity, asset and state roots through
`layerx_platform_authority::authorized_batch_by_activity`, which re-derives
them from the verified header and the decoded receipt and accepts both the
historical and the occupancy-maintenance evidence shapes; the Programs state
proof is checked through the verifier matching that shape. A mutated header,
mutated authority evidence and corrupted receipt bytes must each fail
verification.

Evidence is retained beside each receipt and replayed by a fresh verifier process
without authority URL or transport configuration. All three lifecycle receipts
must pass this offline replay. Authentication failure diagnostics redact response
bodies, and key replay compares exact values without formatting credentials.
The real gateway separately obtains authority from the receipt-authority service.

This gate covers deploy, upgrade and deprecate, authenticated route refusals,
signature rejection and idempotent replay. The same harness also exercises
`POST /rpc` (`local_gateway_rpc`) and authenticated `GET /rpc/ws`
(`local_gateway_websocket_receipt_wake`). It does not claim CALL occupancy,
custody-funded execution, explorer reads or production certification. No tests
or builds are implied by source availability. The funded SEND latency
selection remains a separate qualifier and is not implied by this README.
