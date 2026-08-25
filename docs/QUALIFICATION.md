# Qualification

LayerX qualification is evidence layered by risk. A lower layer never implies a
higher one, and no local command authorizes deployment or handling real assets.

## Evidence levels

| Level | Evidence | Representative gate |
|---|---|---|
| Source integrity | Generated specifications are current; no local artifacts, credentials, or private infrastructure references enter the publication set | `make public-audit` |
| Build and unit behavior | Strict C17 compilation, native tests, Solidity compilation, contract tests, and invariant suites | `make build`, `make test`, `make test-contracts` |
| Runtime safety | Address, undefined-behaviour, thread, memory, and leak instrumentation on applicable suites; consensus symbol and floating-point exclusion | `make ci`, `make qualify-arith` |
| Deterministic replay | Published digest reproduced across optimization levels, compilers, libc implementations, and architectures; mutations fail at the exact divergent sequence | `make qualify-replay` |
| Fault and adversarial behavior | Write-boundary process aborts, recovery, malformed activity/signature/transfer fuzzing, DA withholding, guarantor disagreement, and reserve reconciliation | `make qualify-faults`, `make qualify-fuzz`, settlement gates |
| Deployment evidence | Independently reviewed bytecode, deterministic deployment manifests, live custody reconciliation, sequencer-offline exit drill, and a bounded real-value canary | Owner-gated runbook; never automatic |

The authoritative acceptance criteria and current completion state are in the
[task board](../spec/layerx-protocol/tasks.md).

## Standard suites

```sh
make public-audit
make build
make test
make test-contracts
make ci
```

`make ci` performs the native suite, a reproducible two-build archive comparison,
consensus symbol checks, and ASan, UBSan, and TSan runs. ThreadSanitizer requires
a host that permits the address-space layout used by the compiler runtime.

## Replay qualification

```sh
make qualify-replay
```

The replay matrix requires GCC 13, Clang 18, Docker, an amd64 musl runner, and
an AArch64 cross-compiler plus QEMU runner. It generates a 10-million-activity
corpus and root ledger under `build/qualification/replay/`; these multi-gigabyte
artifacts are deliberately excluded from version control. Only the compact
published digest belongs in `tests/vectors/qualification_replay_10m.digest`.

## Arithmetic qualification

```sh
make qualify-arith
```

This gate combines exhaustive boundary cases, checked 128-bit and 256-bit
operations, rounding-direction and conservation assertions, CBMC/Z3 proof
obligations, consensus floating-point and libm exclusion, and the configured
sanitizer runners. It requires CBMC, Z3, and the compiler runtimes selected by
`tools/lxp_arith_proof.sh`.

## Fault and fuzz qualification

```sh
make qualify-faults
make qualify-fuzz
```

Fault qualification uses real process interruption at durable write boundaries
and verifies restart behavior. Fuzz qualification seeds malformed activity,
signature, and transfer-set mutations from the generated replay corpus. It must
fail closed; a crash, sanitizer finding, non-canonical acceptance, or partially
committed transition is a release blocker.

## Settlement and deployment gates

Settlement qualification must cover guarantor refusal, divergent replay,
equivocation evidence, every data-availability class, exact per-asset reserve
reconciliation, historical debit/credit conservation, and a sequencer-offline
emergency exit from the last finalized checkpoint.

Live contract deployment, validator mutation, custody migration, or a real-value
canary requires an explicit owner-approved runbook. Repository tests may prepare
that evidence but cannot satisfy or authorize the live gate by themselves.

## Paxeer qualification

Paxeer Network has its own qualification gates under `paxeer-network/`:

```sh
make paxeer-ci
make monorepo-ci
```

Paxeer evidence is a separate gate from LayerX qualification. A passing Paxeer build does not imply LayerX qualification, and vice versa. Each subsystem qualifies independently against its own specification, and co-location in this monorepo grants neither system new protocol or deployment authority over the other.

