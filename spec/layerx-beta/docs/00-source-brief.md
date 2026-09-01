# Source brief: the 2026-09-01 beta fix register

This feature is derived from the read-only 48-lane audit of LayerX at revision
`d5ff7e263a48cac4ffde548bf018100d0e5cd3b4`, presented as the beta fix register at
`/tmp/layerx-audit-20260901/presentation/index.html`. Raw lane results are at
`/tmp/layerx-audit-20260901/runtime-protocol/runs/lane-NN/result.json`. The audit's
independent QA (`presentation/QA.md`) verified the register's counts, anchors and
provenance and reported no data defects affecting the twelve items.

Verdict (lane 48, W4-L48-F001, critical): **NO-GO** for the beta as advertised.
Every build, test, runtime, deployment and certification gate was unrun; the only
evidence classes were `source_present`, `statically_coherent` and
`test_defined_but_unrun`. Recommendation: complete the blockers and obtain owner
certification, or cut the beta to a narrow, local, non-custodial, independently
verified payment path.

**Owner instruction (2026-09-02), which this feature follows:** the beta is the
*entire system functional as it would be in production*. The difference from
production is only that the beta need not be polished, externally audited, or
deployed on production infrastructure. The audit's narrowing recommendation is
therefore not adopted. This feature repairs every register item and qualifies
every surface on beta infrastructure.

## Register item to requirement

| # | Register item | State | Finding | Lane | Requirement | Tasks |
|---|---|---|---|---|---|---|
| 01 | Clean bootstrap inputs | MISSING | W4-L47-F001 | 47 six-day-mvp-cutline | req.1 | 1.2, 5.2 |
| 02 | Programs snapshot state | BROKEN | W4-L48-F002 | 48 independent-final-editor | req.2 | 3.1 |
| 03 | LNI pre-queue authentication | BROKEN | W4-L48-F003 | 48 | req.3 | 2.1 |
| 04 | Agent session revocation | BROKEN | W4-L48-F004 | 48 | req.4 | 2.2 |
| 05 | Human evidence verification | BROKEN | W4-L48-F005 | 48 | req.5 | 2.3 |
| 06 | Checkpoint identity and freshness | BROKEN | W4-L48-F006 | 48 | req.6 | 3.2, 5.2 |
| 07 | Hosted reachability and readiness | BROKEN | W4-L48-F007 | 48 | req.7 | 3.6, 5.3 |
| 08 | Artifact publication and provenance | MISSING | W4-L48-F008 | 48 | req.8 | 4.1, 4.2 |
| 09 | Registry and ramp crash recovery | BROKEN | W4-L48-F009 | 48 | req.9 | 3.4, 3.5 |
| 10 | Withdrawal asset binding | BROKEN | W4-L48-F010 | 48 | req.10 | 3.3 |
| 11 | Human API error contract | BROKEN | W4-L48-F011 | 48 | req.11 | 2.4 |
| 12 | Executed qualification evidence | MISSING | W4-L40-F007 | 40 spec-status-evidence-truth | req.12 | 1.1, 5.1–5.4 |
| — | Verdict and must-statically-clarify | — | W4-L48-F001 | 48 | req.13 | 1.1, 4.2, 5.3, 5.4 |

## Evidence anchors per item (at revision d5ff7e26)

**01 Bootstrap.** `platform/docs/content/install.md:21-26` shows
`layerx emulator up --listen 127.0.0.1:9402` and
`layerx environment use emulator --endpoint http://127.0.0.1:9402 --network-id 402`.
`platform/cli/src/main.rs` `EmulatorCommand::Up` requires `sequencer_seed_file`;
`EnvironmentCommand::Use` requires `--endpoint`, `--network-id` and
`--sequencer-trust-anchor` (32-byte hex) together. The published sequence
cannot run.

**02 Programs snapshot.** `src/modules/programs/artifact.c:80-132`
(`lxp_programs_artifact_store` / `_open` persist kernel blobs);
`src/state/lxp_state_root.c:471-479` (`lxp_state_subtree_root` commits blob
keys, bytes and lengths); `src/state/lxp_snapshot.c:136-170` (`snapshot_size`
omits blobs), `336-368` (`lxp_snapshot_write` has no blob section), `683-719`
(`lxp_snapshot_load` publishes without restoring blobs).

**03 LNI auth.** `cmd/layerxd/lxp_daemon_lni.c:644-699` (`send_submit` queues
and acknowledges without signature verification);
`cmd/layerxd/lxp_daemon_main.c:223-257` (`lxp_daemon_submit` checks length,
accepting state and capacity only); `cmd/layerxd/lxp_daemon_batch_wal.c:226-237`
(`validate_canonical_items` defers signature validation);
`src/protocol/lxp_activity_signature.c:5-18` (`lxp_activity_verify_signature`
exists).

**04 Revocation.** `agent/crates/layerx-agentd/src/session.rs:47-70`
(`Token::authorize` omits open and revocation checks); `tenant.rs:202-264`
(`tenant::resolve` calls it without the `SessionRegistry`); `session.rs:203-226`
(close persists `open=false` but invalidates no token);
`session_revocation.rs:60-81` (`apply_revocation` closes records without
touching tokens).

**05 Evidence.** `human/crates/layerx-human-service/src/server/production_reads.rs:143-150`
(`evidence_get` returns receipt-verified from the decoded cache);
`activity/export.rs:221-241` (`EvidenceBundle::verify` exists, unused there);
`server/http.rs:224-248` (`HttpServer::handle`).

**06 Checkpoint.** `src/protocol/lxp_protocol.c:9-20` (`domain_tags`, v1);
`agent/crates/layerx-wire/src/hash.rs:69-70` (`Domain::CheckpointCertificate`,
v1); `contracts/libraries/CanonicalCheckpoint.sol:7-11` (`CHECKPOINT_DOMAIN`,
v2, header prefix `hex"000217010f"`, commit 093d2b90);
`agent/crates/layerx-proof/src/checkpoint.rs:404-421` (`verify_certificate`
checks only `attested_at_ms != 0`); `contracts/CheckpointRegistry.sol:170-176`
(`registerCheckpoint` enforces `attestedAt` within the header-relative window).
`include/layerx/lxp_protocol.h` `LXP_PROTOCOL_VERSION = 2`; hosted configmap
`lxp-wire-protocol-version: "2"`.

**07 Hosted.** `platform/hosted/testnet/deployment.yaml:88-96`
(`LAYERX_TESTNET_GATEWAY_URL=https://layerx-gateway.layerx-testnet.svc.cluster.local:9443`);
`platform/hosted/gateway/deployment.yaml:123-126` (Service exposes 443),
`156-173` (NetworkPolicy ingress only from `ingress-nginx` on 9443);
`platform/hosted/testnet/src/main.rs:399-440` (probe and status report only
core, gateway, Paxeer and release compatibility).

**08 Artifacts.** `platform/release/registries.kvx:1-72` (seven ecosystems,
skeleton entries); `.github/workflows/platform.yml:270-291` (publishes four npm
middleware packages only); `platform/release/src/main.rs:64-70` (validates
declarations only); `Makefile:2897-2898` (`programs-test` broader than the
workflow); `.github/workflows/agent.yml:33-66` (sanitizers and long fuzz
schedule-only); `tools/ci/replay-matrix.sh:4-12` (non-x86 replay gated on
`LXP_REQUIRE_NON_X86=1`).

**09 Recovery.** `platform/hosted/registry/src/journal.rs:43-75`
(`FileDeploymentJournal::proofs` rejects unequal sets), `101-120` (`append`
writes record then proof with no commit marker);
`platform/ramps/toolkit/src/journal.rs:511-542` (`Journal::append` syncs the
callback before apply and does not remove it on failure), `607-652`
(`ProviderCallbackApplied` mutates maps before later validation).

**10 Asset binding.** `src/modules/asset/lx_asset_custody.c:277-317`
(`lx_asset_withdraw_request` checks the asset), `320-358`
(`lx_asset_withdraw_settle` uses the supplied asset without equality);
`src/modules/bridge/lxp_bridge_withdraw.c:143-203`
(`lxp_bridge_withdraw_finalize` forwards an independently supplied asset).

**11 Human API.** `human/schema/human-api/v1.kvx:16-23` (encoding error rule),
`102-103` (`record.ResponseEnvelope` fields `ok`, `result`, `trace` — no
`error`); `human/apps/web/src/api/generated/index.ts:4032-4038` (`execute`
reads `response.error`); `human/apps/web/src/app/api/explorer/verify/route.ts:79-90`
(returns 429 with Retry-After); `human/apps/web/src/explorer/verifier.tsx:41-55`
(maps non-503/409 errors to refused).

**12 Qualification.** `spec/layerx-platform/qualification.kvx:2563-2617`
(observations 38.5.1, 38.5.3, 38.5.6, 38.5.8 describe registry isolation, mTLS,
cgroup, crash and provisioning behaviour as not executed).

## Lane 48 cutline, and how the owner's bar changes it

- **Must fix** → req.1–req.11 (all repaired in waves 1–4). Unchanged.
- **Must statically clarify** → req.12 (evidence rungs, ledger), req.13 (one contract), observation.0.4 (platform 38.5 links; the pending 33.7 reconciliation stays with the platform feature). Unchanged.
- **Must prove** → widened from one narrow journey to the whole surface: req.12.4, tasks 5.1–5.3 (focused negative and fault tests; the `beta-qualify` runner gate over every functional surface with the in-repo driver's adoption, Programs, interop and multichain evidence; clean-profile e2e locally and against hosted; restart and unknown outcome; cross-language vectors per published artifact; source-bound artifact manifests for all seven ecosystems).
- **Owner / external gate** → replaced by `decision.owner_inputs`: the owner supplies beta infrastructure inputs (beta cluster, test-network keys, sandbox credentials, Paxeer testnet, publish tokens, macOS runner, Android toolchain) and the go decision. Production certification of peers, DNS, TLS and KMS is outside the beta.
- **Can defer** → **not adopted.** The owner requires the entire system functional. Hosted Programs operations, seven-ecosystem publication, multichain settlement, ramps, custody-heavy human journeys, mobile and framework breadth are all required at their beta rung. The only things outside the beta are `decision.polish_boundary`: UI polish, visual regression, accessibility, usability, performance budgets and soak; external security audit; production infrastructure and certification.

## Stop conditions (lane 48, widened)

Clean bootstrap failure; non-independent success; unresolved unknown outcome;
ineffective revocation; non-transitive readiness; missing artifact identity;
any surface below its beta rung (replacing the production-only external peer
certification); unverified scope closure. Each maps to a row of the
stop-condition table in `tasks.notes.md` and to req.12.6.

## Six-day sequence (lane 48) to waves

The ordering is kept; the duration is not a commitment under the full-surface bar.

| Day | Lane 48 | Wave |
|---|---|---|
| 1 | freeze, contract, bootstrap | 1 |
| 2 | admission, revocation, evidence, API | 2 |
| 3 | Programs, checkpoint, withdrawal, registry, ramp, hosted topology | 3 (plus 3.7 beta cluster bring-up) |
| 4 | build and focused qualification | 4.1–4.3 artifact truth and the runner gate; 5.1 focused gates |
| 5 | clean e2e, independent receipt verification, restart and unknown outcome, hosted | 5.2 beta stack + `beta-qualify`; 5.3 journey, restart, vectors |
| 6 | aggregate and cross-language gates, bind artifacts, go/no-go | 5.4 report and contract |

## Qualification machinery already in the repository

`tools/qualification/release_runner.py` defines the `platform-qualify` gate
(human journeys, platform build/lint/test, SDK conformance, middleware, docs
samples, hosted smoke, agent install, emulator conformance, real agent-framework,
iOS and Android integrations, Programs, interop, migration testnets, ramp
sandboxes, release check) and records `source_revision`, a dirty-tree
`source_identity`, per-command logs and validated external evidence under
`build/qualification/<gate>/`. It requires an external driver executable
(`LAYERX_QUALIFICATION_DRIVER`, digest-pinned) for the adoption, Programs,
interop, multichain, ui and usability evidence; **no driver exists in the
repository** (observation.0.5). Task 4.3 adds one for the four functional gates
and a `beta-qualify` gate that omits the polish gates; the feature ledger points
at the runner's `status.json` and `report.json` rather than duplicating them.

## Sensitive data

The audit never read `.env` or `.env.*`, and neither does this feature. No
requirement, task, test or script in this feature reads, prints, copies, hashes
or infers those files. Hosted secrets are referenced by Kubernetes secret key
name only.
