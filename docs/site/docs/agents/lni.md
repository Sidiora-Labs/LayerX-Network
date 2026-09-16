# LayerX Node Interface (LNI)

`layerx-agentd` reaches the C17 core only through the LayerX Node Interface:
a versioned, canonical-binary request, response, and streaming protocol
carrying canonical LayerX bytes plus the proofs required to verify them
(`spec/.beta/layerx-agent-interface/spec.kvx`, decision `boundary` and
requirement 2).

Unix domain socket is the default transport. TCP with mutual TLS is permitted
for remote deployment. The agent workspace must not open the node SQLite
projection, read the append-only log files, or bind struct layouts from
`include/layerx/`.

The platform and beta specifications refer to this document as
`spec/layerx-agent-interface`. The checked-in file is
`spec/.beta/layerx-agent-interface/spec.kvx`.

## Handshake

Every connection begins with `NodeInfo`. The layer exchanges node-interface
version, `protocol_version`, `network_id`, node role, chain head sequence,
latest sealed batch number, latest finalised checkpoint, and the sequencer
public key currently authorised. It refuses a `network_id` or
`protocol_version` it was not configured for, and refuses a major
node-interface version it was not built against.

Unknown capabilities are not emulated. Requests that need a missing capability
fail as unavailable.

## Messages

The design body (`spec/.beta/layerx-agent-interface/design.body.md` §5.2)
lists these messages:

| Message | Carries |
| --- | --- |
| `NodeInfo` | Versions, network, role, heads, authorised sequencer key, capabilities |
| `Submit` | Signed activity bytes → admission acknowledgement |
| `GetReceipt` | Receipt bytes by activity id, idempotency key, or sequence |
| `GetAccount` | Account state bytes and optional state inclusion proof |
| `GetHistory` | Canonical activity, receipt, and event bytes over a sequence range |
| `GetBatchHeader` | Batch header bytes and sequencer signature |
| `GetCheckpoint` | Certificate, guarantor signatures, settlement reference |
| `GetProof` | Activity and state inclusion proofs |
| `FetchAvailability` | DA chunks and chunk inclusion proofs |
| `SubscribeEvents` | Ordered event records from a cursor |

Admission is not execution. The client considers an activity resolved only when
a core-produced receipt has been retrieved and verified.

## Hosted path

The hosted TLS surface that submits signed activities onto node LNI is
[Hosted agent boundary](../platform/agent-boundary.md). That binary is not
`layerx-agentd`. The disposable beta cluster does not start agentd; it
forwards the agent-boundary Service
([Testnet quickstart](../overview/quickstart.md)).

## Pre-queue authentication (beta)

The beta specification (`spec/layerx-beta/spec.kvx`, requirement 3) requires
signature and authority verification before queue insertion. An LNI admission
acknowledgement means the activity was authenticated and durably queued.
Unauthenticated submissions must not consume sequencer queue capacity. Batch
and WAL validation keep verifying independently.

That repair is a beta requirement. Treat it as specified behaviour of the
native submit path, qualified under `spec/layerx-beta`.
