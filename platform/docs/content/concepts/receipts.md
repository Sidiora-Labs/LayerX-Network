# Receipts and verification

A receipt is the thing that makes a LayerX Network payment different from a database row that says a payment happened. It is a canonical byte string produced by the protocol, and it verifies against an authorised batch header using only those two inputs. No LayerX node, gateway or hosted service is in the path.

That is the whole point: someone who trusts none of the operators can still check the claim.

## What verification actually checks

| Check | What a failure means |
|---|---|
| Canonical encoding | The bytes are not a receipt this protocol version could have produced |
| Protocol invariants | The receipt describes a state change the transition function would not have made |
| Root chain | The receipt does not belong under the batch header you supplied |
| Signature | The sequencer did not authorise this batch |

The verifier returns the exact check that failed. It never returns a generic "invalid".

## Verification levels

A verified receipt carries a level, and the levels are ordered by declaration: a later one implies every earlier one.

| Level | What backs it |
|---|---|
| `Unverified` | Nothing yet |
| `SequencerSigned` | The sequencer signed the batch containing this activity |
| `BatchIncluded` | Merkle inclusion in that batch is proven |
| `StateProven` | The state transition is proven |
| `CheckpointFinalised` | A finalised checkpoint covers it |
| `SettlementAnchored` | External settlement evidence anchors it |

No layer reports a level its evidence does not justify. When a requested level is not achieved, the response says so rather than quietly downgrading.

## Verifying one yourself

The Rust sample below is a complete program. Give it the receipt bytes and the batch facts, and it prints the verification level, the amount, the protocol result code and the receipt digest, or the exact check that refused.

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

The same thing from a shell, with the CLI:

```text
layerx receipt verify --receipt ./receipt.bin \
  --batch-id "$LAYERX_BATCH_ID" --asset "$LAYERX_BATCH_ASSET" \
  --previous-state-root "$LAYERX_PREVIOUS_STATE_ROOT" \
  --resulting-state-root "$LAYERX_RESULTING_STATE_ROOT" \
  --sequencer-public-key "$LAYERX_SEQUENCER_PUBLIC_KEY" --json
```

## Where the bytes come from

On the human plane, a journey carries evidence references. A reference whose class is `layerx-receipt` can be fetched as evidence material - the canonical bytes exactly as the protocol produced them, plus its content type and verification level. On the agent plane, `read.proof_bundle` and `export.offline` give you the same material for offline use.

You can also verify from an untrusted mirror archive with no LayerX dependency at all. That path returns freshness with the result instead of implying it.

## Enforced by

| Capability | Layer | What that means here |
|---|---|---|
| Offline receipt verification | `protocol` | Verification needs no LayerX component. A settlement claim can be checked by someone who trusts none of them. |
| Honest verification levels | `agent-layer` | Every successful response carries its verification status, and a shortfall is reported rather than silently downgraded. |
| Done means verified | `service` | The human service renders `done` only against verified evidence, never against its own optimism. |
| Receipt-gated resource release | `service` | Seller middleware releases your resource only after the receipt covering the request verifies. |
