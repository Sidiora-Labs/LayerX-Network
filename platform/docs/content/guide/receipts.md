# Receipts

A receipt is the only artefact in LayerX Network that constitutes proof. Everything else - a `200`, a webhook, a journey in state `done`, a dashboard row - is a report about a payment. The receipt is the payment.

## What verification needs

Verifying a receipt takes exactly two things: the canonical receipt bytes, and the batch facts they should be checked against.

| Batch fact | What it is |
|---|---|
| `batch_id` | Which batch this receipt claims membership in |
| `asset` | The asset the batch settles |
| `previous_state_root` | The state the batch started from |
| `resulting_state_root` | The state it produced |
| `sequencer_public_key` | The key that authorised it |

Verification is a pure function of those inputs. No node, no daemon, no database, no clock and no network connection is involved. This is what makes a receipt portable: you can hand it, and the batch facts, to a counterparty who has never spoken to us, and they can check it for themselves.

## Where the batch facts must come from

Not from the same place as the receipt. A receipt verified against batch facts supplied by the party who gave you the receipt proves only that they are internally consistent. Take the batch header from a source you trust independently - a checkpoint you follow, a mirror publication on another chain, a counterparty's own view.

## Verifying in code

```rust sample=verify-receipt-rust
use layerx_proof::receipt::AuthorizedBatch;
use layerx_sdk::production::verify_receipt;

fn settlement(receipt: &[u8], batch: &BatchFacts) -> Result<Settlement, String> {
    let authorised = AuthorizedBatch::new(batch.batch_id, batch.asset, batch.previous_state_root, batch.resulting_state_root, batch.sequencer_public_key);
    let verified = verify_receipt(receipt, &authorised).map_err(|failure| format!("receipt refused at {:?}", failure.check))?;
    let facts = verified.receipt().protocol().ok_or("receipt carries no protocol facts")?;
    Ok(Settlement { level: verified.level().wire_rank(), amount: facts.amount(), result_code: facts.result_code(), digest: verified.evidence().receipt_digest().ok_or("verifier produced no digest")? })
}
```

## Verifying from the command line

```
layerx receipt get <receipt-id> --json
layerx receipt verify \
  --receipt ./receipt.hex \
  --batch-id <hex> \
  --asset <hex> \
  --previous-state-root <hex> \
  --resulting-state-root <hex> \
  --sequencer-public-key <hex>
```

`receipt get` reads exact receipt material from the active environment. `receipt verify` never touches the network: every fact it checks against is an argument you supplied. The `--receipt` file may hold the canonical bytes raw, or the same bytes as ASCII hex - the CLI accepts either, so piping a hex field out of a JSON response into a file works without a conversion step.

On success it prints the verification level as its wire rank, the receipt digest, the activity id, the batch id, the protocol result code and the canonical byte length. On failure it names the exact check that failed - not a generic error.

## Verification levels

A verified receipt carries a level, and the levels are ordered:

| Level | Means |
|---|---|
| `unverified` | Nothing has been established |
| `sequencer-signed` | The sequencer signed it |
| `batch-included` | It is in a batch |
| `state-proven` | The batch's state transition is proven |
| `checkpoint-finalised` | The batch is under a finalised checkpoint |
| `settlement-anchored` | The checkpoint is anchored externally |

A lower level is never reported as a higher one, and no code path returns a level it did not establish. Decide which level your business needs and check for it explicitly - shipping physical goods and unlocking an API response are not the same risk.

## `still-checking` is an answer

If a receipt cannot presently be verified, that is reported as itself. It is not an error and it is not a failure, and treating it as either is how double-payments happen. The rule across every LayerX surface is the same: an unknown outcome stays unknown until something resolves it.

## Enforced by

| Capability | Layer | What that means here |
|---|---|---|
| Offline receipt verification | `protocol` | Verification is a pure function of receipt bytes and batch facts. |
| Atomic settlement | `protocol` | A verified receipt describes a payment that applied whole. |
| Conserved supply | `protocol` | The amounts in a verified receipt are the amounts that moved. |
| Replay refusal | `protocol` | A receipt cannot be presented twice to settle twice. |
| Honest verification levels | `agent-layer` | Levels are reported exactly; `still-checking` is first class. |
