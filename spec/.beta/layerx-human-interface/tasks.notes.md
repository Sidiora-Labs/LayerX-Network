## Engineering ground rules

These apply to **every** task in this plan. They are standing conditions for
reaching `done`, not per-task criteria to be negotiated.

- **Receipt-backed truth.** No surface renders success for a money movement or
  protocol mutation without a verified LayerX receipt or Paxeer finality proof.
  Unknown renders as still-checking with duplicate-capable controls locked, and
  resolves only by receipt lookup. No task may introduce a third option.
- **One write path.** Typed intent → `layerx-intents` → agent-layer prepare →
  disclosure checked against the intent → custody signature over exact bytes →
  agent-layer submit → receipt verification. The only agent-contract change is
  the additive `approval.*` module. No task adds another way to move money.
- **Keys stay in custody.** No key material in the browser, in logs, metrics,
  traces, errors, exports or notification payloads. No key export surface in
  v1. Signing without a byte-identical disclosure is a refusal. Step-up
  operations refuse without fresh, operation-bound passkey evidence.
- **The boundary is inherited.** No SQLite drivers, no node files, no
  `include/layerx/` layouts anywhere in `human/`. `layerx-intents` is the only
  component touching `layerx-wire` encoding. Gates enforce both; a task that
  needs one suppressed is a task that needs a spec change.
- **Five ideas, three decisions.** Default surfaces speak the plain-language
  vocabulary of the copy catalog — banned protocol terms fail the build.
  Common actions initiate in at most three decisions. The wallet opens only at
  explicit custody-boundary signing moments, exactly once per required
  signature.
- **Every screen ships its state matrix.** Loading, empty, error, offline,
  degraded, and still-checking where money is in flight — registered in the
  state-matrix manifest as the screen is built, not backfilled at
  qualification. Every error state has actionable buttons and a trace
  identifier.
- **The UI contract is a gate.** The exact owner-supplied `@layerx/ui`
  component API, token stylesheet, borders, dividers, shadows, gradients,
  palette and platform treatments are authoritative in both shells. Missing
  exports, missing tokens or competing local primitives fail
  `make human-check-ui`.
- **Vocabulary is enforced.** Deposit and withdraw are custody-boundary words;
  internal movements are fund, allocate, return, transfer. "Done" appears only
  via the status translation table.
- **Real systems in tests.** Journey suites run against a real node, a real
  `layerx-agentd` and a Paxeer test network. In-process substitutes prove
  nothing at this layer and do not count toward `done`.
- **Honest partials.** A task that lands part of its scope is `pending` with a
  note about what is missing, never `done` on the strength of the finished
  half.
- **Every completed task is published.** When a task reaches `done`, commit and
  push from the repository root with a natural-sounding message describing what
  actually changed, before starting the next task; a finished task that is not
  pushed is not finished.

## Locked decisions

The `[decision]` block in `spec.kvx` is binding for every task. In brief:
custodial-by-default passkey identity with KMS-held keys and no v1 key export
(`custody`); disclosure-bound signing only (`signing`); `layerx-intents` as the
single payload authority (`intents_only`); the `approval.*` module as the only
agent-contract change, built first (`seams_first`); the wallet at the custody
boundary only (`wallet_boundary`); custody-boundary vocabulary reservation
(`vocabulary`); receipt-backed "Done" (`done_is_receipt`); reclaim without
sweeps (`reclaim`); one app, two native shells (`two_shells`); the UI rules as
gates (`ui_rules`); the state-matrix error standard (`error_standard`); the v1
emergency exit (`emergency_exit_v1`); the public explorer as a rebuildable
projection (`explorer_public`); the enumerated v1 scope (`v1_scope`); and the
additive HTTPS+JSON human-api with a generated TS client (`human_api`).

## Verification commands

| Prefix | Runs |
|---|---|
| `make human-build` | Workspace and web application build, TS client generation with its drift gate |
| `make human-lint` / `make human-lint-copy` | Dependency, lint and supply-chain policy; the copy catalog lint |
| `make human-check` / `make human-check-ui` | Boundary and payload-authority gates; the component-library integrity and kit gates |
| `make human-test-<area>` | Crate suites: `intents`, `service`, `paxeer`, `journeys`, `agents`, `approvals`, `notify`, `activity`, `explorer` |
| `make human-e2e-<area>` | Browser suites in both shells: `foundation`, `journeys`, `settings`, `explorer`, `perf` |
| `make human-qualify-*` / `make human-qualify` | Wave 8 release gates and the qualification report |
| `make agent-test-approvals` | The approval seam's daemon suites in the agent workspace |

A task's `verify_cmd` must exist and pass in the same change that completes the
task. Inventing a target without wiring it into the Makefile is a `no_false_success`
violation.
