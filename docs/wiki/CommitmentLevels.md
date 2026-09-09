# Commitment levels

A payment or activity submit names one commitment: `executed`, `batched`,
or `finalised`. The word is evidence, not a status the caller may
downgrade. An admission acknowledgement, queue position, HTTP 202, or
`pending` body is never success.

These three names are the public submit/verify vocabulary. They sit
beside the L0–L4 settlement ladder on [Finality](Finality.md); they do
not replace it.

Related pages: [Public JSON-RPC](PublicRpc.md),
[Payments developer path](PaymentsQuickstart.md),
[x402 transport](X402Transport.md), [Finality](Finality.md).

---

## The three levels

| Level | Evidence | Who signed it |
| --- | --- | --- |
| `executed` | Canonical successful receipt | Authorized sequencer |
| `batched` | Executed evidence plus receipt inclusion in a signed batch header | Sequencer over the authorized batch |
| `finalised` | Batched evidence plus the guarantor checkpoint certificate covering that batch | Bonded guarantor quorum |

Definitions match `lane/pay-public-rpc`
(`platform/hosted/gateway/openrpc.json`, `lx_sendActivity`) and
`lane/pay-402lxp` (`spec/402lxp/protocol.md`). Both branches are
**on the testnet branch**; `main` has no `commitment` parameter on a
public RPC method.

Trust inputs come from the verifier's configured network authority
(pinned sequencer key, network id, protocol version, guarantor set),
not from the payment header or RPC result alone.

---

## `executed`

A sequencer-signed receipt for the submitted activity. The receipt must
verify, name a successful `result_code`, and bind the payment facts
(asset, amount, payer, recipient, activity id).

On `main`, hosted `POST /v1/activities` already refuses to treat
component HTTP 202 as verified success
(`platform/hosted/gateway/src/main.rs`). The public RPC name `executed`
is the same bar: a verified receipt, not an ack.

On [Finality](Finality.md) this is in-channel accept (L0): the sequencer
is on the hook for inclusion and ordering in the current batch.

---

## `batched`

`executed` plus Merkle inclusion of that receipt in a signed, authorized
batch header. The header's network, protocol, sequence coverage, and
sequencer authorization must verify.

On [Finality](Finality.md) a sealed batch is L1: ordering is fixed.
`batched` is the public name for verified inclusion in that header.

---

## `finalised`

`batched` plus the guarantor checkpoint certificate for that batch.
The certificate must cover the same canonical header as the batch
evidence. A certificate for a different batch does not promote the
receipt.

On [Finality](Finality.md) bonded re-execution and checkpoint
registration are L3–L4. `finalised` is the public name for that
certificate. Custody still moves only on Paxeer.

---

## What is refused

- `"ack"` as a commitment value (JSON-RPC `-32602` on
  `lane/pay-public-rpc`).
- Treating missing or invalid evidence as `executed`.
- Substituting a weaker commitment than the offer or submit requested.
- Releasing a 402 resource on HTTP 202 or `settlement_pending`
  (`interop/crates/layerx-x402`; `lane/pay-402lxp` seller commitment).

On `lane/pay-public-rpc`, `lx_sendActivity` waits a bounded interval
(5 seconds, 50 ms poll) for the requested evidence. If the evidence is
absent it returns `state: "pending"`. That result is not execution.
OpenRPC: *"An admission acknowledgement never establishes execution."*

On `lane/pay-402lxp`, `extra.layerx.commitment` on a 402 offer is
`executed`, `batched`, or `finalised`. An exact offer without
`extra.layerx` defaults to `executed`. Grant schemes require an explicit
commitment.

---

## Where the names appear

| Surface | On `main` | On the testnet branch |
| --- | --- | --- |
| Hosted activity POST | Verified receipt or 202 `unknown`; no `commitment` field | same hosted path |
| `lx_sendActivity` | not present | required param on `lane/pay-public-rpc` |
| x402 `PAYMENT-REQUIRED` | no `extra.layerx.commitment` | `lane/pay-402lxp` |
| x402 settlement `verificationLevel` | literal `sequencer-signed` | plus batch / checkpoint checks when requested |
| Portable receipt verify | local `layerx receipt verify` against caller-supplied batch facts ([CLI](Cli.md)) | same |

[Home](Home.md)
