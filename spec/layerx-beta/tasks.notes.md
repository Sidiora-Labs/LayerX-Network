## Engineering ground rules for this feature

- **The bar is production-functional.** Every surface of the system must work on beta infrastructure the way it would in production. The only things the beta does not need are polish (UI polish, visual regression, accessibility, usability, performance budgets and soak), an external security audit, and production infrastructure and certification. Nothing functional is deferred, excluded or marked unsupported.
- **Fix the anchored path.** Every task names the file and symbol the audit anchored. The task is done when that code path behaves as the acceptance criteria state, proven by its `verify_cmd`. Changing a doc, a label, a status or the audit record closes nothing.
- **Keep every existing check.** The mandatory CLI inputs stay mandatory. `validate_canonical_items` keeps verifying signatures. Solidity keeps its freshness window and v2 domains. The explorer route keeps returning 429. `FileDeploymentJournal::proofs` keeps rejecting an unequal proof set. `platform-qualify` and `human-qualify` in the runner stay exactly as they are. Repairs add checks and supply inputs.
- **No test doubles for the seams or the driver.** Tests drive the real verifier, the real queue, the real journal, the real registry, the real CLI binary and the real beta stack. A fake `EvidenceBundle::verify`, a mocked `SessionRegistry`, a stubbed signature check or a driver that writes evidence without executing is a No Fakes violation and does not count as a gate.
- **Typed refusals, unchanged state.** Every refused call in a repaired seam returns a typed result and leaves state exactly as before, proven by state-root or map equality in the test. Silent fallbacks, empty defaults and best-effort acceptance are forbidden.
- **Gate records are written by running the command.** `beta-qualify.sh` and the runner are the only writers of gate records; they capture the command output under `evidence/<revision>/` and append the record with the revision it ran on. Never hand-write a gate record. A rerun appends; it never edits.
- **Observations stay observations.** The `observation.38.5.*` records in `spec/layerx-platform/qualification.kvx` are linked from gate records when their work is executed through the task 3.7 boundary scripts, or recorded `blocked` with the owner input named. They are never rewritten.
- **Owner inputs are recorded, not assumed.** Beta cluster access, test-network keys, sandbox credentials, the Paxeer testnet, registry publish tokens, a macOS runner, the checkpoint v1 decision and the final go decision belong to the owner. An agent records what the owner supplied and records `blocked` otherwise; a blocked functional gate is a no-go, not an exclusion.
- **Revision discipline.** Wave 5 runs on one release-candidate revision with a clean tree; the runner's source identity fingerprint proves it. If a wave 5 gate exposes a task-scoped defect, the fix lands as a commit, the revision advances, and every wave 5 gate reruns on the new revision. Evidence from the previous revision remains in the ledger as history.
- **Sensitive files.** `.env` and `.env.*` are never read, printed, copied, hashed or inferred, matching the audit's own policy. Secrets referenced by hosted manifests and the beta scripts are named by key or environment variable only.

## Verification commands

| Command | Exists at draft time | What it proves |
|---|---|---|
| `make beta-contract-check` | created by 1.1 | `beta.md` agrees with install docs, hosted manifests, release manifest, workflow and report; no surface below its rung while readiness is claimed; differences limited to the polish boundary |
| `make beta-ledger-check` | created by 1.1 | every gate record names a real commit, real command, existing evidence path, valid outcome; runner evidence matches the record's revision with a clean tree |
| `make platform-test-tooling` | yes; extended by 1.2, 3.6, 3.7 | CLI/emulator/faucet/testnet crate tests, hosted script syntax, and after 1.2 the extracted install.md block from a clean profile |
| `make test-daemon-lni-admission` | created by 2.1 | invalid signature refused with zero occupancy; ack only after insert; capacity survives an invalid flood |
| `make test-admission test-batch-wal-recovery` | yes | existing admission and WAL gates still pass with pre-queue verification added |
| `make agent-test-agentd-session agent-test-agentd-revocation agent-test-agentd-tenant-resolve agent-test-agentd-subscription` | yes; extended by 2.2 | post-closure, post-revocation, post-restart refusal at every entry point; cross-tenant isolation |
| `make human-test-activity` | yes; extended by 2.3 | verified only through the verifier; tamper, principal, domain and authority mismatches unverified with reason |
| `make human-build human-test-unit human-test-component` | yes; extended by 2.4 | schema drift gate; status-to-state mapping; golden failure vectors decode |
| `make test-snapshot test-state-root programs-core-test` | yes; extended by 3.1 | deploy, snapshot, restore, root equality, post-restore CALL; tampered/missing/oversize rejection |
| `make test-protocol agent-test-wire-hashing agent-test-proof-checkpoint test-contracts interop-test-mirrors` | yes; extended by 3.2 | one domain identity and one freshness rule across C, Rust, Solidity and mirrors from shared vectors |
| `make test-asset-withdraw test-bridge-withdraw` | yes; extended by 3.3 | cross-asset refusal with no transfer at both boundaries; conservation |
| `make platform-test-registry` | created by 3.4 | record+proof commit unit, quarantine, restart recovery, replay equivalence |
| `make interop-test-ramps` | yes; extended by 3.5 | staged validation, failed apply retains nothing, idempotent retry, interruption recovery |
| `make platform-hosted-topology-check` | created by 3.6 | every in-cluster URL resolves to an exposed Service port and an admitted NetworkPolicy edge |
| `make platform-beta-cluster-up` / `platform-beta-cluster-down` | created by 3.7 | hosted images built from the revision, real manifests applied on the beta cluster, every journey ready, hosted-smoke variables exported, clean teardown |
| `make platform-hosted-smoke` | yes; extended by 3.6 | pod-to-Service edges and hosted first-payment path with cluster identity (consumes the variables 3.7 exports: `LAYERX_TESTNET_URL`, `LAYERX_GATEWAY_URL`, `LAYERX_FAUCET_URL`, `LAYERX_TEST_AUTH_TOKEN_FILE`, `LAYERX_TEST_CA_FILE`, `LAYERX_TEST_SOURCE_DID`) |
| `make platform-release-check platform-release-plan` | yes; extended by 4.1 and 4.2 | manifest and workflow agree in both directions across seven ecosystems; artifact manifest emitted; published bytes verified |
| `make beta-driver-test` | created by 4.3 | runner tests cover `beta-qualify` and `human-qualify-functional`; the driver executes the adoption and multichain inventories against the local stack and refuses to synthesise |
| `make beta-qualify-focused` | created by 5.1 | runs every wave 2 and 3 verify_cmd plus `qualify-faults`, `qualify-replay`, `agent-qualify-faults`, `agent-qualify-fuzz`, `agent-qualify-wire`, `agent-qualify-boundary`, `human-qualify-faults` on one revision and records gate entries |
| `make beta-qualify` | created by 4.3; run by 5.2 | `release_runner.py beta-qualify` with `LAYERX_QUALIFICATION_REAL_STACK=1` and the in-repo driver: human functional journeys, platform build/lint/test, SDK conformance, middleware, docs samples, hosted smoke, agent install, emulator conformance, real agent-framework, iOS and Android integrations, Programs tests, interop, migration testnets, ramp sandboxes, release check, plus adoption, Programs, interop and multichain external evidence |
| `make beta-qualify-journey` | created by 5.3 | clean-profile verified payment within the five-minute bound locally and against the hosted beta endpoints; independent receipt verification; unknown-outcome reconciliation; cross-language vectors per published artifact |
| `make beta-report` | created by 5.4 | reached rung per surface against required rung, stop-condition table, single go/no-go line, all from gate records only |

## Stop conditions the report must show clear before any invitation

From lane 48, widened for the full surface. Each is cleared only by a named gate record on the release-candidate revision.

| Stop condition | Cleared by |
|---|---|
| Clean bootstrap fails from the published path | `beta-qualify-journey` bootstrap gate (req 1.5, 1.6) |
| Success is not independently verifiable | `beta-qualify-journey` independent receipt verification gate (req 12.4) |
| An unknown outcome cannot be reconciled | `beta-qualify-journey` restart gate (task 5.3 do_3) |
| Revocation is ineffective at any boundary | agentd suites via `beta-qualify-focused` (req 4.3, 4.6) |
| Readiness is not journey-transitive | `platform-hosted-topology-check` and `platform-hosted-smoke` on the beta cluster (req 7.4, 7.5) |
| Artifacts lack identity | `platform-release-check` with artifact manifest, all seven ecosystems (req 8.2, 8.3, 8.4) |
| Any surface below its beta rung | `beta-qualify` runner report with every planned command and external gate passed (req 12.4, 13.3) |
| Scope closure is unverified | `beta-contract-check` against `report.md` (req 13.4) |

## Register cross-reference

The raw finding behind each requirement, with its lane result path, is listed in `docs/00-source-brief.md`. Requirement keys `audit_finding` and `register_item` in `spec.kvx` carry the same identifiers; specgen ignores them, so they are traceability only.
