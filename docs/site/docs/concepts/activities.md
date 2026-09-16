# Activities

Every state-changing operation enters as one signed, canonically encoded
activity. The protocol specification (`spec/layerx-protocol/spec.kvx`,
requirement 2) requires the envelope to carry exactly these fields:

`protocol_version`, `network_id`, `activity_type`, `actor_did`, `authority`,
`account_sequence`, `timestamp_bound`, `idempotency_key`, `fee_limit`,
`payload_hash`, `payload`, and `signature`.

A submission that omits a field, repeats a field, or carries an undeclared
field is a malformed envelope. The human read of that design, including the
LXC/1 wire notes, is [Protocol](../protocol/index.md).

## Admission and execution

Requirement 2 of `spec/layerx-protocol/spec.kvx` binds:

- Unsupported `protocol_version` or a `network_id` that is not this chain
  rejects the activity without consuming sequence or charging a fee.
- `payload_hash` must be the domain-separated hash of the canonical payload
  bytes, checked before authority.
- The signature covers the canonical encoding of every envelope field except
  `signature` itself, under a key the state machine currently binds to
  `authority`.
- `account_sequence` must equal the actor's next expected sequence. A lower
  value is a replay; a higher value is a gap.
- `timestamp_bound` is compared to the batch timestamp, never node wall-clock.
- The same `idempotency_key` from the same actor produces at most one economic
  result. A later submission returns the original receipt.
- A computed fee above `fee_limit` rejects the activity without applying
  effects.

Failed activities that are otherwise well-formed and authorised still consume
sequence, still pay the fee, and still occupy a global sequence number.
Module effects roll back; bookkeeping does not
([Protocol](../protocol/index.md), [Modules](../protocol/modules.md)).

## Activity type

`activity_type` is module id in the high 16 bits and type ordinal in the low
16 bits (`include/layerx/lxp_module.h`). The registered modules are
[Modules](../protocol/modules.md). Programs occupy module `9`
([Programs](../programs/index.md)).

Beta envelopes use protocol 3 (`LXP_PROTOCOL_VERSION_STATE_COMMITMENT` in
`include/layerx/lxp_protocol.h`). Occupancy accounting is used by protocol 2
and 3. The C header default `LXP_PROTOCOL_VERSION` remains 2
([Protocol](../protocol/index.md)).

## Public submission

Authenticated public submission is `lx_sendActivity` on gateway `POST /rpc`
([Public JSON-RPC](../platform/gateway-rpc.md)). Commitment names
`executed`, `batched`, and `finalised` are evidence requirements, not progress
labels ([Commitment levels](../protocol/commitment-levels.md)).

The agent layer submits only exact signed canonical bytes over the
[LayerX Node Interface](../agents/lni.md). An admission acknowledgement is not
execution.

## Status

The envelope, sequence, idempotency, and fee-limit rules are specified and
implemented in the C17 kernel. Public `lx_sendActivity` admits the Asset
ordinals listed in [Assets](assets.md). Ordinal 9 (withdraw) is reserved on
that public path.
