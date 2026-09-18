# Next.js quickstart

Charge for a route handler in the App Router. Eight lines, plus a bundle scanner that fails your build if a secret would ship to the browser.

## Before you start

```text
npm install @sidiora/layerx-next next react react-dom
```

The declared configuration is the same as [Express](framework-express.html), read from the environment on the first request rather than at module scope. `next build` collects your route modules, so a key read at module scope becomes a build input; reading it on the first request keeps `next build` runnable with no key present.

## The integration

```js sample=paid-route-next
import { SingleProcessWebhookDeliveryStore, mountLayerXOnRequest } from "@sidiora/layerx-next";
export const layerx = mountLayerXOnRequest(() => ({
  environment: process.env,
  resources: { release: async () => ({ contentType: "application/json", body: reportBody }) },
  fulfillments: new FileFulfillmentRepository(fulfillmentDirectory()),
  deliveries: new SingleProcessWebhookDeliveryStore(),
  events: { handle: async (event, deliveryId) => { settlements.push({ deliveryId, event }); } },
}));
```

`mountLayerXOnRequest` takes no router here. It returns route objects you export directly and calls your factory once, on the first request. A request that finds a declared variable missing is refused with `503` and a body naming it - `{"error":"missing-declared-key","key":"LAYERX_PRICE"}` - never served with a default or placeholder key. `mountLayerX` is still exported for a host that wants the configuration read and refused at startup.

## Wire the routes

The paid route:

```js sample=paid-route-next file=app/paid/route.js region=route
import { layerx } from "../../lib/layerx.js";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

export const { GET, POST } = layerx.resource;
```

The webhook route:

```js sample=paid-route-next file=app/layerx/webhooks/route.js region=route
import { layerx } from "../../../lib/layerx.js";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

export const { POST } = layerx.webhook;
```

Both must be `runtime = "nodejs"` and `dynamic = "force-dynamic"`. The Edge runtime cannot do the verification the gate performs, and a cached route would serve a paid body to someone who did not pay for it.

## The bundle scanner

The package ships `layerx-scan-bundle`. Run it after `next build`:

```text
npx layerx-scan-bundle
```

It reads your client bundle and reports four kinds of finding, each of which fails the build:

| Finding | What it caught |
|---|---|
| `declared-secret-value` | The literal value of `LAYERX_TOKEN` appears in a shipped artifact |
| `declared-secret-name` | The name of a declared secret appears in a shipped artifact |
| `private-key-block` | A PEM private key block was bundled |
| `published-key-material` | A `NEXT_PUBLIC_`, `PUBLIC_`, `VITE_`, `REACT_APP_` or `EXPO_PUBLIC_` variable is named like a token, secret, credential or signing key |

Findings are reported with a redacted locator and an offset, never with the secret itself. Wire the scanner into your build script so it cannot be forgotten - it is a build-time control, and a control you skipped enforces nothing.

## Fulfilment storage

Serverless makes this sharper than it is on Express: a fulfilment repository backed by process memory is empty on the next cold start, and the exactly-once guarantee goes with it. The sample uses a durable file-backed repository:

```js sample=paid-route-next file=lib/fulfillments.js region=storage
import { mkdir, open, readFile } from "node:fs/promises";
import { join } from "node:path";
import { MiddlewareError } from "@sidiora/layerx-next";

export class FileFulfillmentRepository {
  constructor(directory) {
    this.directory = directory;
  }

  async fulfill(proposed, release) {
    await mkdir(this.directory, { recursive: true, mode: 0o700 });
    const path = join(this.directory, `${proposed.idempotencyKey}.json`);
    try {
      return await this.read(path, proposed);
    } catch (error) {
      if (error.code !== "ENOENT") throw error;
    }
    const resource = await release();
    const record = JSON.stringify({
      requestDigest: proposed.requestDigest,
      receipt: Buffer.from(proposed.canonicalReceipt).toString("base64"),
      resource,
    });
    let file;
    try {
      file = await open(path, "wx", 0o600);
      await file.writeFile(record, "utf8");
      await file.sync();
      return { ...proposed, resource };
    } catch (error) {
      if (error.code !== "EEXIST") throw error;
      return await this.read(path, proposed);
    } finally {
      await file?.close();
    }
  }

  async read(path, proposed) {
    const stored = JSON.parse(await readFile(path, "utf8"));
    if (stored.requestDigest !== proposed.requestDigest) {
      throw new MiddlewareError("fulfillment-conflict");
    }
    return {
      ...proposed,
      canonicalReceipt: Uint8Array.from(Buffer.from(stored.receipt, "base64")),
      resource: stored.resource,
    };
  }
}
```

In production put this on shared durable storage, not on a function's local disk.

## Run it

```text
cd platform/docs/samples/paid-route-next
npm install
npm run dev
```

## Enforced by

| Capability | Layer | What that means here |
|---|---|---|
| Offline receipt verification | `protocol` | The route verifies the receipt from its own bytes against an authorised batch. |
| Atomic settlement | `protocol` | The payment behind a released body happened whole or not at all. |
| Receipt-gated resource release | `service` | The route handler releases only against a verified receipt. |
| Refusal to publish a secret | `service` | The scanner fails a build whose client bundle carries a declared secret. It is a build-time control, so run it. |
| Exactly-once fulfilment | `service` | Only as durable as your repository. On serverless, that means shared storage. |
| Verified, replay-protected webhooks | `service` | Signature-checked, age-checked and lease-claimed in your process. |
