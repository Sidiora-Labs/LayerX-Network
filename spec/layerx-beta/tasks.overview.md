This plan closes the twelve items of the 2026-09-01 beta fix register and qualifies the entire system on beta infrastructure, to the bar the owner set: everything functional as it would be in production, differing from production only in polish, external audit and infrastructure. It follows the audit's ordering — freeze and contract first, then the authorization and truthfulness seams, then durability, settlement identity and hosted topology, then artifact identity — but not its narrowing: wave 3 also makes the hosted stack reproducible on a beta cluster, wave 4 publishes every ecosystem and gives the existing qualification runner the driver it is missing, and wave 5 stands the whole stack up and runs every gate on one revision.

Every task is anchored to a register item and to real files and symbols the audit named. Tasks in waves 2, 3 and 4 depend only on 1.1 (the ledger and contract), except 3.7 after 3.6 and 4.2 after 4.1, so they run in parallel across agents without conflicting touches. Wave 5 is sequential on the release-candidate revision.

| Wave | Register items | Tasks | Verify gates |
|---|---|---|---|
| 1 | 12 Executed evidence, 13 Contract, 01 Bootstrap | 1.1 ledger + contract; 1.2 clean bootstrap | `beta-contract-check`, `beta-ledger-check`, `platform-test-tooling` |
| 2 | 03 LNI auth, 04 Revocation, 05 Evidence, 11 Human API | 2.1, 2.2, 2.3, 2.4 | `test-daemon-lni-admission`, agentd session/revocation/tenant-resolve/subscription, `human-test-activity`, `human-build human-test-unit human-test-component` |
| 3 | 02 Snapshot, 06 Checkpoint, 10 Asset, 09 Recovery, 07 Hosted | 3.1 – 3.7 | `test-snapshot test-state-root programs-core-test`, protocol/wire/proof/contracts/mirrors, `test-asset-withdraw test-bridge-withdraw`, `platform-test-registry`, `interop-test-ramps`, `platform-hosted-topology-check`, `platform-beta-cluster-up platform-hosted-smoke platform-beta-cluster-down` |
| 4 | 08 Artifacts, 12 Executed evidence | 4.1 manifest and seven publication jobs; 4.2 artifact manifest + verification; 4.3 `beta-qualify` runner gate + in-repo driver | `platform-release-check`, `platform-release-plan`, `beta-driver-test` |
| 5 | 12 Executed evidence, 13 Contract, 07 Hosted, 01 Bootstrap, 06 Checkpoint | 5.1 focused gates; 5.2 beta stack + `beta-qualify`; 5.3 clean e2e + restart + vectors; 5.4 report + contract | `beta-qualify-focused`, `beta-qualify`, `beta-qualify-journey`, `beta-report` |

Register item to requirement to task, one line each:

| Item | Requirement | Tasks | New gates created |
|---|---|---|---|
| 01 Clean bootstrap inputs | req.1 | 1.2, 5.3 | `platform/cli/tests/clean-bootstrap.sh` (wired into `platform-test-tooling`) |
| 02 Programs snapshot state | req.2 | 3.1 | — |
| 03 LNI pre-queue authentication | req.3 | 2.1 | `test-daemon-lni-admission` |
| 04 Agent session revocation | req.4 | 2.2 | — |
| 05 Human evidence verification | req.5 | 2.3 | — |
| 06 Checkpoint identity and freshness | req.6 | 3.2, 5.3 | `tests/vectors/checkpoint/` |
| 07 Hosted reachability and readiness | req.7 | 3.6, 3.7, 5.2 | `platform-hosted-topology-check`, `platform-beta-cluster-up/down` |
| 08 Artifact publication and provenance | req.8 | 4.1, 4.2, 5.2 | — |
| 09 Registry and ramp crash recovery | req.9 | 3.4, 3.5 | `platform-test-registry` |
| 10 Withdrawal asset binding | req.10 | 3.3 | — |
| 11 Human API error contract | req.11 | 2.4 | — |
| 12 Executed qualification evidence | req.12 | 1.1, 4.3, 5.1, 5.2, 5.3, 5.4 | `beta-ledger-check`, `beta-driver-test`, `beta-qualify-focused`, `beta-qualify`, `beta-qualify-journey`, `beta-report` |
| Verdict / one canonical contract | req.13 | 1.1, 4.2, 5.4 | `beta-contract-check` |

Two points need owner confirmation before the corresponding task starts, and one set of inputs must be supplied before wave 5; each is recorded in `qualification.kvx` as an observation when answered:

1. **Checkpoint direction** (`decision.checkpoint_v2`, task 3.2): the native core and `layerx-wire` move to the v2 domains Solidity already uses, unless retained v1 checkpoint evidence must stay verifiable, in which case an explicit version-keyed path is added instead.
2. **Polish boundary** (`decision.polish_boundary`, task 4.3): the beta gate omits `human-qualify-perf`, `human-qualify-ui` and `human-qualify-usability` as polish. If the owner wants performance budgets or soak counted as functional, `human-qualify-perf` moves into `human-qualify-functional`.
3. **Owner inputs** (`decision.owner_inputs`, tasks 3.7, 4.1, 5.2): a beta cluster or permission to create a disposable one on the qualification host; Ethereum and Solana test-network RPC endpoints and funded keys; ramp sandbox credentials; the Paxeer testnet endpoint and credentials; beta pre-release publish tokens for the seven registries; a macOS runner for the Swift and iOS gates; an Android toolchain. Referenced by environment variable or secret name only. A gate that lacks its input is recorded `blocked` with the input named — and because every surface is required, a blocked functional gate is a no-go until it is supplied.
