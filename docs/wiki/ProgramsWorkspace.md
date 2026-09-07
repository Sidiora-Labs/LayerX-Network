# Programs workspace gates

The `programs/` tree is a Cargo workspace for the LayerX programs surface: a deterministic WASM runtime, the registry that proves what a program deployed and what it holds, the C↔Rust bridge into the protocol kernel, developer SDKs, porting kits, and the adversarial test corpus (`programs/README.md:5-7`). Guest execution is dispatched under kernel module ID `9` (`LXP_MODULE_PROGRAMS`); a program is not a ninth economic module and writes no balances of its own (`programs/README.md:9-17`).

## Workspace and crate layout

`programs/Cargo.toml` declares a Cargo workspace with `resolver = "2"` (`programs/Cargo.toml:1-26`). Workspace package metadata pins `edition = "2021"`, `rust-version = "1.91.1"`, `version = "0.1.0"` (`programs/Cargo.toml:35-39`). `programs/clippy.toml` sets `msrv = "1.91.1"` and `avoid-breaking-exported-api = false` (`programs/clippy.toml:1-2`). Workspace lints deny `unsafe_code`, Clippy `all` and `pedantic`, `unwrap_used`, `expect_used`, `float_arithmetic`, and `lossy_float_literal` (`programs/Cargo.toml:52-61`).

Members under `programs/crates/` (`programs/Cargo.toml:4-9`):

| Crate | One line |
| --- | --- |
| `layerx-programs-runtime` | Deterministic WASM runtime foundation for LayerX guest programs (`programs/crates/layerx-programs-runtime/src/lib.rs:1`). |
| `layerx-programs-registry` | Receipt-bound registry: deployment journal, program value-account bindings, real-balance proofs, and wind-down/deprecation (`programs/README.md:48`). `src/lib.rs` has no crate-level documentation comment (`programs/crates/layerx-programs-registry/src/lib.rs:1`). |
| `layerx-programs-protocol-adapter` | Thin C↔Rust adapter exposing receipt-verified program state reads to the rest of the protocol (`programs/README.md:49`). `src/lib.rs` has no crate-level documentation comment (`programs/crates/layerx-programs-protocol-adapter/src/lib.rs:1`). |
| `layerx-programs-interpreter` | A bounded deterministic scripting program for the LayerX Programs ABI (`programs/crates/layerx-programs-interpreter/src/lib.rs:1`). |
| `layerx-programs-market` | Workspace member (`programs/Cargo.toml:7`). The crate `Cargo.toml` has no `description` (`programs/crates/layerx-programs-market/Cargo.toml:1-16`). `src/lib.rs` has no crate-level documentation comment (`programs/crates/layerx-programs-market/src/lib.rs:1`). |
| `layerx-programs-sandbox` | Protocol-state models for bounded, ephemeral program sandboxes (`programs/crates/layerx-programs-sandbox/src/lib.rs:1`). |

`programs/README.md` names three crates in its layout table (`programs/README.md:47-49`) and a section titled "The three crates" (`programs/README.md:57-89`). The workspace member list also includes `layerx-programs-interpreter`, `layerx-programs-market`, and `layerx-programs-sandbox` (`programs/Cargo.toml:6-9`).

## Frozen ABI

`programs/abi-frozen.sha256` stores SHA-256 checksums for frozen ABI vector files. Comment lines start with `#` (`programs/abi-frozen.sha256:1-2`, `programs/tools/generate-abi-vectors.py:94-95`). The pinned rows are (`programs/abi-frozen.sha256:3-4`):

```
1 09fcad46aeea9659d7d555a4a09ec151bd36d60b85bfd052d1f7a43cf71d58a3
2 8827869bf1324c3e82c60baf7360b1c3e2baa84d8591e49eb6b73c2cea6c8b69
```

Those digests are compared against `hashlib.sha256(path.read_bytes()).hexdigest()` of `programs/tests/vectors/abi-v{version}.hex` (`programs/tools/generate-abi-vectors.py:11-12`, `programs/tools/generate-abi-vectors.py:93-107`). The file comment states it is reviewed and updated separately only when a newly allocated ABI version is frozen (`programs/abi-frozen.sha256:1-2`).

`make programs-abi-drift` runs `programs/tools/check-abi-drift.sh`, then `cargo test --locked -p layerx-programs-runtime --test abi_linker` inside `programs/` (`Makefile:2887-2889`). `check-abi-drift.sh` execs `python3 programs/tools/generate-abi-vectors.py --check` (`programs/tools/check-abi-drift.sh:1-4`).

### `generate-abi-vectors.py`

The generator audits canonical sources under `programs/crates/layerx-programs-runtime/src` (`abi/manifest.rs`, `abi/mod.rs`, `lib.rs`) and `programs/sdk/rust/src/abi.rs` (`programs/tools/generate-abi-vectors.py:5-12`). It requires crate-root `ABI_VERSION` to equal `2` (`programs/tools/generate-abi-vectors.py:45-46`). It rebuilds the v1 and v2 manifests and fails (stderr `ABI surface drift: …`, return `1`) if the v1 table and v1 manifest diverge, if the v1+v2 tables and the composite v2 manifest diverge, if v2 function types and signatures diverge, if the validator allowlist is not derived from the frozen table, or if the Rust SDK v2 manifest or table diverges (`programs/tools/generate-abi-vectors.py:42-84`, `programs/tools/generate-abi-vectors.py:90-92`).

For each audited version it forms `generated` as the hex encoding of a 2-byte big-endian version prefix plus the manifest bytes, plus a trailing newline, destined for `programs/tests/vectors/abi-v{version}.hex` (`programs/tools/generate-abi-vectors.py:98-100`).

Write and check behaviour (`programs/tools/generate-abi-vectors.py:101-114`):

- Frozen version, missing vector file: stderr `frozen ABI v{version} vector is missing and cannot be recreated`; return `1`. The generator does not create that file.
- Frozen version, checksum mismatch against `abi-frozen.sha256`: stderr `frozen ABI v{version} checksum differs from independent baseline`; return `1`.
- Frozen version, file bytes differ from `generated`: stderr `immutable ABI v{version} surface drift; allocate a new ABI version`; return `1`.
- Unfrozen version whose vector file already exists: stderr `unfrozen ABI v{version} vector exists; review and add its checksum baseline`; return `1`.
- Unfrozen version, `--check`, no vector file: stderr `new ABI v{version} has no generated vector`; return `1`.
- Unfrozen version, no `--check`, no vector file: `path.write_text(generated)` writes `programs/tests/vectors/abi-v{version}.hex`.

`--check` is a boolean flag (`programs/tools/generate-abi-vectors.py:88-89`). `check-abi-drift.sh` passes `--check`, so the Makefile drift gate does not write vectors (`programs/tools/check-abi-drift.sh:4`).

## Dependency policy

`make programs-lint` depends on `programs-module-boundaries`, then runs Clippy, `sh programs/tools/dependency-policy.sh`, `cargo deny check advisories sources`, and `cargo deny check bans --exclude-dev` (`Makefile:2878-2882`). `programs/deny.toml` is the cargo-deny config the last two commands consume (`programs/deny.toml:1-57`). The shell script requires that file to be readable before it prints success (`programs/tools/dependency-policy.sh:163-164`).

### Banned crates (graph-wide, including dev)

`dependency-policy.sh` runs `cargo metadata --manifest-path programs/Cargo.toml --locked --format-version 1` (`programs/tools/dependency-policy.sh:11-12`) and matches every `.packages[].name` against (`programs/tools/dependency-policy.sh:14-18`):

`bindgen`, `libsqlite3-sys`, `rusqlite`, `sqlx-sqlite`, `ctor`, `inventory`, `getrandom`, `rand`, `chrono`, `time`, `instant`, `tokio`, `mio`, `socket2`, `wasi`, `wasmtime`.

The name check reads every `.packages[].name` and does not filter on dependency kind (`programs/tools/dependency-policy.sh:15`).

`programs/deny.toml` `[bans].deny` lists the same sixteen crate names (`programs/deny.toml:32-49`). `[graph] all-features = true` (`programs/deny.toml:1-2`). The Makefile bans check passes `--exclude-dev` (`Makefile:2882`), so cargo-deny bans omit dev-only packages. The shell script does not omit them.

`rand_core` is not in the banned-name list. If any `rand_core` node in `resolve.nodes` enables feature `getrandom` or `std`, the script prints `programs dependency policy: rand_core entropy features are forbidden` and exits `1` (`programs/tools/dependency-policy.sh:19-28`).

### Exact exit behaviour (`dependency-policy.sh`)

The script uses `set -eu` (`programs/tools/dependency-policy.sh:2`). Failures write one line to stderr and `exit 1`:

| Condition | Stderr | Lines |
| --- | --- | --- |
| Banned package name in metadata | `programs dependency policy: forbidden boundary, clock, randomness or network crate` | `programs/tools/dependency-policy.sh:15-18` |
| `rand_core` with `getrandom` or `std` | `programs dependency policy: rand_core entropy features are forbidden` | `programs/tools/dependency-policy.sh:19-28` |
| Sourced package with empty license | `programs dependency policy: $package has no SPDX license` | `programs/tools/dependency-policy.sh:44-47` |
| License tokens outside the script allowlist | `programs dependency policy: $package uses non-allowlisted license $license` | `programs/tools/dependency-policy.sh:48-79` |
| Sourced package/version lacking a vendored `Cargo.toml` plus `.cargo-checksum.json` | `programs vendoring policy: $package $version is not vendored with a checksum` | `programs/tools/dependency-policy.sh:106-114` |
| Workspace `wasmi` pin is not `path = "vendor/wasmi-0.31.2"` and `version = "=0.31.2"` | `programs vendoring policy: the WASM engine must stay pinned to an exact revision` | `programs/tools/dependency-policy.sh:116-125` |
| `programs/.cargo/config.toml` lacks `replace-with = "vendored-sources"` | `programs vendoring policy: builds must resolve the engine from programs/vendor` | `programs/tools/dependency-policy.sh:126-130` |
| `unsafe fn` / `unsafe trait` / `unsafe impl` / `unsafe extern` / `unsafe {` under `programs/crates`, except `layerx-programs-runtime/src/ffi*.rs`, `layerx-programs-sandbox/src/host_ffi.rs`, and `layerx-programs-protocol-adapter/src/ffi.rs` | `programs unsafe policy: unsafe code is forbidden` | `programs/tools/dependency-policy.sh:132-150` |
| `f32` or `f64` under `programs/crates` | `programs integer-only policy: floating-point types are forbidden in consensus-adjacent code` | `programs/tools/dependency-policy.sh:152-161` |

On success it prints `programs dependency, vendoring, unsafe and integer-only policies passed` (`programs/tools/dependency-policy.sh:164`).

The script license allowlist tokens are `Apache-2.0`, `BSD-1-Clause`, `BSD-2-Clause`, `BSD-3-Clause`, `CC0-1.0`, `ISC`, `MIT`, `Unicode-3.0`, `Zlib`, `LLVM-exception` (`programs/tools/dependency-policy.sh:30-38`). `programs/deny.toml` `[licenses].allow` lists `Apache-2.0`, `Apache-2.0 WITH LLVM-exception`, `BSD-2-Clause`, `BSD-3-Clause`, `CC0-1.0`, `ISC`, `MIT`, `Unicode-3.0`, `Zlib` (`programs/deny.toml:15-25`). `BSD-1-Clause` is in the script allowlist and absent from `deny.toml`.

## Module-boundary check

`make programs-module-boundaries` runs `sh programs/tools/runtime-module-boundaries.sh` (`Makefile:2884-2885`). With no argument the script checks `programs/crates/layerx-programs-runtime/src` (`programs/tools/runtime-module-boundaries.sh:4`). After `check_root` it re-executes itself with `--self-test` (`programs/tools/runtime-module-boundaries.sh:223-224`).

`check_root` fails (non-zero return, stderr as below) when (`programs/tools/runtime-module-boundaries.sh:43-137`):

- A required file is missing: `budget.rs`; `abi/{mod,balance,capability,codec,context,event_tests,host_state,manifest,response,storage_ops}.rs`; `host/{mod,balance,context,memory,storage,events,calls,transfer,scan,crypto,signature}.rs` (`programs/tools/runtime-module-boundaries.sh:47-58`). Stderr: `runtime module boundary: missing $path`.
- Legacy `abi.rs` or `host.rs` exists (`programs/tools/runtime-module-boundaries.sh:60-66`). Stderr: `runtime module boundary: legacy $legacy remains`.
- Any `ffi*.rs` or `lifecycle.rs` matches `_for_qualification` (`programs/tools/runtime-module-boundaries.sh:68-75`). Stderr: `runtime module boundary: production transition reaches qualification-only API`.
- The `abi/` `*.rs` basename set is not exactly `balance.rs capability.rs codec.rs context.rs event_tests.rs host_state.rs manifest.rs mod.rs response.rs storage_ops.rs` (`programs/tools/runtime-module-boundaries.sh:77-89`). Stderr: `runtime module boundary: unexpected ABI module inventory`.
- The `host/` `*.rs` basename set is not exactly `balance.rs calls.rs context.rs crypto.rs events.rs memory.rs mod.rs scan.rs signature.rs storage.rs transfer.rs` (`programs/tools/runtime-module-boundaries.sh:90-103`). Stderr: `runtime module boundary: unexpected host module inventory`.
- Host families `storage`, `events`, `calls`, `transfer`, `scan`, `crypto`, `signature` import a sibling family, name or alias a forbidden parent (`crate::host`, `use crate as`, `use super as`, grouped `self as`, `extern crate self as`), or mention `Abi` / `Composition` / `Storage` / `Meter` / `RuntimeState` as code tokens (`programs/tools/runtime-module-boundaries.sh:104-135`). Stderr: `runtime module boundary: $family imports a sibling host family`, `runtime module boundary: $family names or aliases a forbidden parent`, or `runtime module boundary: $family reaches state outside RuntimeState`.

`--self-test` builds a temporary layout, asserts the valid layout is accepted, and asserts missing `budget.rs`, qualification-API leakage, sibling imports, parent aliases, and direct state access are rejected (`programs/tools/runtime-module-boundaries.sh:140-220`).

## Fixture recipes

`PROGRAMS_CARGO` defaults to `cargo` (`Makefile:2861`). Capability recipes run the `layerx-programs-runtime` example `capability_fixture` (`Makefile:2893-2901`). Lifecycle and executed recipes depend on `$(BUILD_DIR)/tests/programs_call_activity`, built from `tests/programs/test_call_activity.c` plus `$(LIBRARY)` and `$(PROGRAMS_RUNTIME_LIB)` after `programs-build` (`Makefile:2921-2925`, `Makefile:2872-2876`).

| Recipe | What it does | Files |
| --- | --- | --- |
| `programs-generate-capability-fixture` | Runs `cargo run --locked -p layerx-programs-runtime --example capability_fixture` in `programs/`, copies stdout onto the fixture path (`Makefile:2893-2896`). | Writes `platform/sdk/conformance/fixtures/native-program-capabilities-v2.json` (`Makefile:2896`). |
| `programs-check-capability-fixture` | Same generator, then `cmp` against the committed fixture (`Makefile:2898-2901`). | Reads `platform/sdk/conformance/fixtures/native-program-capabilities-v2.json` (`Makefile:2901`). |
| `programs-native-lifecycle-fixtures` | Runs `python3 platform/sdk/conformance/fixtures/generate_native_lifecycle_fixtures.py --encoder $<` (`Makefile:2928-2929`). `$<` is `$(BUILD_DIR)/tests/programs_call_activity`. The encoder is invoked as `--dump-native-lifecycle` (`platform/sdk/conformance/fixtures/generate_native_lifecycle_fixtures.py:12-16`). | Writes, next to the generator, `native-program-deploy-v3.json`, `native-program-upgrade-v3.json`, `native-program-wind-down-route-v3.json`, `native-program-wind-down-deprecate-v3.json`, `native-program-wind-down-tombstone-v3.json`, `native-program-wind-down-exit-v3.json` (`platform/sdk/conformance/fixtures/generate_native_lifecycle_fixtures.py:18-48`). |
| `programs-check-native-lifecycle-fixtures` | Same generator with `--check` (`Makefile:2931-2932`). | Reads those six JSON files and compares them to encoder output (`platform/sdk/conformance/fixtures/generate_native_lifecycle_fixtures.py:44-46`). |
| `programs-executed-fixture` | Runs `python3 platform/sdk/conformance/fixtures/generate_executed_program_fixture.py --encoder $<` (`Makefile:2935-2936`). The encoder is invoked as `--dump-executed-v3` (`platform/sdk/conformance/fixtures/generate_executed_program_fixture.py:131-132`). | Writes `platform/sdk/conformance/fixtures/receipt-programs-executed-v3.json` (`platform/sdk/conformance/fixtures/generate_executed_program_fixture.py:137-143`). |
| `programs-check-executed-fixture` | Same generator with `--check` (`Makefile:2938-2939`). | Reads `platform/sdk/conformance/fixtures/receipt-programs-executed-v3.json` (`platform/sdk/conformance/fixtures/generate_executed_program_fixture.py:138-141`). |

## Test tiers

`PROGRAMS_RUNTIME_LIB` is `programs/target/debug/liblayerx_programs_sandbox.a` (`Makefile:2862`). `programs-build` is `cd programs && $(PROGRAMS_CARGO) build --locked --workspace --features layerx-programs-sandbox/host-ffi` (`Makefile:2875-2876`). Core C binaries list `| programs-build` as an order-only prerequisite (`Makefile:2903-2925`, `Makefile:2941-2969`).

| Recipe | Command | Prerequisites |
| --- | --- | --- |
| `programs-core-test` | `$(RUN_PREFIX)` on `$(BUILD_DIR)/tests/programs_{registration,lifecycle,monetary_law,call_activity,occupancy_batch,metering_schedule,fee_governance,accounts,winddown}` (`Makefile:2980-2988`). | Those nine binaries (`Makefile:2971-2979`). Each binary is compiled from the matching `tests/programs/test_*.c` against `$(LIBRARY)` or `$(TEST_LIBRARY)` and `$(PROGRAMS_RUNTIME_LIB)` (`Makefile:2903-2925`, `Makefile:2941-2969`). |
| `programs-protocol-regression` | No recipe body (`Makefile:2990-2991`). Make runs the prerequisite targets. | `test-kernel`, `test-module-ctx`, `test-dispatch`, `test-receipts`, `test-state-root`, `test-snapshot`, `test-replay-golden-local` (`Makefile:2990-2991`). |
| `programs-fuzz-smoke` | In `programs/`: `$(PROGRAMS_CARGO) run --locked -p layerx-programs-fuzz --bin programs-fuzz --` for `validation fuzz/corpus/validation`, `instantiation fuzz/corpus/instantiation`, and `execution fuzz/corpus/execution` (`Makefile:2993-2996`). | None declared on the target (`Makefile:2993`). |
| `programs-adversarial` | In `programs/`: `$(PROGRAMS_CARGO) test --locked -p layerx-programs-runtime --test isolation --test composition --test monetary_law` (`Makefile:2998-2999`). | None declared on the target (`Makefile:2998`). |
| `programs-conservation` | `programs/tests/conservation/run.sh` (`Makefile:3001-3002`). | None declared on the target (`Makefile:3001`). |
| `programs-qualify` | `python3 tools/qualification/release_runner.py $@` (`Makefile:3004-3005`). `$@` is the target name `programs-qualify`. | None declared on the target (`Makefile:3004`). |
| `programs-differential` | In `programs/`: `$(PROGRAMS_CARGO) test --locked -p layerx-programs-runtime --test replay --test determinism`, then `$(RUN_PREFIX) $(BUILD_DIR)/tests/programs_parallel_differential` (`Makefile:3017-3019`). | `$(BUILD_DIR)/tests/programs_parallel_differential`, compiled from `programs/tests/differential/parallel.c`, `tests/programs/test_call_activity.c`, `cmd/layerxd/lxp_daemon_batch_wal.c`, `$(LIBRARY)`, and `$(PROGRAMS_RUNTIME_LIB)` with `| programs-build` (`Makefile:3008-3015`). |
| `programs-interpreter-conformance` | In `programs/`: `$(PROGRAMS_CARGO) build --locked --release --target wasm32-unknown-unknown -p layerx-programs-interpreter`, then `LAYERX_INTERPRETER_WASM=$(pwd)/target/wasm32-unknown-unknown/release/layerx_programs_interpreter.wasm $(PROGRAMS_CARGO) test --locked -p layerx-programs-runtime --test interpreter_program` (`Makefile:3021-3023`). | None declared on the target (`Makefile:3021`). |
| `programs-bench` | No recipe body; Make runs `programs-interpreter-bench` (`Makefile:3025`). That target builds release `wasm32-unknown-unknown` packages `layerx-programs-interpreter` and `layerx-interpreter-compiled-equivalent`, then `$(PROGRAMS_CARGO) bench --locked -p layerx-programs-runtime --bench interpreter` with `LAYERX_INTERPRETER_WASM` and `LAYERX_COMPILED_EQUIVALENT_WASM` set to those two `.wasm` paths (`Makefile:3027-3030`). | `programs-interpreter-bench` (`Makefile:3025`). |
