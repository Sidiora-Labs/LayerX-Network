import { readFile } from "node:fs/promises";
import { FileFulfillmentRepository } from "./fulfillments.js";

const reportBody = await readFile(new URL("../resource.json", import.meta.url), "utf8");
const fulfillmentDirectory = () => process.env.LAYERX_FULFILLMENT_DIR ?? "./fulfillments";

export const settlements = [];

// layerx:begin integration
import { SingleProcessWebhookDeliveryStore, mountLayerXOnRequest } from "@sidiora/layerx-next";
export const layerx = mountLayerXOnRequest(() => ({
  environment: process.env,
  resources: { release: async () => ({ contentType: "application/json", body: reportBody }) },
  fulfillments: new FileFulfillmentRepository(fulfillmentDirectory()),
  deliveries: new SingleProcessWebhookDeliveryStore(),
  events: { handle: async (event, deliveryId) => { settlements.push({ deliveryId, event }); } },
}));
// layerx:end integration
