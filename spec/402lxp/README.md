
RPC finalised verification accepts `checkpoint_evidence.checkpoint_id`, `checkpoint`,
`context`, and `canonical_header` as lowercase hexadecimal. `rpcCheckpointEvidence`
and Python `rpc_checkpoint_evidence` decode the version-1 binary certificate and
canonical batch header into the SDK checkpoint verifier input. The embedded header
must equal both the published header and the receipt inclusion header. Certificate
threshold, order, bounds, trailing bytes, and context mismatches are refused.

Operators supply `RpcCheckpointAuthority`: the authenticated canonical context,
registered checkpoint and settlement reference, settlement domain, bonded guarantor
keys, availability confirmation, and required guarantor count. The context is pinned
byte-for-byte; its contents never supply authority or a default quorum. Obtain this
configuration through the operator's authenticated authority channel, independently
of the payment RPC response. The adapter constructs evidence; payment verification
must still call the SDK verifier before releasing a resource. No RPC evidence field
is missing from this mapping. A deployment without the operator inputs remains
unqualified for finalised payments.

402LXP payment payloads always use the literal `sequencer-signed` for
`verificationLevel`. The selected offer separately carries the RPC commitment:
`executed`, `batched`, or `finalised`. Implementations must not copy a commitment into
`verificationLevel`, infer a commitment from that field, or accept the spelling
`finalized`.
