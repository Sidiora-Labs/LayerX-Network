import { readFile } from "node:fs/promises";
import { SingleProcessWebhookDeliveryStore, mountLayerX } from "@sidiora/layerx-next";
import { FileFulfillmentRepository } from "./fulfillment.mjs";

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
