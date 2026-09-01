# The LayerX Node Interface (LNI)

The boundary between `layerx-agentd` and the C17 core. This document specifies
the message set, the framing, the handshake, the version rules and the
prohibitions. It is the reference for `[req.2]` and for task group 6.

---

## 1. Why the boundary exists

The agent layer could read the node's SQLite projection directly. It is faster to
build and it is wrong three times over:

1. **It couples release trains.** The projection schema is a node implementation
   detail that the node is entitled to change. Reading it makes every projection
   refactor an agent-layer outage.
2. **It produces unverifiable answers.** A projection row carries no sequencer
   signature, no inclusion proof and no checkpoint certificate. A balance read
   that way cannot be reported at any verification level above `Unverified`, so
   it cannot honestly be reported as a protocol fact at all.
3. **It starts the slide into a second consensus implementation.** Once state is
   read directly, the next convenient step is computing what the state *should*
   be — and now there are two implementations of execution that will disagree.

LNI exists so that none of those steps is available.

---

## 2. Shape

LNI is a versioned request, response and streaming protocol. Its distinguishing
property is that it does not restate protocol structures: payloads carrying
protocol data are **opaque canonical LayerX bytes** plus the proof material
required to verify them. A second encoding of an activity or a receipt would be a
second chance for the two sides to disagree about what a value is.

```text
+-----------------------------+          +-----------------------------------+
|  layerx-client              |   LNI    |  layerxd                          |
|  - framing                  | <------> |  - ingress / history / DA service |
|  - handshake                |  canon.  |  - checkpoint + proof service     |
|  - verification (layerx-    |  binary  |                                   |
|    proof) on every response |          |                                   |
+-----------------------------+          +-----------------------------------+
```

Verification happens on the client side of the boundary, always. The node is a
source of bytes and proofs, not a source of truth-by-assertion.

---

## 3. Message set (v1)

The normative, machine-readable source is `agent/schema/lni/v1.kvx`. The
`layerx-client` schema test checks every name, capability, and literal golden
encoding below against that artefact, making this document a checked projection
of the schema rather than a second authority.

| Operation | Schema messages | Request carries | Response or stream carries |
|---|---|---|---|
| Node info | `NodeInfoRequest`, `NodeInfoResponse` | empty selector | LNI version, protocol version, network id, role, head sequence, latest sealed batch, latest finalised checkpoint, authorised sequencer keys, capability list |
| Submit | `SubmitRequest`, `SubmitResponse` | signed activity bytes | admission acknowledgement or rejection result code and evidence |
| Receipt lookup | `ReceiptLookupRequest`, `ReceiptLookupResponse` | activity id, idempotency key or global sequence | receipt bytes and verification context, or canonical absence marker |
| Account read | `AccountReadRequest`, `AccountReadResponse` | account id, asset, optional root selector, proof request flag | account state bytes and state inclusion evidence |
| History range | `HistoryRangeRequest`, `HistoryItem`, `HistoryEnd` | sequence range or cursor, filter, page bound | canonical activity, receipt and event bytes in sequence order, evidence, next cursor |
| Batch header | `BatchHeaderRequest`, `BatchHeaderResponse` | batch number or checkpoint id | batch header bytes and authorised sequencer evidence |
| Checkpoint | `CheckpointRequest`, `CheckpointResponse` | checkpoint id or batch number | checkpoint bytes, certificate, guarantor signature set, threshold, settlement reference |
| Proof bundle | `ProofBundleRequest`, `ProofBundleResponse` | activity id or account id, target root selector | opaque target bytes, activity inclusion proof, state inclusion proof |
| Availability fetch | `AvailabilityFetchRequest`, `AvailabilityChunk`, `AvailabilityEnd` | checkpoint id / batch / sequence range / activity id, class selector | DA chunks with inclusion proofs and final class report |
| Event subscribe | `EventSubscribeRequest`, `EventRecord`, `EventGap`, `EventHeartbeat` | start cursor, filter | ordered event records, gap markers, heartbeat |
| Preparation state | `PreparationStateRequest`, `PreparationStateResponse` | actor DID | one atomic node-owned snapshot containing network id, account sequence, protocol timestamp, observed head and state root, kernel epoch, and module registry |
| Finality evidence registration | `FinalityEvidenceRegisterRequest`, `FinalityEvidenceRegisterResponse` | verified checkpoint bytes and evidence context | checkpoint id, batch number, and durable registration-record digest |
| Shared failure | `ErrorResponse` | not applicable | typed boundary failure distinct from core rejection and proof failure |

Every response that carries protocol data carries it as canonical bytes. Every
response that supports a verification level carries the material for it, or says
plainly that it cannot.

---

## 4. Framing and transport

- **Framing.** Length-prefixed canonical binary, with an explicit maximum message
  size. An over-long frame is rejected before any allocation sized by it.
- **Default transport.** Unix domain socket, file-permission scoped.
- **Remote transport.** TCP with mutual TLS. Identical message semantics; the
  transport changes who may connect, never what the messages mean.
- **Multiplexing.** Streams are multiplexed such that a slow reader on one stream
  cannot starve submission delivery on another connection or stream.
- **Deadlines and limits.** Every call carries a deadline; every connection
  carries limits on concurrent streams and in-flight bytes.
- **Errors.** Transport errors are typed distinctly from core rejections and from
  verification failures, and are never mapped onto one another.

The framing decoder is fuzzed, like every other decoder in the workspace.

---

## 5. Handshake and versioning

Every connection begins with `NodeInfo`. The client refuses to proceed when:

| Condition | Behaviour |
|---|---|
| LNI major version differs | Refuse with an incompatibility error naming both versions. No best-effort interpretation. |
| LNI minor version is newer on the node | Proceed using the known subset; unknown capabilities are simply not used. |
| `network_id` differs from configuration | Startup failure, not a per-request failure. |
| `protocol_version` outside configured support | Startup failure. |
| Advertised sequencer keys do not cover held receipts | Refuse to verify those receipts against unadvertised keys; report the gap. |

**Version rule.** Within a major version, changes are additive only: new
messages, new optional fields, new capabilities. Golden encoding vectors are
committed for every message, so any encoding change appears as a reviewable diff
rather than as a silent behaviour change.

LNI v1.0 encodes `major:u16be`, `minor:u16be`, `message_tag:u16be`,
`correlation_id:u64be`, then a `u32be`-length canonical payload and a
`u32be`-length proof-material byte string. The schema records a literal vector
for every request, response, and stream tag. Tags are never reused within a
major version.

---

## 6. Capabilities and gaps

A node advertises a capability list. The client computes the intersection with
what it knows and operates inside it. Two rules make this honest:

- A request needing a capability the node does not advertise fails as
  `Unavailable`, naming the capability.
- The client **never emulates** a missing capability. It does not reconstruct a
  proof, synthesise a certificate, or assemble an answer from partial data.

The capability gap report enumerates every capability this specification requires,
whether the connected node exposes it, and what happens when it does not. It is
surfaced through daemon status, through the CLI, and in the qualification report,
so a release states plainly what was unavailable. Gaps are closed in the protocol
feature, not worked around here.

---

## 7. The optional C ABI transport

For embedding, LNI may also be carried over a stable C ABI. It is permitted only
under these constraints:

- **Opaque handles and byte buffers only.** No struct layout crosses the ABI. The
  agent side never learns the size, field order or padding of any core type.
- **Its own version.** The ABI carries a version negotiated with the same refusal
  rules as the socket handshake; a mismatch is a refusal, never undefined
  behaviour.
- **Unsafe is enumerated.** Every `unsafe` block it requires lives in one module
  listed in `unsafe-allowlist.toml` with its safety argument written beside it.
- **Same conformance.** The boundary suite runs over the ABI transport as well as
  the socket transport, and asserts identical observable behaviour.

---

## 8. Prohibitions, and how they are enforced

| Prohibited | Enforcement |
|---|---|
| SQLite driver dependency anywhere in `agent/` | dependency scan in `make agent-check-boundary` |
| Reading node log segments, projection files, snapshots | source scan for node-private path patterns |
| Binding `include/layerx/` struct layouts | `repr(C)` mirror detection against the stable-ABI allowlist |
| Linking the C core outside the published ABI | dependency and link scan |
| A second crate opening a core connection | only `layerx-client` may; checked structurally |

These are build failures, not review conventions. The gate is built in wave 1, so
no later task can drift across the line quietly.

---

## 9. Behaviour when the node is not healthy

| Node state | Layer behaviour |
|---|---|
| Unreachable | Degraded read mode: serve only values still verifiable, marked with the head and checkpoint they are relative to. Refuse preparation needing live state. Refuse submission acknowledgement. Keep resolving unknown submissions when connectivity returns. |
| Behind | Serve reads with explicit staleness. Do not present a stale value at a level its evidence no longer supports. |
| Halted / emergency / data-unavailable | Propagate into health and read responses. Stop advertising finality that cannot be verified. |
| Equivocating or serving non-verifying bytes | Treat as verification failure; retain the bytes and the mismatching commitment as evidence; record in the audit trail. |

In none of these states does the layer invent an admission decision on the node's
behalf, or upgrade a value's verification level to compensate for missing
evidence.

---

## 10. Conformance

The boundary suite starts a real `layerxd` from this repository with a genesis
manifest and exercises every message, every error case, streaming behaviour,
pagination, version negotiation and capability reporting. It asserts that
responses carrying protocol data re-hash to the identifiers they claim — so the
suite tests the boundary itself rather than the client's belief about it.

An in-process substitute for the node does not satisfy this suite, in any task,
at any point in the plan.
