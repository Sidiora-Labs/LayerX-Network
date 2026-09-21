<!-- Generated from platform/docs/capabilities.kvx by platform/docs/build/build_site.py. Do not hand-edit. -->

# Enforcement reference

Every capability in this documentation carries the layer that actually enforces it. A lower layer never implies a higher one. This page is generated from the same registry the site build checks, so a documented capability without a label fails the build.

## What each label means

| Label | Meaning |
|---|---|
| `protocol` | The LayerX state machine refuses the violating transition. The guarantee survives every component above it, including a hostile client, a hostile daemon and a hostile gateway. |
| `agent-layer` | layerx-agentd enforces it while it is in the request path. Bypassing the daemon bypasses the restriction; it is not a protocol guarantee. |
| `service` | A LayerX service process enforces it - layerx-human-service, the settlement service or the middleware you deploy. It binds callers of that service only. |
| `hosted-surface` | The hosted surface enforces it - the gateway, the faucet or the developer dashboard. It is an operational control on the hosted deployment, not a property of the protocol. |

## Enforced by protocol

| Capability | Where you meet it | What is guaranteed |
|---|---|---|
| Atomic settlement | Every payment you commit | An activity applies completely or not at all. There is no partial-transfer transition, so no client, daemon, gateway or middleware above the protocol can produce a half-applied payment. |
| Conserved supply | Every balance you read | The kernel transfer primitive is the only path that mutates a balance. A state change outside an authenticated 402LXP transfer aborts the transition, so total supply is conserved by construction rather than by reconciliation. |
| Deterministic program execution | Every node that replays your program | The programs runtime denies clocks, networking, filesystem, floating point, threads, randomness and ambient authority, and records the runtime and ABI version in the receipt. Replay executes under the recorded version, so an upgrade never rewrites history. |
| Interop adapters hold no protocol authority | x402, AP2, UCP, A2A, MCP and card, bank or RTP flows | An adapter translates an external protocol at the edge and then submits an ordinary authenticated activity. It never gains authority the submitting principal lacks, so a compromised adapter cannot produce a settlement the protocol would refuse. |
| Offline receipt verification | layerx receipt verify, and the verify APIs in every SDK | A receipt verifies from its own bytes against an authorised batch header: canonical encoding, protocol invariants, root chain and signature. Verification needs no LayerX node, gateway or hosted service, so a settlement claim can be checked by someone who trusts none of them. |
| Programs never write balances | Every LayerX Program you deploy or call | Guest program code receives no balance-writing authority. Its only monetary exit is a typed 402LXP transfer request bound to the invoking activity's authority and applied by the kernel primitive, so a hostile program cannot move money its caller could not move. |
| Protocol budget ceilings | Agent spend limits backed by the budget module | A budget is a protocol object in the budget module. Spending past a funded budget is refused by the transition function, so the ceiling holds even against an agent runtime that has been fully compromised. |
| Replay refusal | Every retry your code makes | Each principal's activity sequence is consumed exactly once. Resubmitting an already-applied activity is refused by the transition function, so a retried network call cannot pay twice. |
| Typed program failure with rollback | Every program activity receipt | A program that traps, returns a malformed result or exhausts its metered budget has every state write discarded, its sequence consumed and its metered fee charged, and the receipt carries the typed failure. A program cannot stall or crash a node. |

## Enforced by agent-layer

| Capability | Where you meet it | What is guaranteed |
|---|---|---|
| Agent tenancy isolation | Sessions opened for each agent | Each session is scoped to its agent's tenancy and the daemon refuses cross-tenancy reads and writes. It is an agent-layer boundary, not a protocol one. |
| Approval holds | The approval inbox and the approval.* operations | The agent layer can hold an activity for human approval, carrying the held activity's structured disclosure, its canonical-bytes digest, its hold reason and a deterministic expiry. The hold exists while the daemon is in the request path. |
| Capability attenuation | capability.create, capability.attenuate and capability.revoke | layerx-agentd refuses a request that falls outside the presented capability and lets you narrow or revoke one at any time. This binds callers that go through the daemon; a principal reaching the protocol another way is bound by protocol budgets, not by this. |
| Honest verification levels | Every protocol fact an SDK returns | Every successful response carries its verification status, and a shortfall is reported rather than silently downgraded. No layer reports a level its evidence does not justify. |
| Unknown is a real outcome | submit, track and wait | When a submission cannot be classified, the answer is Unknown - a terminal-pending state you resolve by tracking, never a guessed success or a guessed failure. |

## Enforced by service

| Capability | Where you meet it | What is guaranteed |
|---|---|---|
| Done means verified | Journey states and the evidence references they carry | The human service reports a journey as done only against a verified LayerX receipt or a Paxeer finality proof, and every figure it renders travels with the verification level behind it. |
| Exactly-once fulfilment | The fulfilment repository you hand to the middleware | A repeated payment with the same idempotency key returns the stored fulfilment, and a repeat carrying a different request digest is refused as a conflict. The guarantee is only as durable as the repository you supply. |
| Idempotent money moves | The Idempotency-Key header on every money-moving mutation | layerx-human-service requires the header on every mutation that could move money and returns the original journey when the request repeats, so a retry after a timeout cannot create a second effect. |
| Quote then commit | move.quote and move.commit | A quote states the fee estimate with its ceiling, the arrival expectation and any irreversibility before anything moves, and the commit turns exactly that quote into a journey. The human service enforces the ordering; callers of that service are bound by it. |
| Receipt-gated resource release | The seller middleware and the framework integrations you deploy | The middleware releases your protected resource only after it verifies the receipt covering the request against an authorised batch. It binds requests that reach your service through the middleware. |
| Refusal to publish a secret | The mobile bindings and the Next.js bundle scanner | The mobile configuration accepts publishable values only, and the Next.js scanner fails a build whose client bundle contains a declared secret. Client code holds a brokered ephemeral session token, never a long-lived key. |
| Verified, replay-protected webhooks | The webhook endpoint the integration mounts in your process | Deliveries are verified by signature against your configured public keys, refused when stale, and claimed under a lease so a redelivery is processed once. Your delivery store decides whether that survives a restart. |

## Enforced by hosted-surface

| Capability | Where you meet it | What is guaranteed |
|---|---|---|
| API keys, usage and request logs | The developer dashboard | Key issuance, rotation, usage counters, request logs and webhook delivery logs are controls of the hosted deployment. They bind traffic arriving through the hosted gateway. |
| Beta faucet funding | The beta faucet | The faucet decides who may draw test funds and how much. Test funds are ordinary protocol balances once issued; the eligibility rule lives entirely in the hosted surface. |
| Honest degradation reporting | The status page | The status page distinguishes gateway, network, core and Paxeer-side degradation instead of collapsing them into one indicator, so you can tell whose outage you are seeing. |
| Hosted rate limits | The gateway response to a burst | The gateway refuses excess traffic with a rate-limit error carrying honest retry timing. It is an operational control on the hosted deployment, not a property of the protocol. |
| Scheduled beta resets | The published beta reset calendar | The hosted beta network is reset on a published schedule and every balance, agent and receipt from the previous epoch is discarded. Nothing on the beta network is durable and no protocol rule promises otherwise. |
