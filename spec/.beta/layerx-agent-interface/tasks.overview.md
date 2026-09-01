## How to execute this plan

This plan builds the Rust interaction layer for LayerX: the surface autonomous
agents touch, sitting on top of the C17 protocol core. It provides identity,
sessions, capabilities, budgets and policy; preparation, signing, submission and
receipt tracking; verified reads, proofs and availability retrieval; streaming and
durable subscriptions; MCP tools; three SDKs; and the operational properties —
limits, idempotency, audit, observability, tenancy — that make it deployable.

**One rule governs every task below.** The layer never invents or directly
changes protocol state. Every mutation becomes canonical signed LayerX bytes
submitted to the core, and every claimed result is backed by a core-produced
receipt or proof that this layer verified for itself. A task whose output would
let the layer report a value the core did not produce is not done, however green
its `verify_cmd` looks.

**The boundary is not negotiable.** `layerx-agentd` reaches the core only through
the versioned node interface. It never opens the node's SQLite projection, never
reads the append-only log directly, and never binds a struct from
`include/layerx/`. Task 1.4 builds the CI gate that enforces this, and it is
built in wave 1 precisely so that no later task can casually violate it.

**Work is driven by `cg spec next`.** Do not pick tasks by reading down the list
and choosing what looks interesting. Run `cg spec next`; it resolves the wave
graph, filters out anything whose dependencies are unmet, and returns the next
eligible task. If it returns nothing, the plan is complete or blocked — diagnose
the block rather than inventing work outside the plan.

**Exactly one task is `in_progress` at a time.** Set it to `in_progress` in
`spec.kvx` before starting, and to `done` only when it genuinely meets the bar, or
back to `pending` with an honest note about why it did not.

**A task is `done` only when its `verify_cmd` genuinely passes** on the real code
path, against real types, with no stub standing in for the thing under test. For
this layer specifically: a test that passes against a fake node proves nothing
about the boundary, and a verification test that passes because the verifier was
short-circuited proves the opposite of what it claims. The boundary suite runs
against a real `layerxd` started from this repository; that is a requirement, not
a preference.

**Waves express real dependencies.** Bytes must be byte-exact before a signature
means anything; verification must exist before any read can be honest about its
level; the boundary must be pinned before the daemon can be built on it. Tasks
within a wave are independent and may be worked in any order.

## The twelve waves

| Wave | Delivers |
|---|---|
| 1 | Foundations: the `agent/` workspace, unsafe and supply-chain policy, the error model, the boundary-purity gate, the test and fuzz harness, and `layerx-types`. |
| 2 | `layerx-wire`: the canonical codec, byte-exact with the C core, with rejection parity, domain-separated identifiers, differential harness and fuzz targets. |
| 3 | `layerx-crypto`: signature verification, the signer abstraction, disclosure-bound signing, the encrypted keystore, remote signers and secret hygiene gates. |
| 4 | `layerx-proof`: receipts, inclusion and state proofs, checkpoint certificates, availability verification, and the verification level lattice with its negative corpora. |
| 5 | The node boundary: the LNI schema and version rules, framing and transports, the handshake and capability intersection, the optional C ABI, and the conformance suite against a real node. |
| 6 | `layerx-client`: connection and head tracking, exact-byte submission, receipt resolution, verified reads, ordered streaming and availability retrieval. |
| 7 | `layerx-agent-api`: the versioned contract schema for identity, capabilities, budgets, the write path, reads and proofs, subscriptions, errors and idempotency. |
| 8 | Authority in `layerx-agentd`: the tenant-scoped store, identity and sessions, capabilities and attenuation, budgets and reconciliation, and the policy engine. |
| 9 | The write and read paths: preparation and disclosure, signing and byte binding, the durable outbox and unknown resolution, verified reads and exports, streaming and durable subscriptions. |
| 10 | Operability: rate limits and backpressure, tenant isolation, the hash-chained audit trail and observability, configuration, degraded modes and the operator surface. |
| 11 | Surfaces: `layerx-mcp` scoped read and write tools, the authored Rust SDK, the generator, the TypeScript and Python SDKs and the cross-SDK parity suite. |
| 12 | Qualification: differential wire conformance, boundary conformance against a real node, the hostile-node no-fabrication suite, fault injection and exactly-once, fuzz and sanitizers, soak, and the release-gating qualification report. |

Waves 1 through 6 are the trust spine. Nothing above wave 6 can be honest until
bytes are exact, signatures cover what they claim, evidence is verified locally
and the boundary refuses what it cannot support — because everything above wave 6
either reports a protocol fact or causes a protocol effect, and both depend
entirely on the spine being real.
