# FastAPI quickstart

Charge for a FastAPI route. Seven lines mount a payment gate and a verified webhook endpoint on an app you already have.

## Before you start

```text
python3 -m pip install layerx-fastapi uvicorn
```

The integration reads the declared keys from the environment: `LAYERX_PRINCIPAL`, `LAYERX_PROTECTED_PATH`, the `LAYERX_RESOURCE_*` description fields, `LAYERX_X402_SCHEME`, `LAYERX_X402_NETWORK`, `LAYERX_PRICE`, `LAYERX_ASSET`, `LAYERX_PAY_TO`, `LAYERX_PAYER`, optional `LAYERX_COMMITMENT` (`executed` when unset), `LAYERX_PURPOSE_HASH` for metered and subscription schemes, `LAYERX_WINDOW_SECONDS` for subscriptions, `LAYERX_PAYMENT_TIMEOUT_SECONDS`, `LAYERX_AUTHORIZED_BATCH_JSON`, the four `LAYERX_WEBHOOK_*` values, `LAYERX_HUMAN_URL`, `LAYERX_SOURCE` and `LAYERX_TOKEN`. `LAYERX_TOKEN` is the only secret among them. Every 402 challenge includes `extra.layerx.payer`. Grant schemes also include `purposeHash` and an explicit commitment.

Before it configures anything, `read_declared_config` refuses to start if a declared secret has been copied into a variable with a published prefix - `NEXT_PUBLIC_`, `PUBLIC_`, `VITE_`, `REACT_APP_` or `EXPO_PUBLIC_`. A misconfiguration that would leak a token to a browser is a startup failure, not a runtime surprise.

## The integration

```python sample=paid-endpoint-fastapi
from layerx_fastapi import LayerXMountOptions, SingleProcessWebhookDeliveryStore, mount_layerx

mount = mount_layerx(app, LayerXMountOptions(
    resources=ReleaseReport(report_body),
    fulfillments=FileFulfillmentRepository(fulfillment_directory),
    deliveries=SingleProcessWebhookDeliveryStore(),
    events=settlements,
))
```

`mount_layerx` registers the payment gate at `LAYERX_PROTECTED_PATH` for `GET` and `POST`, and the webhook endpoint at `LAYERX_WEBHOOK_PATH` for `POST`. Both are registered with `include_in_schema=False`, so they never appear in your OpenAPI document.

| Option | What it is |
|---|---|
| `resources` | An object with `async release(request) -> LayerXResource` |
| `fulfillments` | An object with `async fulfill(proposed, release) -> StoredFulfillment` |
| `deliveries` | A webhook delivery store |
| `events` | An object with `async handle(event, delivery_id)` |

## Fulfilment storage is yours

```python sample=paid-endpoint-fastapi file=app.py region=storage
class FileFulfillmentRepository:
    def __init__(self, directory: Path) -> None:
        self.directory = directory

    async def fulfill(
        self,
        proposed: ProposedFulfillment,
        release: Callable[[], Awaitable[LayerXResource]],
    ) -> StoredFulfillment[LayerXResource]:
        self.directory.mkdir(parents=True, exist_ok=True, mode=0o700)
        path = self.directory / f"{proposed.idempotency_key}.json"
        if path.exists():
            return self.stored(path, proposed)
        resource = await release()
        record = json.dumps(
            {
                "requestDigest": proposed.request_digest,
                "receipt": b64encode(proposed.canonical_receipt).decode("ascii"),
                "contentType": resource.content_type,
                "body": b64encode(resource.body).decode("ascii"),
            }
        ).encode("utf-8")
        try:
            descriptor = path.open("xb")
        except FileExistsError:
            return self.stored(path, proposed)
        with descriptor as handle:
            handle.write(record)
            handle.flush()
            fsync(handle.fileno())
        return StoredFulfillment(
            idempotency_key=proposed.idempotency_key,
            request_digest=proposed.request_digest,
            canonical_receipt=proposed.canonical_receipt,
            authorized_batch=proposed.authorized_batch,
            resource=resource,
        )

    def stored(self, path: Path, proposed: ProposedFulfillment) -> StoredFulfillment[LayerXResource]:
        record = json.loads(path.read_text(encoding="utf-8"))
        if record["requestDigest"] != proposed.request_digest:
            raise MiddlewareError(MiddlewareErrorCode.FULFILLMENT_CONFLICT)
        return StoredFulfillment(
            idempotency_key=proposed.idempotency_key,
            request_digest=proposed.request_digest,
            canonical_receipt=b64decode(record["receipt"], validate=True),
            authorized_batch=proposed.authorized_batch,
            resource=LayerXResource(
                content_type=record["contentType"],
                body=b64decode(record["body"], validate=True),
            ),
        )
```

It opens with `xb` so two concurrent settlements cannot both create the record, `fsync`s before returning, and raises `MiddlewareError(FULFILLMENT_CONFLICT)` when the same idempotency key arrives with a different request digest. Replace the directory with shared durable storage before you run more than one worker.

## Run it

```text
cd platform/docs/samples/paid-endpoint-fastapi
python3 -m pip install -r requirements.txt
uvicorn app:app --host 127.0.0.1 --port 8080
```

`/mount` reports the paid path, the webhook path and the price actually in force; `/settlements` shows the verified events that have arrived.

## Enforced by

| Capability | Layer | What that means here |
|---|---|---|
| Offline receipt verification | `protocol` | The gate verifies the receipt from its own bytes against an authorised batch. |
| Atomic settlement | `protocol` | The payment behind a released resource happened whole or not at all. |
| Receipt-gated resource release | `service` | The mount releases only against a verified receipt. |
| Exactly-once fulfilment | `service` | Only as durable as the repository you supply. |
| Verified, replay-protected webhooks | `service` | Signature-checked, age-checked and lease-claimed in your process. |
| Refusal to publish a secret | `service` | Startup fails when a declared secret appears under a published prefix. |
