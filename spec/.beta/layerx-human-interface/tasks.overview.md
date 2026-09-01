## How to execute this plan

This plan builds the Human Control Plane for LayerX: the surface people touch,
sitting on the agent layer, which sits on the C17 core. It delivers the custody
service, the intent compiler, the approval seam in the agent contract, the
Paxeer boundary client, the explorer index, the human-api contract, and one web
application with two native shells.

**Two rules govern every task below.** The surface exposes exactly five ideas —
log in, add money, move money, manage agents, see what happened — and "Done" is
rendered only when a verified LayerX receipt or a Paxeer finality proof backs
it. A task whose output would let a screen claim something the protocol did not
do is not done, however green its `verify_cmd` looks.

**The plane never grows a second write path.** Every mutation is a typed intent
compiled by `layerx-intents`, disclosed, signed by the custody service and
submitted through the agent layer's existing pipeline. The only agent-contract
change this feature makes is the additive `approval.*` module, built in wave 1.

**Work is driven by `cg spec next`.** Run it; it resolves the wave graph and
returns the next eligible task. Exactly one task is `in_progress` at a time.
A task is `done` only when its `verify_cmd` genuinely passes on real code paths:
journey suites run against a real node, a real `layerx-agentd` and a Paxeer
test network — a test that passes against a fake proves nothing here.

**Waves express real dependencies.** The approval seam and the workspace gates
come first; intents and the contract before custody; custody and the Paxeer
boundary before journeys; journeys before surfaces; everything before
qualification. Tasks within a wave are independent and may be worked in any
order.

## The eight waves

| Wave | Delivers |
|---|---|
| 1 | Foundations: the `human/` workspace, dependency and boundary policy, the copy catalog and its lint, design tokens and the UI rule gates, the test and state-matrix harness — and the `approval.*` module in the agent contract with regenerated SDKs. |
| 2 | `layerx-intents` — the typed intent vocabulary, compilation to canonical bytes, golden vectors, the disclosure round-trip gate, the single-payload-authority gate — and the `human-api` schema with its generated TypeScript client. |
| 3 | Custody: the principal-scoped store, passkey auth and step-up, the KMS keystore and custody signer, onboarding orchestration, session-key provisioning, the wallet-binding journey, the audit chain — and `layerx-paxeer-client` for finality, deposits, claims and exits. |
| 4 | Journeys: the route resolver, the durable journey engine, deposit, withdrawal and claim, emergency exit, move-money — and the managed-agent journeys plus approvals and notifications in the service. |
| 5 | Evidence and reads: the rebuildable explorer index, the unified activity feed, entry detail, statement and evidence exports, explorer lookups and the verifier. |
| 6 | The application foundation: the two planes, SSR-safe shell selection, the component kit in both shells, the state-matrix machinery and error surfaces, the accessibility and theme foundation, the performance machinery. |
| 7 | The application surfaces: onboarding, home and move-money, the custody journeys, agents, approvals and notifications, activity — plus settings, security, wallet binding, support and the public explorer plane. |
| 8 | Qualification: every journey against the real stack in both shells, the hostile-plane no-fabrication suite, the journey fault-injection matrix, the UI, copy and accessibility gates, performance and soak, the usability gate, and the release-gating qualification report. |

Waves 1 through 3 are this feature's trust spine: the seam that lets humans veto
agent spending, the compiler that makes disclosures provable, and the custody
that keeps signing power where it belongs. Everything above them either moves
money or claims something about money, and both depend on the spine being real.
