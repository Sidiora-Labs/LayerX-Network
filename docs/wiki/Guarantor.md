# Guarantor checkpoint production

The checkpoint producer is not implemented. The daemon now serves canonical
availability bundles, but only for batches at or below its finalized head.
This prevents a producer from retrieving a new checkpoint candidate before
it attests. The existing availability harness registers a fixture certificate
before successful retrieval; it does not demonstrate checkpoint production.

The required order is: retrieve the signed header through LNI tag 12, fetch
all five canonical availability classes for each candidate batch, verify the
served bytes against the header's availability root, verify authority and
replay through the existing deterministic engine, recompute every committed
root with `lxp_guarantor_recompute_roots`, durably retain the verified data,
then call `lxp_guarantor_attest`. A mismatch must refuse without a signature.
Each fetch covers one batch; multi-batch fetches are refused. The availability
root uses canonical 65536-byte chunks.

An authenticated candidate fetch interface is still required. In
`cmd/layerxd/lxp_daemon_lni.c`, `send_availability` returns -804 when the
selected batch exceeds `latest_finalized_batch`. With a finalized head of
zero, no positive candidate batch can be fetched. With a finalized head of
N, batch N+1 is also unavailable. Registering before fetching would bypass
the guarantor's replay and possession duties. The existing finalized-only
consumer interface and its tests must remain intact when candidate access is
added.

Beta checkpoint certificates use the signed batch header and an **empty
validity proof**. No validity proof is generated or claimed. The implemented
checkpoint and attestation domains are `LXP/v2/checkpoint-certificate\0` and
`LXP/v2/guarantor-attestation\0`. Section 5 of the guarantor specification
still describes `LXP1/guarantor-attest`; the implemented C, Rust and Solidity
verifiers and shared vectors determine the producer encoding.

The intended beta deployment has two bonded guarantor identities operated by
the same cluster. They provide **no operational independence**. Production
independence is an onboarding requirement and is not claimed for this beta.
Two keys and two processes do not establish independent operators.

After locally verified, bonded peer attestations meet the threshold, the
producer must assemble the certificate, register it through the relay, and
verify the registration event. Existing registration must be detected before
submitting another transaction. The settlement domain must use the deployed
GuarantorBond address and actual bonded set; chain 125 identifies the beta
deployment, and no contract address may be invented. The daemon must then
independently verify the finality bundle through tag 28 before consumers can
retrieve it through tag 14.

The two producer processes, mutual-TLS attestation exchange, dedicated funded
checkpoint submitter, candidate replay, registration and finality feedback
remain unimplemented. No runtime or deployment qualification is claimed.
