# Programs workspace gates

The `programs/` tree is a Cargo workspace for the LayerX Network programs surface: a deterministic WASM runtime, the registry that proves what a program deployed and what it holds, the C↔Rust bridge into the protocol kernel, developer SDKs, porting kits, and the adversarial test corpus (`programs/README.md:5-7`). Guest execution is dispatched under kernel module ID `9` (`LXP_MODULE_PROGRAMS`); a program is not a ninth economic module and writes no balances of its own (`programs/README.md:9-17`).

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

`programs/README.md` names three crates in its layout table (`programs/README.md:47-49`) and a section titled "The three crates" (`programs/README.md:57-89`). The workspace member list also includes `layerx-programs-interpreter`, `layerx-programs-market`, and `layerx-programs-sandbox` (`programs/Cargo.toml:6-9`). `porting/cosmwasm`, `porting/cosmwasm/guest`, `porting/evm`, `porting/evm/guest`, `porting/solana`, and `porting/solana/guest` are workspace members (`programs/Cargo.toml:11-16`); see [Porting](Porting.md).

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

`make programs-lint` depends on `programs-module-boundaries`, then runs Clippy,
`sh programs/tools/dependency-policy.sh`,
`cd programs && cargo deny check advisories sources`, and
`cd programs && cargo deny --exclude-dev check bans`
(`Makefile:2970-2974`). `programs/deny.toml` is the cargo-deny config the last
two commands consume (`programs/deny.toml:1-57`). The shell script requires
that file to be readable before it prints success
(`programs/tools/dependency-policy.sh:163-164`).

### Banned crates (graph-wide, including dev)

`dependency-policy.sh` runs `cargo metadata --manifest-path programs/Cargo.toml --locked --format-version 1` (`programs/tools/dependency-policy.sh:11-12`) and matches every `.packages[].name` against (`programs/tools/dependency-policy.sh:14-18`):

`bindgen`, `libsqlite3-sys`, `rusqlite`, `sqlx-sqlite`, `ctor`, `inventory`, `getrandom`, `rand`, `chrono`, `time`, `instant`, `tokio`, `mio`, `socket2`, `wasi`, `wasmtime`.

The name check reads every `.packages[].name` and does not filter on dependency kind (`programs/tools/dependency-policy.sh:15`).

`programs/deny.toml` `[bans].deny` lists the same sixteen crate names (`programs/deny.toml:32-49`). `[graph] all-features = true` (`programs/deny.toml:1-2`). The Makefile bans check passes `--exclude-dev` before `check bans` (`Makefile:2974`), so cargo-deny bans omit dev-only packages. The shell script does not omit them.

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

## Signature and storage test authorities

`signatures/sources.json` pins four fetched files by `file`, `url`, and `sha256` (`signatures/sources.json:1-32`):

| File | URL | SHA-256 |
| --- | --- | --- |
| `rfc8032.txt` | `https://www.rfc-editor.org/rfc/rfc8032.txt` (`signatures/sources.json:3-4`) | `ed63657ff389301282b169b0abde9b5dd2c7e4d524fdfa5da6ff3094fc93c4c3` (`signatures/sources.json:5`) |
| `wycheproof.json` | `https://raw.githubusercontent.com/C2SP/wycheproof/main/testvectors_v1/ecdsa_secp256k1_sha256_test.json` (`signatures/sources.json:8-9`) | `43db761c0a2eae71fb0755d355d5130e28ce64a5b07846cf27e7072082597a81` (`signatures/sources.json:10`) |
| `secp256k1.h` | `https://raw.githubusercontent.com/bitcoin-core/secp256k1/v0.6.0/include/secp256k1.h` (`signatures/sources.json:23-24`) | `0c3bbf04703f9a240a200a57fb75b3d7ef072464f24369d34bd9fc0848a66f3b` (`signatures/sources.json:25`) |
| `secp256k1_recovery.h` | `https://raw.githubusercontent.com/bitcoin-core/secp256k1/v0.6.0/include/secp256k1_recovery.h` (`signatures/sources.json:28-29`) | `9ef005af267b04b2f4def6e7d3bc7a8ad42dad80063f939aab8eb46317c6212f` (`signatures/sources.json:30`) |

The Wycheproof pin lists `cases`: `tcId` 3 purpose `Valid prehashed signature, compressed and uncompressed SEC1 key acceptance` (`signatures/sources.json:12-15`); `tcId` 1 purpose `Existing genuine high-S refusal in signature_authority.rs` (`signatures/sources.json:16-19`).

`signatures/check.py` iterates those pins and asserts `hashlib.sha256((root / source['file']).read_bytes()).hexdigest() == source['sha256']` (`signatures/check.py:11-12`). It prints `All authority source SHA256 values match` and `backend.openssl_version_text()` from the OpenSSL cryptography backend (`signatures/check.py:5`, `signatures/check.py:13-14`).

It reads `programs/crates/layerx-programs-runtime/tests/signature_vectors.rs` (`signatures/check.py:15`). From `wycheproof.json` it takes `testGroups[0]`, the test with `tcId == 3`, and asserts `result == 'valid'` (`signatures/check.py:16-18`). It decodes the DER `sig` hex, splits `(r, s)` with `utils.decode_dss_signature`, recomputes `digest` as SHA-256 of the `msg` hex, loads `group['publicKey']['uncompressed']` as a `SECP256K1` point, and forms 32-byte big-endian `r || s` (`signatures/check.py:19-23`). For `secp256k1_verify_accepts_compressed_public_key`, `secp256k1_verify_accepts_uncompressed_public_key`, and `secp256k1_verify_published_test_vector_1` it extracts every `hex::decode` hex string from the function body and asserts those bytes equal `[digest, key.public_bytes(X962, CompressedPoint|UncompressedPoint), raw]` (`signatures/check.py:24-28`). It OpenSSL-verifies the original DER against that digest with `ECDSA(Prehashed(SHA256))` (`signatures/check.py:29-30`).

For `(r, s)` in `(0, 0)` and `(0, 255)` it encodes DSS signatures and asserts OpenSSL raises `InvalidSignature` on the same tcId 3 key and digest (`signatures/check.py:31-37`). For Wycheproof `tcId == 1` it decodes DER `s`, asserts `s > 0xfffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141 // 2`, and OpenSSL-verifies the high-S DER (`signatures/check.py:38-43`).

For `malleable_signature_detection_ed25519` it decodes the three `hex::decode` values as `pk`, `sig`, and `order` (`signatures/check.py:44-45`). It strips whitespace from `rfc8032.txt` and asserts `pk.hex()` and `sig.hex()` both occur in that text (`signatures/check.py:46-47`). It asserts little-endian `order` equals `L = 2**252 + 27742317777372353535851937790883648493` (`signatures/check.py:48-49`). OpenSSL Ed25519 verifies `sig` over `b''` and rejects `S + L` (`signatures/check.py:50-58`). Independently it sets `p = 2**255 - 19`, `y` from 32 bytes of `0x02` little-endian, curve `d = -121665 * pow(121666, -1, p) % p`, and `x2 = (y*y - 1) * pow(d*y*y + 1, -1, p) % p`; it asserts `pow(x2, (p-1)//2, p) == p-1` (`signatures/check.py:59-64`).

It asserts `secp256k1.h` contains `R and S with value 0 are allowed in the encoding.` (`signatures/check.py:65`, `signatures/secp256k1.h:474`). It asserts `5 + 3 * 8 == 29`, `5 + 2 * 8 + 80 == 101`, and `5 + 8 == 13` (`signatures/check.py:67`).

`secp256k1_verify_accepts_compressed_public_key` and `secp256k1_verify_accepts_uncompressed_public_key` first call `verify_secp256k1` with all-zero digest, all-zero public key, and all-zero `SECP256K1_SIGNATURE_BYTES` signature and assert `Err(SignatureRefusal::MalformedSignature)` (`programs/crates/layerx-programs-runtime/tests/signature_vectors.rs:141-149`, `programs/crates/layerx-programs-runtime/tests/signature_vectors.rs:161-169`). `secp256k1_verify_zero_vector_fails` asserts the same compressed zero triple (`programs/crates/layerx-programs-runtime/tests/signature_vectors.rs:180-188`). `secp256k1_recover_zero_vector_fails` asserts `recover_secp256k1` of all-zero digest and signature returns `MalformedSignature` for every `recovery_id` in `0..=3` (`programs/crates/layerx-programs-runtime/tests/signature_vectors.rs:229-238`). The pinned compact parser allows R and S of value 0 and, when R or S are zero, guarantees verification failure for any message and public key (`signatures/secp256k1.h:472-478`). OpenSSL rejects `(r, s) = (0, 0)` (`signatures/check.py:31-37`). `malleable_signature_detection_secp256k1` uses an all-zero `SECP256K1_SIGNATURE_BYTES` signature with `signature[63] = 0xff` and asserts `MalformedSignature` (`programs/crates/layerx-programs-runtime/tests/signature_vectors.rs:338-348`).

`malleable_signature_detection_ed25519` calls `verify_ed25519` with `public_key = [2u8; ED25519_PUBLIC_KEY_BYTES]` and asserts `Err(SignatureRefusal::MalformedPublicKey)` (`programs/crates/layerx-programs-runtime/tests/signature_vectors.rs:304-314`). `check.py` treats 32 bytes of `0x02` as a quadratic nonresidue (`signatures/check.py:59-64`). RFC 8032 5.1.3 decoding fails when no square root exists (`signatures/rfc8032.txt:596-597`). 5.1.7 splits the signature, decodes `R`, `S`, and public key `A`, and treats any failed decoding as an invalid signature (`signatures/rfc8032.txt:750-754`). The same function then uses RFC 8032 TEST 1 (`programs/crates/layerx-programs-runtime/tests/signature_vectors.rs:316`, `signatures/rfc8032.txt:1301-1320`): `verify_ed25519` of the empty message returns `Ok(())`; after adding little-endian `L` into `S` with carry 0 it returns `Err(SignatureRefusal::VerificationFailed)` (`programs/crates/layerx-programs-runtime/tests/signature_vectors.rs:317-334`).

`wycheproof_secp256k1_sha256_tc1_rejects_high_s` asserts `verify_secp256k1` of the tcId 1 digest, uncompressed key, and compact signature returns `Err(SignatureRefusal::VerificationFailed)` (`programs/crates/layerx-programs-runtime/tests/signature_authority.rs:13-20`). Wycheproof records that vector as `result: valid` with flag `ValidSignature` (`signatures/wycheproof.json:137-144`). `check.py` asserts the decoded `s` is above `n/2` and that OpenSSL accepts the DER (`signatures/check.py:38-43`). The pin purpose names that refusal (`signatures/sources.json:16-19`).

Comments on `secp256k1_verify_accepts_compressed_public_key`, `secp256k1_verify_accepts_uncompressed_public_key`, and `secp256k1_verify_published_test_vector_1` cite `Wycheproof ecdsa_secp256k1_sha256 tcId 3; signatures/sources.json` (`programs/crates/layerx-programs-runtime/tests/signature_vectors.rs:151`, `programs/crates/layerx-programs-runtime/tests/signature_vectors.rs:171`, `programs/crates/layerx-programs-runtime/tests/signature_vectors.rs:269`). Each asserts `verify_secp256k1(...) == Ok(())` for prehash `bb5a52f42f9c9261ed4361f59422a1e30036e7c32b270c8807a419feca605023`, compact `r || s`, and either the compressed or uncompressed SEC1 key (`programs/crates/layerx-programs-runtime/tests/signature_vectors.rs:152-157`, `programs/crates/layerx-programs-runtime/tests/signature_vectors.rs:172-176`, `programs/crates/layerx-programs-runtime/tests/signature_vectors.rs:270-274`). Wycheproof records `tcId` 3 as `result: valid` (`signatures/wycheproof.json:157-164`). The pin purpose is compressed and uncompressed SEC1 acceptance (`signatures/sources.json:12-15`). `check.py` matches those test bytes to the recomputed digest, encodings, and `r || s`, then OpenSSL-verifies the DER (`signatures/check.py:16-30`).

`scan_guest_selected` places prefix at offset 0, cursor at 32, and `keep` at 256 (`programs/crates/layerx-programs-runtime/tests/storage_scan.rs:201`). `scan_status_guest` places `keep` at 128 (`programs/crates/layerx-programs-runtime/tests/storage_scan.rs:260`) and stores the scan status i32 at memory 64 and the i32 at `sentinel_pointer` at memory 68, then `response_write`s 8 bytes from 64 (`programs/crates/layerx-programs-runtime/tests/storage_scan.rs:240-249`). `candidate_scan_paginates_across_activities_and_is_insertion_order_independent` overwrites `cursor[64 - 32..68 - 32]` with `b"keep"` (`programs/crates/layerx-programs-runtime/tests/storage_scan.rs:414-415`), executes `scan_status_guest` with that cursor and sentinel 128 (`programs/crates/layerx-programs-runtime/tests/storage_scan.rs:417-424`), and asserts `storage_read_bytes == 0`, response bytes `(-2_i32).to_le_bytes()` followed by `b"keep"`, and storage equal to the pre-call clone (`programs/crates/layerx-programs-runtime/tests/storage_scan.rs:425-429`). The uncorrupted cursor then resumes as a 13-byte page (`programs/crates/layerx-programs-runtime/tests/storage_scan.rs:430-445`). `check.py` records that guest cursor starts at 32 so a memory overwrite of `64..68` maps to `cursor[32..36]` (`signatures/check.py:69`).

`expected_page` encodes a big-endian u16 count, per entry a big-endian u16 key length plus key plus big-endian u32 value length plus value, a has-cursor byte, a big-endian u16 cursor length, and cursor bytes (`programs/crates/layerx-programs-runtime/tests/storage_scan.rs:283-296`). Three `(a, a) / (b, b) / (c, c)` entries with `None` cursor are 29 bytes (`programs/crates/layerx-programs-runtime/tests/storage_scan.rs:466-467`). The scan with `max_entries` 64, `max_bytes` 101, and 29-byte response capacity returns those 29 bytes and meters `storage_read_bytes` 29 (`programs/crates/layerx-programs-runtime/tests/storage_scan.rs:468-477`). After seeding `(d, d)` (`programs/crates/layerx-programs-runtime/tests/storage_scan.rs:478-479`), two entries plus `expected_cursor(owner, actor, 64, 101, b"b")` are 101 bytes (`programs/crates/layerx-programs-runtime/tests/storage_scan.rs:481-483`); the same 64/101 limits return that 101-byte page and meter 101 (`programs/crates/layerx-programs-runtime/tests/storage_scan.rs:484-499`). `check.py` asserts the independent sizes 29, 101, and 13 (`signatures/check.py:67-68`).

`write_then_scan_guest(12, true)` pushes `0x00` after the scan (`programs/crates/layerx-programs-runtime/tests/storage_scan.rs:105-106`, `programs/crates/layerx-programs-runtime/tests/storage_scan.rs:724`). `execute_authorized_candidate` returns a record whose `outcome()` is `V2ActivityOutcome::Failure` (`programs/crates/layerx-programs-runtime/tests/storage_scan.rs:726-743`). The failure `class()` is `RefusalClass::RuntimeFault`, `program()` equals the owner, `response()` is `None`, and storage equals the pre-call clone (`programs/crates/layerx-programs-runtime/tests/storage_scan.rs:744-750`).

[Home](Home.md)
