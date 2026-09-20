# Receipts

A receipt is evidence the protocol computed. No receipt field is supplied by a
client (`spec/layerx-protocol/spec.kvx`, requirement 2 `ac_9` and
requirement 12).

The activity receipt carries at least `activity_id`, `global_sequence`,
`previous_state_root`, `resulting_state_root`, `activity_root`, `result_code`,
`effects`, `fee_charged`, `batch_id`, and `sequencer_signature`. Every value
is computed from the resulting state.

## 402LXP receipts

Requirement 12 of `spec/layerx-protocol/spec.kvx` requires a successful 402LXP
operation to return a `402LXPReceipt` with protocol-computed before and after
balances, transfer-set root, authorization and context hashes, chained state
roots, batch id, timestamp, and sequencer signature. Client-supplied balances
are ignored or rejected.

A failed operation emits a failure result with the result code and unchanged
balances. It does not emit a signed `402LXPReceipt`.

After the covering checkpoint is accepted on Paxeer, the retrievable receipt
may be augmented with inclusion proofs, `checkpoint_id`, a guarantor
certificate, and a Paxeer settlement reference, without altering
pre-checkpoint fields.

## Public evidence names

Gateway submit and 402 offers name three evidence levels:
`executed`, `batched`, and `finalised`
([Commitment levels](../protocol/commitment-levels.md)).
Those names do not replace the L0–L4 ladder in [Finality](../protocol/finality.md).

An admission acknowledgement, queue state, HTTP 202, or activity id without a
receipt is not execution evidence.

## Independent verification

`layerx-proof` verifies a signed receipt from its bytes alone
(`spec/.beta/layerx-agent-interface/spec.kvx`, requirement 6). The CLI command
`layerx receipt verify` is a local check against caller-supplied batch facts
([CLI](../platform/cli.md), [Quickstart](../overview/quickstart.md)).

`layerx-portable` exports and verifies a `layerx-receipt-proof-v1` JSON object
against an independently trusted `AuthorizedBatch` with no node, gateway,
daemon, database, clock, or network
([Portable receipt verifier](../interop/portable-receipts.md)).

The human plane renders Done only against a verified LayerX receipt or a
verified Paxeer finality proof (`spec/layerx-platform/spec.kvx`, decision
`done_is_receipt`). Unknown outcomes render as still-checking
([Human journeys](../human/journeys.md)).

## Agent-layer rule

The agent-interface specification requires every claimed result to be backed
by a core-produced receipt or proof that the layer verified for itself. A
submission whose fate cannot be determined is reported as unknown and is
resolved only by looking up the receipt for its idempotency key.
