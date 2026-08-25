# Contributing to LayerX

LayerX is security-critical accounting and settlement software. Contributions
must preserve deterministic replay, conservation of value, explicit authority,
and fail-closed behavior across both the C runtime and Solidity contracts.

## Before proposing a change

Read the [normative requirements](spec/layerx-protocol/requirements.md),
[design](spec/layerx-protocol/design.md), and
[threat model](spec/layerx-protocol/docs/threat-model.md). Search existing issues
before opening a new one. Do not use a public issue for a suspected
vulnerability; follow [SECURITY.md](SECURITY.md).

Protocol changes need an explicit requirement and task in
`spec/layerx-protocol/spec.kvx`. Generated files—including `AGENTS.md`, IDE rule
files, and the Markdown mirrors next to `spec.kvx`—must be regenerated from KVX
sources rather than edited directly.

## Task workflow

The repository uses Codify for one-at-a-time task ownership and implementation
traceability:

```sh
cg spec next
cg spec start <task-id>
cg context "change area"
cg impact <symbol> -d 2
```

Implement only the claimed task and its acceptance criteria. When all real
verification gates pass, finish with:

```sh
cg sync
cg changes
cg spec done <task-id>
cg spec trace <task-id>
```

Do not force a task to done to bypass a failed verification command or graph
check. Do not add fake implementations, placeholder behavior, or mocked
security boundaries to satisfy tests.

## Implementation rules

- Use C17 for the LayerX protocol runtime and Solidity `0.8.27` for LayerX settlement contracts.
- Paxeer Network is a separate Go module under `paxeer-network/` with its own Makefile and CI (`make paxeer-build`, `make paxeer-lint`, `make paxeer-test`, `make paxeer-ci`).
- Programs is a Rust workspace under `programs/` for the programmable LayerX runtime.
- Preserve canonical byte encodings and result-code assignments. Both are
  protocol interfaces, not implementation details.
- Use checked fixed-width integer arithmetic for every consensus-critical
  calculation. Floating point is prohibited in transition paths.
- Keep 402LXP as the only balance writer. Modules emit validated transfer sets.
- Keep network, wall-clock, filesystem enumeration, and database iteration order
  outside deterministic state transitions.
- Reject malformed or non-canonical input explicitly and transactionally.
- Add real regression tests, negative tests, and fuzz coverage for every parser
  or externally controlled boundary changed.
- Preserve unrelated workspace changes.

## Verification

Run the narrowest affected target while iterating, then the applicable broad
gates before review:

```sh
make public-audit
make build
make test
make test-contracts
```

Paxeer Network and the monorepo have separate optional gates that do not replace LayerX qualification:

```sh
make paxeer-ci       # Paxeer-specific evidence
make monorepo-ci     # Cross-subsystem integrity
```

Arithmetic, replay, recovery, settlement, and cross-architecture changes have
additional gates documented in [docs/QUALIFICATION.md](docs/QUALIFICATION.md).
Include the exact commands and outcomes in the pull request. A local result is
not a production certification, and no contribution authorizes deployment,
validator mutation, custody migration, or a real-value canary.

## Pull requests

Keep changes narrowly scoped and explain the threat or invariant they preserve.
Complete the pull-request checklist, identify the affected requirement and task,
and call out any verification that could not be run. Reviewers may require new
adversarial cases, proof obligations, replay vectors, or migration analysis when
a change crosses a trust boundary.

