# Commitment levels

`lx_sendActivity` and the payment middleware use three exact commitment names:
`executed`, `batched`, and `finalised`. These surfaces are on the testnet
branch. A commitment is an evidence requirement, not a progress label. The
gateway never converts a stronger request into a weaker success.

See [Public JSON-RPC](PublicRpc.md),
[Payments developer path](PaymentsQuickstart.md), and
[x402 transport](X402Transport.md).

## Evidence at each level

| Level | Required result |
| --- | --- |
| `executed` | A verified canonical receipt for the submitted activity |
| `batched` | `executed`, plus the authenticated receipt proof whose `canonical_value` exactly equals the returned receipt |
| `finalised` | `batched`, plus the latest finalised checkpoint evidence whose `canonical_header` exactly equals the proof's signed batch header |

For `executed`, the receipt must already be present and verified by the
ordinary gateway activity path. An admission acknowledgement, queue state,
HTTP 202, or activity id without a receipt is not execution evidence.

For `batched`, the gateway reads the receipt proof by activity id. It requires
the proof to name the same activity and its canonical value to equal the
receipt byte-for-byte. The proof is returned as `batch_evidence`.

For `finalised`, the gateway reads node info, selects its nonzero
`latest_finalised_checkpoint`, reads that checkpoint, and requires its exact
canonical header to match the signed header in `batch_evidence`. The checkpoint
is returned as `checkpoint_evidence`. A different checkpoint or merely newer
head is not substituted.

These names are public submission and verification vocabulary. They do not
replace the broader L0–L4 lifecycle described in [Finality](Finality.md).

## Pending and refusal behavior

When the bounded wait cannot establish the requested level, JSON-RPC returns
an error rather than a result:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "error": {
    "code": -32001,
    "message": "Requested commitment unavailable",
    "data": {
      "state": "pending",
      "requested_commitment": "finalised",
      "evidence": {}
    }
  }
}
```

An upstream HTTP 202 is also translated to `-32001` with `state: "pending"`
and the requested commitment. Preserve the original signed activity and
activity id, then recover with `lx_getActivityStatus`, `lx_getReceipt`, and the
required proof reads. Do not create a replacement activity to escape an
unknown result.

Values such as `ack`, `accepted`, `finalized`, and uppercase variants are
invalid parameters (`-32602`). The wire spelling is the British
`finalised`.

## x402 binding

An x402 offer may carry the commitment in `extra.layerx.commitment`.
`executed` is the default only for an exact offer that omits the LayerX extra;
metered and subscription offers require their LayerX payer and purpose terms.
The seller verifies the exact requested level before releasing the resource.

Successful settlement identifies the verified receipt as
`lxp:<receipt_digest>`. That reference, an HTTP success, or a
`verificationLevel` string does not stand alone: the buyer and seller validate
the receipt, payment facts, configured authority, and commitment evidence.

[Home](Home.md)
