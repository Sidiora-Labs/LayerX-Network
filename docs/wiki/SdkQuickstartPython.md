# Python SDK quickstart

Package: `layerx-sdk` at `agent/sdk/python`. There is no `rpc`
module of typed `lx_*` wrappers. `PaymentRpc` in
`layerx_sdk.x402_rpc` is a generic JSON-RPC client whose public
`call` can name any method. Dedicated helpers exist only for
`lx_sendActivity`, `lx_getReceipt`, and `lx_getActivityStatus`.

---

## Register

No `register()` helper. `PaymentRpc.call` can send `lx_register`:

```python
from layerx_sdk import PaymentRpc

def register(rpc: PaymentRpc, signer_public_key: str, registration_signature: str) -> dict:
    return rpc.call("lx_register", [signer_public_key, registration_signature])
```

Both arguments are lowercase hex as required by
[Public JSON-RPC](PublicRpc.md). Binding-digest construction is not
in this package; the Rust crate's `register::binding` is the typed
helper for that.

---

## Fund from faucet

No `request_funds()` helper. `PaymentRpc.call` can send
`lx_requestFunds`. The gateway requires a Bearer identity session.
Pass it in `PaymentRpc`'s `headers`:

```python
from layerx_sdk import PaymentRpc

def request_funds(endpoint: str, session_token: str, did: str, signer_public_key: str) -> dict:
    rpc = PaymentRpc(endpoint, {"Authorization": f"Bearer {session_token}"})
    return rpc.call("lx_requestFunds", [did, signer_public_key])
```

`PaymentRpc` refuses non-HTTPS endpoints except loopback
`http://localhost`, `127.0.0.1`, or `::1`, and requires path
`/rpc`. The generated agent operation `"faucet.claim"` exists on
`Client.call` / `ProductionClient.agent` but
`AgentHttpTransport` is programs-only.

---

## Send

```python
from layerx_sdk import PaymentRpc

def send(rpc: PaymentRpc, canonical_hex: str, commitment: str = "executed") -> dict:
    return rpc.send(canonical_hex, commitment)
```

`commitment` must be `executed`, `batched`, or `finalised`. For
`lx_sendActivity` attach `LayerX-Key` via `headers`, not Bearer.

Hosted human-plane send, as in
`platform/docs/samples/first-payment-python`:

```python
from layerx_sdk import IdempotencyKey, ProductionClient

def pay(layerx: ProductionClient, source, destination, money, payment_key):
    quote = layerx.human("move.quote", {"source": source, "destination": destination, "money": money})
    return layerx.human("move.commit", {"quote_id": quote["quote_id"]}, idempotency_key=IdempotencyKey(payment_key))
```

`ProductionClient` needs a `ProductionTransport`. The in-repo sample
uses `layerx_transport.HumanApiTransport`, which is **not** shipped
inside the `layerx-sdk` package.

---

## Verify a receipt

```python
from layerx_sdk import (
    AuthorizedReceiptBatch,
    LocalSignatureVerifier,
    ReceiptVerification,
    verify_receipt,
)

def verify(
    canonical_receipt: bytes,
    authorized_batch: AuthorizedReceiptBatch,
    signatures: LocalSignatureVerifier,
) -> ReceiptVerification:
    return verify_receipt(canonical_receipt, authorized_batch, signatures)
```

`AuthorizedReceiptBatch` fields: `batch_id`, `asset`,
`previous_state_root`, `resulting_state_root`,
`sequencer_public_key`. `LocalSignatureVerifier` is a `Protocol`
with `verify_ed25519` and `verify_recoverable_secp256k1`. This
package does not ship a default implementation of that protocol;
the caller supplies one. Those batch facts must come from a source
you already trust.

[SDK quickstarts](SdkQuickstarts.md) · [Home](Home.md)
