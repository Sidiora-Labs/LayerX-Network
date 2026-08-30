import { mkdir, open, readFile } from "node:fs/promises";
import { join } from "node:path";
import { MiddlewareError, SingleProcessWebhookDeliveryStore, mountLayerX } from "@sidiora/layerx-next";

class FileFulfillmentRepository {
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

const resourceBody = await readFile(new URL("../resource.json", import.meta.url), "utf8");

export const settlements = [];

export const layerx = mountLayerX({
  environment: process.env,
  resources: {
    async release() {
      return { contentType: "application/json", body: resourceBody };
    },
  },
  fulfillments: new FileFulfillmentRepository(process.env.LAYERX_FULFILLMENT_DIR ?? "./fulfillments"),
  deliveries: new SingleProcessWebhookDeliveryStore(),
  events: {
    async handle(event, deliveryId) {
      settlements.push({ deliveryId, event });
    },
  },
});
