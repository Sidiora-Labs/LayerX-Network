# Retries and unknown outcomes

Networks time out in the middle of payments. LayerX Network is built so that the obvious reaction - retry - is safe, and so that the case where nobody can yet say what happened is a first-class answer rather than a silence.

## Idempotency keys

Every mutation on the human plane that could move money requires an `Idempotency-Key` header. You choose the key. Repeating the request with the same key returns the original journey rather than producing a second effect.

The SDKs enforce this before the request leaves your process. Calling `move.commit` without a key fails locally with `idempotency-required`; you do not discover the problem from a duplicate payment. Sending the same key with a different body is a conflict, not a silent overwrite.

Choose a key that is stable for the payment, not for the attempt. A good key is derived from the thing being paid for - an order identifier, a cart digest - so that a retry after a crash reuses it naturally. A UUID generated at the call site is the classic mistake: the retry generates a new one and pays twice.

## What a retry is allowed to do

Every refusal carries a retriability class, and the SDKs surface it on the error rather than making you parse a message.

| Class | What it means |
|---|---|
| `retriable` | Retry the same request as-is |
| `retriable-after` | Retry once the carried `retry_after_ms` has elapsed. Rate-limit refusals always classify here and always carry the timing |
| `structural` | Retrying cannot help until something around it changes - re-authenticate, reload |
| `final` | The outcome is settled. Retrying will not produce a different one |

## Unknown

Sometimes a submission genuinely cannot be classified: the request left, the response did not come back, and nothing yet proves which side of the transition it landed on. LayerX answers `Unknown` on the agent plane and `still-checking` on the human plane.

`Unknown` is terminal-pending, not an error. The correct handling is:

1. Do not retry the payment. The idempotency key already protects you, but a retry does not answer the question either.
2. Resolve it by looking up the receipt under your idempotency key. That is what `track` and `wait` do on the agent plane.
3. Keep every duplicate-capable control locked while it lasts. The human plane does this for you; do it in your own UI too.

What you must not do is render it as either a success or a failure. A payment that might have happened is a different fact from one that did not, and the whole system is built to preserve that distinction rather than round it away.

## Protocol-level replay

Underneath all of this, each principal's activity sequence is consumed exactly once. Even if every layer above the protocol were bypassed, an already-applied activity cannot apply again. Idempotency keys are how you get a useful answer; sequence consumption is why a wrong answer cannot cost you money twice.

## Enforced by

| Capability | Layer | What that means here |
|---|---|---|
| Replay refusal | `protocol` | An already-applied activity is refused by the transition function. |
| Atomic settlement | `protocol` | A timed-out call left either a whole payment or none of one behind. |
| Unknown is a real outcome | `agent-layer` | An unclassifiable submission is reported as `Unknown` and resolved by tracking. |
| Idempotent money moves | `service` | The human service requires the key and returns the original journey on repeat. |
| Exactly-once fulfilment | `service` | A repeated payment returns the stored fulfilment; a different request digest under the same key is a conflict. |
| Hosted rate limits | `hosted-surface` | The gateway refuses excess traffic with honest retry timing. |
