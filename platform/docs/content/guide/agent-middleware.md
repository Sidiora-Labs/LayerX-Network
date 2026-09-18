# Agent middleware

`@sidiora/layerx-agent-middleware` is what an autonomous agent spends through. It wraps the agent plane's prepare/sign/submit/track loop in a budget ledger and refuses to call a payment successful without a verified receipt.

## One call, six outcomes

`spend(request)` returns exactly one of these. There is no exception path that means "it probably worked".

| Result | Meaning | Budget state |
|---|---|---|
| `verified` | Receipt verified and matched against amount, asset and recipient | `committed` |
| `approval-hold` | The daemon refused on policy and a matching held approval exists | `held` |
| `pending` | Still settling after the poll budget | `reserved` |
| `unknown` | The outcome is genuinely not known | `reserved` - deliberately not released |
| `refused` | Failed, expired, or refused by the daemon | `released` |
| `budget-refused` | The reservation was refused before anything was prepared | none taken |

## The order of operations

Reserve first. The budget is taken before `prepare` is called, so a spend that never reaches the network cannot exceed the ceiling. `reserve` is keyed on tenant, idempotency key and request digest, so a retry of the same spend re-reserves the same reservation rather than a second one; a same-key different-digest reservation is a `conflict`, which raises `idempotency-conflict`.

Then prepare, sign, submit. The signer only ever sees a `PreparedActivity` - the unsigned canonical bytes, the signing preimage and the disclosure. The middleware never constructs canonical bytes itself and never holds a key.

Then track, with a bounded backoff of up to `maximumTrackPolls` polls (default 20, capped at 1000 at construction). The wait function is injectable so the loop is testable without real time.

## `unknown` is a real answer

If the SDK raises with `retry === "unknown-outcome"`, the middleware returns `unknown` and leaves the reservation standing. It does not release the budget, because releasing it would let a second spend go out while the first may yet settle. It does not commit it either, because nothing has been verified. Resolution belongs to whoever can look the submission up later.

`retry === "safe"` also yields `unknown` rather than an automatic retry: the middleware will not decide on your behalf that a payment is safe to reissue.

## Verification is a match, not a lookup

A resolved receipt is verified against its batch authorisation, and then three fields are compared to the request:

- `verification.receipt.amount` must equal the protocol amount that was reserved.
- The receipt must carry the `protocolVersion` the middleware was configured with. That version has no default; a configuration that omits it is refused at construction with `missing-protocol-version`.
- `verification.receipt.asset` must equal the requested asset, compared in constant time.
- `verification.receipt.to` must equal the requested recipient, compared in constant time.

A receipt that verifies cryptographically but pays a different recipient is `verification-failure`, not success. Only after all three match is the budget committed, and the commit result is itself re-checked for state and digest.

## Approval holds

A `policy-refusal` or `budget-refusal` on submit triggers a lookup of the tenant's held approvals, matching on the disclosure digest of the prepared activity. A match returns an `ApprovalHold` with `enforcement: "daemon_enforced"` - the hold is real and enforced outside your process; the middleware only surfaces it.

## Enforced by

| Capability | Layer | What that means here |
|---|---|---|
| Protocol budget ceilings | `protocol` | The budget module refuses over-ceiling spends at the protocol boundary. |
| Offline receipt verification | `protocol` | Receipts are verified against a batch authorisation, not asked about. |
| Replay refusal | `protocol` | The same canonical activity cannot settle twice. |
| Capability attenuation | `agent-layer` | The agent signs only what `prepare` produced. |
| Approval holds | `agent-layer` | A held approval is enforced by the daemon; the middleware reports it. |
| Honest verification levels | `agent-layer` | `pending` and `unknown` are returned as themselves, never as success or failure. |
| Unknown is a real outcome | `agent-layer` | An unresolved spend keeps its reservation instead of freeing budget. |
| Agent tenancy isolation | `agent-layer` | Reservations and approvals are scoped to the tenant on the request. |
