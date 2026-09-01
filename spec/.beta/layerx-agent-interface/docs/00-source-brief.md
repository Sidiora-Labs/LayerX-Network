# Source brief — LayerX Agent Interface

This document is the provenance record for `spec/layerx-agent-interface/spec.kvx`.
Every normative statement in that file traces back to the brief captured here, to
the protocol feature it sits on top of (`spec/layerx-protocol/`), or to a locked
decision recorded in `[decision]`.

---

## 1. The request

Build the Rust interaction layer for LayerX. It provides:

- Agent identity, sessions, capabilities, budgets, and policy enforcement.
- Activity preparation, local signing, submission, and receipt tracking.
- Verified balances, history, checkpoints, proofs, and DA retrieval.
- Streaming events and durable subscriptions.
- MCP servers with scoped read/write tools.
- A Rust SDK plus generated TypeScript and Python SDKs.
- Rate limits, idempotency, audit trails, observability, and tenant isolation.

## 2. The critical rule

> This layer never invents or directly changes protocol state. Every mutation
> becomes canonical signed LayerX bytes, and every claimed result must be backed
> by a core-produced receipt or proof.

This is not a design preference. It is the property that makes the layer safe to
put in front of autonomous agents, and it is the first requirement in the spec
(`[req.1]`). Two obligations fall out of it:

1. **Write direction.** The only way this layer can change anything is to build a
   canonical LayerX activity, have it signed by a key the LayerX state machine
   binds to the acting identity, and submit those exact bytes to the core. The
   layer holds no state-mutating authority of its own, has no privileged path
   into the ledger, and cannot produce an effect the protocol did not execute.
2. **Read direction.** Anything this layer reports as an outcome — a balance, a
   transfer result, a settled invoice, a position — is either core-produced bytes
   whose sequencer signature, inclusion proof or checkpoint certificate verified
   locally, or it is labelled as unverified and never presented as a result. A
   value the layer computed itself is a projection, not an answer.

## 3. Crate layout

```text
agent/
  crates/
    layerx-types        # canonical domain types, mirrors of the wire structures
    layerx-wire         # canonical binary codec, byte-exact with the C core
    layerx-crypto       # key custody, signing, session keys, remote signers
    layerx-proof        # receipt, inclusion, state, checkpoint and DA verification
    layerx-client       # boundary client: submit, track, verified reads, streams
    layerx-agent-api    # the agent-facing service contract (schema + types)
    layerx-agentd       # the daemon that implements the contract
    layerx-mcp          # MCP servers exposing scoped read and write tools
    layerx-sdk          # Rust SDK, plus the generator for TypeScript and Python
```

## 4. The core boundary

> `layerx-agentd` should communicate with the core through a stable protocol
> boundary, not by reaching into SQLite or binding directly to internal C
> structs. That preserves independent upgrades and prevents the agent layer from
> becoming a second consensus implementation.

Three prohibitions follow, and the spec enforces each of them with a CI gate
rather than a convention:

- **No SQLite.** The projections under `migrations/` are rebuildable views owned
  by the node. They are not an API. The agent layer never opens the node's
  database file, and no crate in `agent/` may depend on a SQLite driver.
- **No internal C structs.** The agent layer never binds `struct lxp_account`,
  `struct lx_log_record_header`, or any other layout from `include/layerx/`.
  Binding a layout would couple the two release trains together and would make a
  core refactor a silent memory-safety incident in Rust.
- **No second consensus implementation.** The agent layer decodes, verifies and
  re-hashes; it does not execute transitions, does not apply transfers, does not
  compute state roots from activities, and does not decide whether an activity
  would have been accepted. It asks the core and verifies the answer.

The boundary itself is named in this spec as the **LayerX Node Interface (LNI)**:
a versioned, canonical-binary request/response and streaming protocol served by
`layerxd`, carrying opaque canonical byte payloads plus the proofs needed to
verify them. `docs/node-boundary.md` specifies it.

## 5. What the protocol feature already fixes

The agent layer is downstream of `spec/layerx-protocol/`, and inherits rather
than redefines:

| Inherited | Source |
|---|---|
| The activity envelope and its twelve fields | `layerx-protocol` req 2 |
| Canonical binary encoding, domain tags, merkle rules | req 6 |
| DID identity, session keys, capability grants, revocation | req 3 |
| 402LXP as the single financial doorway, `SEND` / `RECEIVE` | reqs 7, 8, 9 |
| Account and subaccount namespaces | req 10 |
| `402LXPReceipt` fields and offline verification | req 12 |
| Batch headers, sequencer signatures, checkpoint certificates | reqs 21, 22 |
| Data availability commitments and the retrieval interface | req 23 |
| Result-code taxonomy | design section 18 |

Where this spec restates one of those, it restates it as a *consumer obligation*
— what the Rust layer must do with it — never as a redefinition. If the two ever
disagree, the protocol spec wins and the agent spec is the defect.

## 6. Open questions resolved before implementation

These were ambiguous in the brief and are locked in `[decision]`:

- **Rust is allowed here, and only here.** The protocol runtime stays C17. The
  agent layer is Rust in a separate workspace under `agent/`, built and released
  independently. No Rust links into the consensus runtime.
- **Transport.** LNI over a Unix domain socket is the default and the normative
  transport. TCP with mutual TLS is permitted for remote deployment. An optional
  stable C ABI shim is permitted as an alternative transport only under the
  opaque-handle, canonical-bytes-only rules in `docs/node-boundary.md`.
- **`layerx-agentd` is not a signer by default.** It prepares and submits. It
  signs only when an operator has explicitly provisioned a protocol-level session
  key to it, with a declared scope and expiry that the state machine enforces
  independently of anything `agentd` believes.
- **Local policy is a restriction, never a grant.** The policy engine, budgets
  and capability scopes in this layer can only refuse. They can never authorise
  an activity the protocol would otherwise reject, and their approval is not
  evidence of anything.
- **Generated SDKs are generated.** TypeScript and Python SDKs are emitted from
  the same contract schema as the Rust SDK, and hand-edits to generated output
  are a build failure, so three SDKs cannot drift into three dialects.
- **Unknown is a first-class outcome.** A submission whose fate the layer cannot
  determine is reported as `unknown` and resolved by receipt lookup. It is never
  reported as success and never reported as failure.
