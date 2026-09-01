## Engineering ground rules

These apply to **every** task in this plan. They are standing conditions for
reaching `done`, not per-task criteria to be negotiated.

- **Rust here, C17 there.** The interaction layer is Rust in the `agent/`
  workspace. The protocol runtime stays C17. No crate in `agent/` is a build
  dependency of `src/`, and no Rust translation unit links into the consensus
  runtime.
- **Never invent protocol state.** A mutation is canonical signed bytes submitted
  to the core, or it does not happen. A reported protocol fact is core-produced
  bytes whose evidence this layer verified, or it is labelled unverified. There
  is no third option, and no task may introduce one for convenience.
- **The boundary is the only way in.** No SQLite driver, no reading node log
  segments or projection files, no binding of `include/layerx/` struct layouts,
  no in-process shortcut to the core. `make agent-check-boundary` enforces this;
  a task that needs it suppressed is a task that needs a spec change.
- **Verification produces levels, not booleans.** A `VerificationLevel` is
  constructed only by the `layerx-proof` routine that performed the check, and
  travels with the evidence it rests on. No code path may raise a level, and a
  requested level that cannot be achieved is a refusal rather than a silent
  downgrade.
- **`Unknown` is never rounded.** A submission whose fate is undetermined stays
  `Unknown`, keeps its reservations held, and is resolved only by receipt lookup
  under its idempotency key. No task may add a heuristic that guesses the
  outcome, and none may report `Unknown` as success or failure.
- **Local controls only refuse.** Policy, capabilities and daemon-side budgets can
  narrow what an agent may do. They can never authorise anything, and no
  response, log line, metric or documentation string may describe a
  daemon-enforced restriction as a protocol guarantee.
- **Exact bytes, end to end.** The bytes signed are the bytes transmitted. Every
  signing call carries a disclosure decoded from those same bytes, and a
  disclosure that does not re-encode to them is a refusal to sign.
- **No secrets on any output surface.** Key material, session token values,
  unredacted secret configuration and out-of-retention payload contents never
  reach a log, metric, trace, error, panic payload or audit entry.
  `make agent-check-secrets` enforces it.
- **No panics from untrusted bytes.** Decoders, framing, the contract surface and
  the policy loader return typed errors. Each ships with a fuzz target in the same
  task that introduces it; a panic, hang or unbounded allocation is a defect, not
  a finding.
- **Unsafe is enumerated.** `unsafe` is denied workspace-wide except for blocks
  listed in `unsafe-allowlist.toml` with a written safety argument. The C ABI
  transport is the expected occupant; anything else needs justification in review.
- **Real dependencies in tests.** Boundary tests run against a real `layerxd`
  started from this repository. Differential tests run against the real C codec.
  Verification tests run against real corpora including the negative ones. A
  passing test whose subject was replaced by a fake is a false `done`.
- **Determinism where it is claimed.** Policy evaluation, capability checks and
  encoding are deterministic functions of their recorded inputs. No ambient clock,
  no randomness, no map-iteration-order dependence, no load-sensitive behaviour in
  any of them.
- **Every completed task is published.** When a task reaches `done`, commit and
  push from the repository root with a natural-sounding message describing what
  actually changed, before starting the next task. Development on this layer
  happens in the open so it can be audited step by step; a finished task that is
  not pushed is not finished, and a commit message must describe the real change
  rather than the intended one.

## Locked decisions

These supersede anything ambiguous in `docs/00-source-brief.md`.

- **Language and placement.** Rust 2021, its own workspace at `agent/`, built,
  tested and released independently of the C core, sharing the repository so spec
  and code stay in one Codify graph.
- **Boundary.** The LayerX Node Interface: versioned, canonical binary, Unix
  domain socket by default, mutual-TLS TCP for remote deployment. An optional
  stable C ABI transport is permitted with opaque handles and canonical byte
  buffers only, never shared struct layouts.
- **Signing posture.** `layerx-agentd` does not hold agent primary keys. External
  signing is the default. It signs directly only under a protocol session key an
  operator explicitly provisioned, whose scope and expiry the state machine
  enforces independently of the daemon.
- **Budgets prefer protocol objects.** Where the protocol offers an enforceable
  equivalent — capability grants, session key scopes, budgets, payer grants — the
  restriction is expressed there, so it still binds when the daemon is bypassed.
  Daemon-only limits are labelled as such with their bypass statement.
- **SDK generation.** The Rust SDK is authored; TypeScript and Python are
  generated from the same contract schema by the same generator. Hand-editing
  generated output fails the build.
- **Capability gaps are reported, not worked around.** If the node does not expose
  something this plan requires, the dependent request fails as `Unavailable` and
  the gap is reported for the protocol feature to close. Reconstructing the answer
  locally is the specific failure this layer exists to prevent.

## Verification commands

Every `verify_cmd` is a `make` target in the repository Makefile that delegates to
cargo inside `agent/`. The naming is mechanical, so a task and its gate are always
findable from each other:

| Prefix | Runs |
|---|---|
| `make agent-build` / `agent-lint` / `agent-check-*` | build, lint and the purity gates |
| `make agent-test-<crate>-<area>` | the suite for one crate and area |
| `make agent-test-agentd-<area>` | one daemon subsystem |
| `make agent-test-<surface>` | a cross-cutting gate (boundary, isolation, parity) |
| `make agent-fuzz-<target>` | a fuzz target with its committed corpus |
| `make agent-qualify-*` / `make agent-qualify` | the wave 12 release gates |

Adding a task means adding its target in the same change. A `verify_cmd` that does
not exist yet is not a plan, and a target that passes without exercising the task's
subject is worse than no target at all.
