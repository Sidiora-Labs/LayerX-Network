# LayerX Protocol Codebase Audit — 15-Agent Refresh

**Audit date:** 2026-08-28  
**Canonical checkout:** `/root/Layerx-protocol`  
**Audited commit:** `9d388f97799b150bbae5d4dbd0af7cac97eea33c`  
**Branch state at launch:** `main` matched `origin/main`  
**Audit engine:** 15 independent OpenCode sessions using `openrouter/z-ai/glm-5.3-flash`  
**Assurance pass:** Primary Codex agent independently re-read the highest-severity source seams and reconciled conflicting lane reports  
**Explicit exclusion:** `/root/Layerx-protocol/paxeer-network`  
**Mechanical exclusions:** `.worktrees`, `node_modules`, `target`, `build`, `.gradle`, vendored/generated caches, binaries, and other derived artifacts

## 1. Executive verdict

**Verdict: NOT RELEASE-READY.**

The codebase has a large, serious, and in several areas unusually well-designed implementation. The C protocol kernel, deterministic Programs runtime, authenticated evidence paths, interop mirror, human-plane secret hygiene, and much of the developer tooling are substantive implementations rather than scaffolding. The overnight work materially advanced the Programs frontier: tasks 33.3, 33.4, 33.5, and 34.1 are now source-present and marked `implemented`.

The repository is nevertheless not ready for a production or qualification-success claim. The most important reasons are:

1. The daemon opens its canonical and batch logs without recovering their durable offsets. A checkpointed restart consequently scans an apparently empty canonical log and fails closed.
2. The hosted program registry accepts source archives without authenticating the caller, then executes the archive-declared build command. It defaults to loopback, but any exposed deployment or trusted local peer turns this into a command-execution boundary.
3. The canonical node has no production network submission route. Its served HTTP surface is GET-only and its activity ingress is a local FIFO; no production FIFO producer exists in the audited tree. The agent LNI write client likewise has no production server.
4. The production human HTTPS binary delegates all operations to a Unix-socket component protocol for which no server implementation exists in the repository. Its production custody provider similarly speaks an LXKP mTLS protocol with no in-tree gateway server.
5. The Go SDK's canonical receipt decoder cannot consume receipts carrying a Programs outcome. The same feature is absent by source search from the other generated SDK surfaces, although only Go was proven line-by-line.
6. Three files covered by the SDK lock have hashes that differ from `platform/sdk/pipeline.kvx`; the repository's own SDK drift check is therefore expected to refuse this commit.
7. Three frozen interpreter refusal vectors declare two registers but write to register 2. They are rejected during decoding instead of exercising the intended runtime arithmetic refusals, contradicting the conformance test.

There is also an immediate local security incident: the ignored root `.env` contains a GitHub-token-shaped credential. The audit did not print or copy it. The file is untracked, has no `.env` path history, and a tracked-HEAD token-shape scan found zero files. The credential must still be revoked and rotated because local tooling can read ignored files.

No build, test, lint, benchmark, replay, deployment, or runtime qualification command was run. Under the repository's phase boundary, this audit establishes source presence and static coherence only. It does not establish compilation, passing behavior, or production certification.

## 2. Scope and method

### 2.1 Audit lanes

The pass divided the canonical source into 15 disjoint review lanes:

1. C protocol core, state, receipts, arithmetic, and kernel transitions
2. Programs runtime, ABI, metering, capabilities, storage, and C ingress
3. Programs registry, lifecycle evidence, interfaces, accounts, and wind-down
4. Recent Programs work: interpreter, bindings, benchmarks, and sandbox lease
5. Native daemon, WAL, receipt authority, listeners, and recovery
6. Human backend, authentication, custody, journeys, and service wiring
7. Human web application, generated client, explorer, privacy, CSP, and accessibility
8. Platform CLI, emulator, schemas, generation workflow, and developer tooling
9. Hosted gateway, registry, faucet, testnet, dashboard, and webhooks
10. Interop gateway, migration, mirror publication, and mirror verification
11. SDKs, generators, language bindings, schemas, and conformance locks
12. Crypto, authentication, CI, dependencies, containers, provenance, and repository hygiene
13. Spec-to-source implementation reality and qualification-log freshness
14. Cross-cutting persistence, concurrency, fees, snapshots, and authorization
15. Holistic product and release readiness

Each lane was instructed to remain read-only, avoid build/test commands, cite concrete source evidence, distinguish implementation from qualification, and label uncertainty. Exploration was time-boxed, followed by a tools-disabled final report. The primary agent then re-read the high-severity seams directly.

### 2.2 Assurance labels

This report uses three evidence levels:

- **Confirmed:** independently re-read during consolidation and supported by the cited source.
- **Scope-confirmed:** supported by a complete lane read within its assigned files, but not re-read independently during consolidation.
- **Conditional or unproven:** depends on an unread caller, runtime behavior, deployment topology, or external component. These claims are either downgraded or kept out of the findings register.

### 2.3 Severity model

- **Incident:** exposed operational material requiring immediate owner action.
- **P1:** security, durability, compatibility, or end-to-end product blocker.
- **P2:** material correctness, policy, scalability, or defense-in-depth defect.
- **P3:** maintenance, observability, UX, performance, or future-risk advisory.

Missing deployment seams are classified as P1 release blockers, not as remotely exploitable P0 vulnerabilities. No tracked-source P0 vulnerability was proven in this pass.

## 3. Immediate action

### INC-01 — Token-shaped credential in ignored root `.env`

**Evidence level:** Confirmed  
**State:** Immediate owner action required

The canonical checkout contains a root `.env` whose first-party contents match a GitHub personal-access-token shape. The audit verified only a boolean match and did not display the value.

- `.env` is ignored by the repository's `.gitignore` rule.
- `.env` is not tracked.
- `git log --all -- .env` returns no path history.
- A token-shape scan over tracked `HEAD`, excluding `paxeer-network`, found zero tracked files.

This limits the observed exposure to the local checkout, but ignored files remain visible to developer tools, shells, local servers, and agents. Revoke and rotate the credential, then move the replacement to the intended secret manager. Deleting the file was deliberately left to the owner.

## 4. Confirmed P1 findings

### P1-01 — Canonical and batch logs are opened but never recovered

**Evidence level:** Confirmed  
**Subsystem:** `cmd/layerxd`, storage, restart recovery

`lxp_log_open()` initializes `write_offset`, `previous_record_offset`, and `next_sequence` to zero regardless of the existing file contents (`src/storage/lxp_log.c:152-169`). `open_process()` opens the feed, canonical, authority, and batch logs (`cmd/layerxd/lxp_daemon_process.c:2752-2762`), but the only runtime calls to `lxp_log_recover()` are for the receipt-authority log and Programs feed log. The generic sequencer recovery function invokes `lxp_log_recover()`, but it has no production caller.

The restart consequence is direct:

- A selected checkpoint makes `reconcile_snapshot_evidence()` search the canonical log up to `canonical_log.write_offset` (`lxp_daemon_process.c:1009-1110`).
- Because that offset remains zero, no pending/complete group can be found.
- The function returns `LXP_ERR_PROJECTION_STALE`.
- Batch-log reconciliation similarly sees no durable batch records.
- The node fails closed instead of resuming.

The authority and feed logs do not share this defect because their owning components recover them explicitly.

**Remediation:** Recover canonical and batch logs immediately after open and before history-index construction, checkpoint reconciliation, or WAL replay. A human qualification pass must then exercise clean restart, torn-tail recovery, checkpoint restart, and each batch-WAL recovery classification.

### P1-02 — Hosted registry accepts unauthenticated source and executes its declared command

**Evidence level:** Confirmed  
**Subsystem:** `platform/hosted/registry`

`Registrar::route()` handles `POST /__registry/sources` without checking authorization (`platform/hosted/registry/src/routes.rs:127-148`). `ingest_source()` accepts a URI, build plan, and archive, then publishes them to the mirror (`routes.rs:517-553`). `POST /v1/programs/registry/{program}/source` reaches `verify()`, which fetches that mirrored source and calls the reproducible builder (`routes.rs:243-258,350-419`). The builder splits the plan's command and calls `Command::new(program).args(arguments)` in the materialized archive directory (`platform/hosted/registry/src/builder.rs:84-149`).

The gateway sends a bearer registry token (`platform/hosted/gateway/src/main.rs:1146-1155,1318-1327`), but the registry server never validates it. This creates an especially dangerous false boundary: the caller believes it authenticated, while the callee ignores the credential.

The service defaults to `127.0.0.1:9420`, which limits exposure in the default configuration. The defect becomes an arbitrary-command-execution path for any untrusted local peer or deployment that exposes the port through a proxy or sidecar.

The same service handles one connection at a time with no visible socket read deadline (`platform/hosted/registry/src/main.rs:87-107`) and performs long builds inline, so a partial request or one valid build can monopolize it.

**Remediation:** Authenticate every non-health route, reserve `/__registry/*` for an operator identity, minimize the gateway credential, replace free-form build commands with a pinned allowlisted builder entrypoint, add connection deadlines, and move builds to a bounded worker queue.

### P1-03 — No production submission seam reaches the native node

**Evidence level:** Confirmed  
**Subsystem:** native daemon, agent boundary, product integration

The daemon's served protocol router rejects every method except GET (`cmd/layerxd/lxp_daemon_protocol.c:380-388`). The actual activity ingress is `LAYERX_NODE_ACTIVITY_FIFO`: `lxp_daemon_serve()` opens the FIFO, reads a four-byte length and canonical activity bytes, then calls `lxp_daemon_submit()` (`cmd/layerxd/lxp_daemon_process.c:2857-2861,2917-2940`). Source search found no production FIFO producer. The only additional submit bridge is under `agent/tests/boundary/node/`.

The Rust agent client implements the LNI framed protocol and signed submission, but the production agent sources contain clients, not an LNI server. The only `layerx-agentd` binary listener accepts GET requests for `/healthz` and `/v1/programs/{id}/balances` (`agent/crates/layerx-agentd/src/main.rs:210-289`).

Therefore the core transition engine is source-present, but an external agent, hosted gateway, SDK, or CLI cannot traverse a production write boundary into it using only the shipped canonical components.

**Remediation:** Implement one authoritative authenticated submission boundary—either an LNI server bridging to `lxp_daemon_submit()`, or a bounded authenticated daemon POST/framed route—and remove or clearly document competing seams. Qualify from real client prepare/sign/submit through durable receipt lookup.

### P1-04 — Human production binary depends on two absent server-side boundaries

**Evidence level:** Confirmed  
**Subsystem:** human backend and custody

The production human service constructs only `UnixComponents` from `LAYERX_HUMAN_COMPONENT_SOCKET` (`human/crates/layerx-human-service/src/main.rs:24-40`). That implementation sends `session.authorize`, `human-api.execute`, and `readiness` requests over a framed Unix socket (`src/server/backend.rs:301-445`). Repository search found no non-test listener implementing those request kinds. The only `HumanApiComponents` implementation is the client-side `UnixComponents` adapter.

The production custody path has the same shape. `RemoteKmsProvider` sends the bounded LXKP protocol over mTLS (`human/crates/layerx-human-service/src/custody/provider.rs:487-560`). The only `KmsProvider` implementations are the remote client and development envelope provider; no in-tree LXKP gateway server exists.

Both boundaries fail closed, which protects custody integrity, but the shipped production binary cannot execute the real human operations or open production custody using repository-owned components alone.

**Remediation:** Ship the privileged human component service and the LXKP KMS/HSM gateway, or explicitly define and package the externally owned components with protocol conformance artifacts. Do not mark the end-to-end human service qualified until both peers are deployed and exercised.

### P1-05 — Go SDK rejects canonical Programs receipts

**Evidence level:** Confirmed for Go; cross-language extent is source-search evidence  
**Subsystem:** platform SDK verification

The canonical C encoder writes an optional `program_outcome` after the receipt timestamp and before the signature marker (`src/protocol/lxp_receipt.c:552-559`). The canonical decoder detects and validates it (`lxp_receipt.c:645-669`).

The Go decoder reads the signature marker immediately after the timestamp (`platform/sdk/go/verify.go:270-287`). For a receipt with a Programs outcome, it interprets the outcome's first byte as the signature marker and refuses the receipt as non-canonical. This makes valid Programs receipts unverifiable through the Go SDK.

A repository search found no `program_outcome`, `programOutcome`, or `program-outcome` handling in the other SDK directories. That is strong evidence of a family-wide omission, but this report proves the decoder flow only for Go.

The Go decoder also omits the C decoder's zero-field canonicality checks for global sequence, module id/version, timestamp, and resulting state root, creating a second divergence in what the SDK may label verified.

**Remediation:** Port the canonical Programs-outcome parse and post-decode invariants into the shared generator model, regenerate every language, refresh the lock, and run receipt vectors across all SDKs during qualification.

### P1-06 — Frozen interpreter refusal vectors contradict their conformance contract

**Evidence level:** Confirmed  
**Subsystem:** `layerx-programs-interpreter`

Refusal vectors 2, 3, and 4 in `programs/crates/layerx-programs-interpreter/vectors/v1-refusals.hex` declare two registers. Their arithmetic instructions use destination register 2. `register()` requires `index < registers` (`src/lib.rs:115-117`), and the ALU decoder validates the destination before execution (`src/lib.rs:126-145`).

The conformance test expects only vectors 0, 1, and 6 to fail validation and explicitly unwraps validation for vector indices 2, 3, and 4 (`tests/conformance.rs:207-218`). Those three vectors therefore fail for the wrong reason before the intended overflow, divide-by-zero, and signed-division-overflow runtime paths are reached.

**Remediation:** Declare three registers or retarget the destination to an existing register, preserve the intended refusal oracles, and have human qualification run both the independent interpreter oracle and built-WASM runtime route.

### P1-07 — SDK lock disagrees with three declared generated outputs

**Evidence level:** Confirmed  
**Subsystem:** SDK generation and conformance

The current SHA-256 values of these files do not match `platform/sdk/pipeline.kvx`:

- `platform/sdk/jvm/src/main/java/com/sidiora/layerx/sdk/verify/LocalVerifier.java`
- `platform/sdk/jvm/src/conformance/java/com/sidiora/layerx/sdk/ConformanceMain.java`
- `platform/sdk/conformance/run-jvm.sh`

The lock records `3475…`, `a0cf…`, and `9ff8…`; the current files hash to `97a1…`, `53f6…`, and `d0d7…`, respectively. No generator or check was executed, but the static digest mismatch is decisive: the repository's declared generated-output invariant does not hold at this commit.

**Remediation:** Determine whether the file changes are intended generated outputs or handwritten policy. Regenerate or refresh the lock through the canonical generator path; if they are handwritten, remove them from the generated-file claim and apply a separate integrity gate.

## 5. Confirmed P2 findings

### P2-01 — Consensus permits ABI downgrade that Rust projections refuse

The C Programs upgrade executor validates only that the new ABI is 1 or 2, then unconditionally writes the new ABI into the program record (`src/modules/programs/deploy.c:320-336,435-445`). The Rust registry and resolver explicitly refuse a v2-to-v1 transition (`programs/crates/layerx-programs-registry/src/lib.rs:407-416`; `resolver.rs:167-173`).

An authorized ABI downgrade can therefore be accepted in protocol state while the Rust projection refuses to ingest it. The path is fail-closed rather than forgeable, but it can make registry-dependent reads unavailable.

**Remediation:** Define one normative upgrade rule and enforce it identically in C execution, receipt evidence, and Rust projection.

### P2-02 — Program events are not metered and are count-bounded late

`event_emit` reads up to 64 bytes of topic and 65,536 bytes of data and calls `Abi::emit_event()` (`programs/crates/layerx-programs-runtime/src/host/events.rs:10-38`). `emit_event()` checks authority and per-event bounds, then allocates and appends the event with no metering charge (`src/abi/mod.rs:640-663`). The aggregate event-count cap is enforced only when the canonical event envelope is constructed (`src/abi/mod.rs:341-369`).

This is deterministic and fail-closed, but allows unpriced host work and can discover an over-count only after work has been performed.

**Remediation:** Charge event bytes and enforce the event-count bound at emission time.

### P2-03 — Capability transport bounds disagree across the C ingress and guest ABI

The production C ingress refuses capability encodings larger than 4,096 bytes (`programs/crates/layerx-programs-runtime/src/ffi_call.rs:2028-2049`). The guest call surfaces and C SDK allow 16,384 bytes (`src/host/calls.rs:35-44,154-163`; `programs/sdk/c/include/layerx/program.h:24-34`). Valid larger capability sets can therefore be constructed and decoded at one boundary but cannot enter through the production C seam.

**Remediation:** Freeze one protocol constant and derive every transport and SDK bound from it.

### P2-04 — Explorer security headers omit the public verification plane

The Next proxy adds CSP, nonce, referrer, content-type, and permissions headers only for `/app/:path*` (`human/apps/web/src/proxy.ts:11-30`). The public `/explorer/*` plane and its pasted-evidence verifier are outside that matcher.

**Remediation:** Apply the policy to all document routes or define equivalent headers centrally, then qualify the nonce/CSP behavior for both planes.

### P2-05 — Explorer UI converts overload into evidence refusal

The verification API returns 429 with `{status:"overloaded"}` and `Retry-After` (`human/apps/web/src/app/api/explorer/verify/route.ts:79-84`). The UI maps every non-503/non-409 failure to `refused` and does not read the response body (`human/apps/web/src/explorer/verifier.tsx:41-50`). A temporary concurrency limit is therefore presented as failed evidence.

**Remediation:** Model 429 as a distinct retryable state and honor `Retry-After`.

### P2-06 — Gateway readiness is hardcoded false

`platform/hosted/gateway/src/main.rs:1238-1262` probes its dependencies and then sets `let ready = false`, so `/readyz` always returns 503. This may be an honest gate for the unimplemented state-proof boundary, but it makes standard readiness-based deployment impossible.

**Remediation:** Document the deliberate degraded mode and probe contract, or derive readiness from the actual required dependencies once the missing boundary exists.

### P2-07 — Supply-chain policy is stronger for workflows than containers

The repository audit enforces immutable workflow/action references, but production Dockerfiles use mutable base-image tags instead of digest pins. The backport workflow uses an external reusable workflow with `secrets: inherit` and `contents: write` on `pull_request_target`. Dependabot coverage is concentrated under the excluded Paxeer subtree and omits major root, agent, platform, and human ecosystems.

These are policy and exposure gaps, not proof of compromise.

**Remediation:** Digest-pin base images, minimize reusable-workflow permissions and secrets, audit or inline the external workflow, and extend automated dependency updates across the canonical monorepo.

### P2-08 — Core mixed-batch API silently processes only the Programs prefix

`lxp_kernel_prepare_activity_batch()` counts only the leading `LX_PROGRAMS_CALL` activities and does not reject a non-Programs suffix (`src/protocol/lxp_kernel.c:2585-2590`). A caller passing a mixed batch receives a prepared prefix without an explicit processed-count result.

The current daemon caller appears to construct Programs batches, so production reachability of a mixed input was not proven. The API contract is still hazardous for future callers.

**Remediation:** Reject `count != offered_count` or return the processed count as part of the explicit contract.

## 6. Strongest implemented areas

### 6.1 Protocol core and state

- The staged state journal has explicit commit/rollback discipline and performs replacement allocations before publication.
- Snapshot and state-root code use canonical ordering, duplicate rejection, bounded proofs, and domain-separated hashes.
- Receipt, fee, u128, and u256 arithmetic surfaces contain systematic overflow, length, and canonicality checks.
- Prepared Programs batches bind level snapshots, activity execution, final state, receipts, events, and publication digests.
- WAL and checkpoint formats are cryptographically and structurally revalidated before recovery decisions.

### 6.2 Programs runtime

- Capabilities narrow downward, with typed escalation refusals and owner-origin rules for program spending.
- Nested calls have explicit depth, edge, fanout, reentrancy, and rollback rules.
- Access declarations are canonical, presence-sensitive, and enforced across storage, accounts, and nested calls.
- Metering schedules are versioned, canonically encoded, and paired with instrumented golden material.
- Program-owned accounts, interfaces, lifecycle evidence, and program-state proofs are substantive cross-language implementations.

### 6.3 Human plane

- Session and custody code consistently uses digest-only storage, zeroization, redacted debug output, strict framing, bounded parsing, and fail-closed errors.
- Passkey ceremonies are server-side, single-use, expiring, counter-checked, and revocable by epoch.
- The custody signer binds disclosures byte-for-byte, verifies returned signatures, and consumes custody-side step-up evidence.
- The web application uses the owner-supplied `@layerx/ui` package and contains evidence-aware balance and activity presentation rather than optimistic success rendering.

### 6.4 Hosted and interop planes

- Hosted gateway, webhooks, faucet, and dashboard surfaces contain real signature verification, constant-time comparisons, idempotency, and bounded parsers.
- The webhook engine has DNS/IP SSRF restrictions, lease-based delivery, sequence replay protection, signing-key rotation, retry/backoff, and dead letters.
- The interop mirror is the cleanest broad lane in this pass: no P0/P1/P2 defect was established. It has explicit finality/reorg state, pinned authorities, canonical archive verification, and idempotent publication journals.
- Migration and settlement paths consistently require independently verified LayerX receipts before reporting settlement.

### 6.5 Developer platform

- The CLI command tree reaches real implementations, uses strict HTTP and credential handling, and refuses indeterminate state honestly.
- The emulator is wired to the real C kernel rather than a fake transition engine.
- Schema sources, generator machinery, and most declared output digests are coherent; the three mismatches are narrow and actionable.

## 7. Implementation-reality matrix

| Subsystem | Source state | Strongest evidence | Material gap | Qualification state |
|---|---|---|---|---|
| C protocol kernel | Deeply implemented | atomic journals, roots, receipts, arithmetic, prepared batches | mixed-batch API hazard; dead legacy branches | not run |
| Native daemon | Substantive | WAL, authority log, checkpoints, read API | canonical/batch recovery bug; no network write route | not run |
| Programs runtime | Deeply implemented | capabilities, access sets, nested calls, metering, FFI | unmetered events; capability-bound mismatch | not run |
| Programs registry | Substantive | receipt/header/state proof chain, interfaces, accounts | ABI downgrade divergence; hosted registry auth | not run |
| Recent Programs work | Source-present | interpreter, bindings, benchmark files, lease model | refusal-vector inconsistency; 33.3/33.5 not fully re-audited | not run |
| Agent libraries | Large source surface | prepare, submit, outbox, receipts, events, budgets | no production LNI server; narrow agentd binary | not run |
| Human backend | Large source surface | auth, custody, signing, bounded HTTPS router | missing component server and LXKP gateway | not run |
| Human web | Substantive product UI | schema client, privacy, explorer, evidence-aware state | CSP scope and overload-state mismatch | not run |
| CLI/emulator | Source-coherent | real dispatch, real kernel link, strict transport | payment command unavailable on default emulator | not run |
| Hosted services | Substantive but uneven | gateway auth, webhooks, faucet quotas | registry execution boundary; gateway never ready | not run |
| Interop/mirror | Strong static coherence | finality, reorg, proofs, receipt authority | checkpoint trust and migration deployment pending | not run |
| SDKs | Generated family present | shared schema, lock, money/crypto code | Programs receipt parity; three drifted outputs | not run |
| CI/supply chain | Strong baseline | pinned actions, public audit, cargo-deny | mutable container tags; external workflow trust | not run |
| Spec ledger | Broadly honest | explicit implemented/pending/qualification taxonomy | stale observations and path metadata remain | not run |

## 8. Current task ledger and remaining agent-owned work

The active spec contains 238 `[task.*]` sections. Thirty-seven are umbrella task groups, leaving **201 executable leaf tasks**:

- **106 `done`**
- **54 `implemented`** — source is written; qualification is still pending
- **41 `pending`**

The 41 pending leaf tasks divide into:

- **14 implementation tasks owned by agents**
- **25 qualification tasks owned by human engineers**
- **2 documentation tasks**

### 8.1 Remaining implementation tasks

| Wave | Task | Purpose | Dependencies |
|---|---|---|---|
| 19 | 33.6 | Make Programs first-class on the agent plane | 33.3, 33.4, 28.8 |
| 19 | 33.7 | Finish Program operations across hosted gateway and seven SDKs | 17.4, 33.6 |
| 20 | 34.2 | Escrow sandbox lease funds in a program-owned account | 34.1, 30.4 |
| 20 | 34.3 | Execute sandbox work under lease-scoped capabilities | 34.1 |
| 20 | 34.4 | Settle sandbox usage incrementally | 34.2, 34.3 |
| 20 | 34.5 | Destroy the sandbox and reclaim state | 34.4, 29.4 |
| 20 | 34.6 | Snapshot and restore sandbox state under renter authority | 34.1, 29.3 |
| 21 | 35.1 | Commit to execution state per step | 32.3 |
| 21 | 35.2 | Verify a single execution step on-platform | 35.1 |
| 21 | 35.3 | Arbitrate disputes by bisection | 35.2 |
| 21 | 35.4 | Build the compute marketplace program | 30.4, 33.1 |
| 21 | 35.5 | Settle usage inside a challenge window | 35.4 |
| 21 | 35.6 | Stake and slash providers | 35.3, 35.5 |
| 21 | 35.7 | Accept attested non-deterministic inputs | 31.4 |

The two pending documentation tasks are 34.8 and 35.8. The 25 qualification tasks are deliberately deferred to waves 90–93 and cover human journeys, hostile-plane checks, parity, performance, fuzzing, sandbox escape, determinism, monetary-law replay, and release reporting.

### 8.2 Status of the overnight Programs work

- **33.3 typed bindings:** `implemented`; source and CLI surfaces exist, but this lane did not establish generated-code completeness or a passing stale-binding gate.
- **33.4 deterministic interpreter:** `implemented`; the core is real and success vectors are coherent, but three refusal vectors are malformed relative to their intended stage.
- **33.5 interpreter-vs-compiled pricing:** `implemented`; benchmark files are source-present, but no benchmark was executed.
- **34.1 sandbox lease state model:** `implemented`; the lease crate and state model are present, but escrow, execution, usage, destruction, and snapshot tasks remain pending.

### 8.3 Recommended implementation sequence

Within the declared dependency graph, the remaining source work naturally sequences as:

1. 33.6, then 33.7.
2. 34.2, 34.3, and 34.6 as the independent wave-20 branches; 34.4 after 34.2+34.3; 34.5 after 34.4.
3. 35.1, 35.4, and 35.7 as independent wave-21 roots; then 35.2 and 35.5; then 35.3; finally 35.6.

The audit findings should be recorded for human qualification and targeted repair authority. They should not be silently mixed into unrelated implementation tasks.

## 9. Fifteen-lane results

| Lane | Verdict | Most important result | Coverage limitation |
|---|---|---|---|
| 1 Core protocol | Not clean | strong atomicity; mixed-batch truncation and several P3 canonicality issues | ledger and several protocol files unread |
| 2 Programs runtime | Not clean | unmetered events; 4 KiB/16 KiB capability mismatch | several host and SDK bodies unread |
| 3 Programs registry | Not clean | C/Rust ABI downgrade divergence | production registry consumer reachability incomplete |
| 4 Recent Programs | Not clean | refusal vectors 2–4 fail at decode | bindings, benchmarks, and lease not fully read before stop |
| 5 Daemon | Not clean | canonical/batch logs never recovered | agent Rust plane excluded from this lane |
| 6 Human backend | Not clean | component server and LXKP server absent | journeys and store not read in depth |
| 7 Human web | Not clean | explorer CSP gap and 429/refusal mismatch | e2e and package overlays unrun/unread |
| 8 CLI/emulator | Clean for assigned severity | real command wiring and emulator boundary | hosted endpoint reachability unproven |
| 9 Hosted plane | Not clean | unauthenticated registry execution chain | integrations and some dashboard/webhook files unread |
| 10 Interop/mirror | Clean through P2 | strong finality, proof, and idempotency design | checkpoint trust and migration deployment pending |
| 11 SDKs | Not clean | Programs receipt gap; three lock mismatches | Go read deeply; other languages largely searched |
| 12 Security/supply chain | Not clean | local credential incident; supply-chain gaps | external workflow mapping not fetched |
| 13 Spec reality | Not clean | stale ledger observations; future waves honestly pending | several log-backed claims not re-read |
| 14 Cross-cutting | Not clean | strong WAL/fees; conditional feed duplicate rejected in assurance pass | Rust and product planes excluded |
| 15 Product readiness | Not clean | write boundary and agent serving plane absent | hosted/human/interop bodies sampled rather than exhaustive |

## 10. Qualification and release gates still required

Static review cannot close any of these gates:

1. **Durability:** clean restart, checkpoint restart, torn tails, WAL classification, authority/feed/canonical/batch consistency, and crash injection at every fsync boundary.
2. **Consensus and determinism:** C/Rust differential execution, interpreter-vs-built-WASM vectors, metering schedule replay, parallel scheduling, and cross-host deterministic results.
3. **Monetary law:** conservation across Programs calls, program-owned accounts, escrow, sandbox settlement, reversions, idempotent replay, and program-heavy histories.
4. **Boundary integration:** real LNI or canonical submission server, hosted gateway to node, human HTTPS to component service, custody to real KMS/HSM, and independent receipt authority.
5. **SDK parity:** canonical receipts including Programs outcomes, zero-field canonicality, schema drift, checkpoint verification, error taxonomy, and packaging in all seven languages.
6. **Security:** hosted registry isolation/authentication, secret rotation, container provenance, CI privilege minimization, webhook/faucet fault injection, and dependency freshness.
7. **Product:** every human journey, honest unknown outcomes, explorer tamper detection, CSP, accessibility, privacy, offline/degraded states, and usability thresholds.
8. **Performance:** daemon read concurrency, 64 MiB response arena pressure, Redis connection churn, webhook shard scaling, registry build queueing, interpreter pricing, and soak tests.
9. **External infrastructure:** Paxeer settlement integration, self-hosted mirror runners, Codecov/OIDC/IAM trust, registries, release signing, and publicly retrievable artifacts.

## 11. Hallucination-control and rejected claims

The consolidation pass deliberately rejected or narrowed several lane hypotheses:

- **Rejected:** Programs feed-store duplicate-on-restart as a separate P1. `lxp_history_open()` calls `lxp_history_index_rebuild()` before feed recovery (`src/replica/lxp_history.c:259-292`), closing the lane's stated condition once the log has a correct recovered offset.
- **Not promoted:** same-actor duplicate idempotency keys causing a fatal prepared-batch failure. The trigger depends on scheduler co-leveling that was not independently established.
- **Not promoted:** interpreter `response_capacity: 4` versus an eight-byte response. The enforcement path was not fully traced.
- **Narrowed:** the human web session-cookie mismatch. The mismatched `APP_SESSION_COOKIE` is a dead constant; the operative access/refresh/CSRF cookie names align.
- **Not promoted:** snapshot two-root unsatisfiability, live account-refresh failure, and cross-asset fee mispricing. Each depends on an unread deciding function.
- **Rejected as a defect:** Merkle odd-leaf self-pairing, snapshot size arithmetic, receipt builder aliasing, and the distinction between canonical state root and receipt-chain root; the audited implementations were internally consistent.
- **No high-severity interop claim:** the mirror/migration lane found only P3 rollback/freshness/performance notes and explicit pending deployment seams.
- **Credential handling:** the token value was never placed in this report, command output, or a repository file by the consolidation pass.

## 12. Final assessment

LayerX is not an empty or superficial codebase. It contains credible protocol, runtime, custody, verification, and interoperability engineering. The main risk is no longer a lack of code everywhere; it is the gap between strong subsystem implementations and the production seams that join them, plus a growing qualification debt.

The correct current statement is:

> The canonical monorepo contains a broad, advanced implementation with 160 of 201 executable tasks either done or source-implemented. It is not yet production-ready. Fourteen implementation tasks remain, 25 human qualification tasks remain, and the confirmed restart, registry-authentication, submission-boundary, human-component, SDK-receipt, vector, and generation-drift defects must be resolved before release certification.

That conclusion is source-grounded at commit `9d388f97799b150bbae5d4dbd0af7cac97eea33c`. It makes no claim that the repository compiles, tests pass, or any deployment is qualified.
