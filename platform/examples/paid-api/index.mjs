import { createServer } from "node:http";
import { mkdir, open, readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import {
  MiddlewareError,
  PAYMENT_SIGNATURE_HEADER,
  ReceiptPayloadAuthority,
  SellerMiddleware,
} from "@sidiora/layerx-seller-middleware";
import {
  LayerXApplicationStateError,
  ReceiptAuthorityClient,
  exactObject,
  hex32,
  loadApplicationConfig,
  requiredEnvironment,
} from "../support/runtime.mjs";

export function platform_ref_seller() {
  return "seller-middleware-live-receipt-authority-paid-api";
}

const safeName = (value) => {
  if (!/^[A-Za-z0-9._-]{1,255}$/u.test(value)) throw new Error("invalid_identifier");
  return value;
};

const serializeBatch = (batch) => ({
  batch_id: Buffer.from(batch.batchId).toString("hex"),
  asset: Buffer.from(batch.asset).toString("hex"),
  previous_state_root: Buffer.from(batch.previousStateRoot).toString("hex"),
  resulting_state_root: Buffer.from(batch.resultingStateRoot).toString("hex"),
  sequencer_public_key: Buffer.from(batch.sequencerPublicKey).toString("hex"),
});

const parseBatch = (value) => {
  const batch = exactObject(value);
  return {
    batchId: hex32(batch.batch_id),
    asset: hex32(batch.asset),
    previousStateRoot: hex32(batch.previous_state_root),
    resultingStateRoot: hex32(batch.resulting_state_root),
    sequencerPublicKey: hex32(batch.sequencer_public_key),
  };
};

const syncDirectory = async (path) => {
  const directory = await open(dirname(path), "r");
  try {
    await directory.sync();
  } finally {
    await directory.close();
  }
};

class FileFulfillmentRepository {
  constructor(directory) {
    this.directory = directory;
  }

  async fulfill(proposed, release) {
    await mkdir(this.directory, { recursive: true, mode: 0o700 });
    const path = join(this.directory, `${safeName(proposed.idempotencyKey)}.json`);
    try {
      return await this.read(path, proposed);
    } catch (error) {
      if (error.code !== "ENOENT") throw error;
    }
    const resource = await release();
    const record = {
      requestDigest: proposed.requestDigest,
      receipt: Buffer.from(proposed.canonicalReceipt).toString("base64"),
      authorizedBatch: serializeBatch(proposed.authorizedBatch),
      resource,
    };
    try {
      const file = await open(path, "wx", 0o600);
      try {
        await file.writeFile(JSON.stringify(record), "utf8");
        await file.sync();
      } finally {
        await file.close();
      }
      await syncDirectory(path);
      return { ...proposed, resource };
    } catch (error) {
      if (error.code !== "EEXIST") throw error;
      return this.read(path, proposed);
    }
  }

  async read(path, proposed) {
    const stored = exactObject(JSON.parse(await readFile(path, "utf8")));
    const canonicalReceipt = Uint8Array.from(Buffer.from(stored.receipt, "base64"));
    const authorizedBatch = parseBatch(stored.authorizedBatch);
    if (stored.requestDigest !== proposed.requestDigest) throw new MiddlewareError("fulfillment-conflict");
    return { ...proposed, canonicalReceipt, authorizedBatch, resource: stored.resource };
  }
}

class StatePreservingReceiptAuthority {
  constructor(resolver) {
    this.delegate = new ReceiptPayloadAuthority(resolver);
  }

  async settle(request) {
    try {
      return await this.delegate.settle(request);
    } catch (error) {
      if (!(error instanceof LayerXApplicationStateError)) throw error;
      if (error.state === "pending") return { kind: "pending" };
      if (error.state === "refused") return { kind: "refused", reason: error.message };
      throw error;
    }
  }
}

const config = await loadApplicationConfig(import.meta.url, "paid-api");
const resourceBody = exactObject(JSON.parse(await readFile(resolve(config.directory, config.resourceFile), "utf8")));
const resolver = new ReceiptAuthorityClient(
  config.receiptAuthorityUrl,
  requiredEnvironment(config.tokenEnvironment),
);
const middleware = new SellerMiddleware({
  paymentRequired: {
    x402Version: 2,
    resource: {
      url: config.resourceUrl,
      description: "Receipt-verified metered weather",
      mimeType: "application/json",
      serviceName: "LayerX paid API",
    },
    accepts: [{
      scheme: config.scheme,
      network: config.network,
      amount: requiredEnvironment(config.priceEnvironment),
      asset: requiredEnvironment(config.assetEnvironment),
      payTo: requiredEnvironment(config.payToEnvironment),
      maxTimeoutSeconds: 60,
    }],
  },
  authority: new StatePreservingReceiptAuthority(resolver),
  fulfillments: new FileFulfillmentRepository(resolve(config.directory, config.fulfillmentDirectory)),
});

const port = Number(config.port);
if (!Number.isSafeInteger(port) || port <= 0 || port > 65535) throw new Error("invalid_port");

createServer(async (request, response) => {
  try {
    if (request.method !== "GET" || request.url !== "/paid") {
      response.writeHead(404).end();
      return;
    }
    const header = request.headers[PAYMENT_SIGNATURE_HEADER.toLowerCase()];
    const paymentHeader = Array.isArray(header) ? undefined : header;
    const decision = await middleware.handle("public-paid-api", paymentHeader, async () => resourceBody);
    const headers = decision.kind === "payment-required" || decision.kind === "refused" || decision.kind === "released"
      ? decision.headers
      : { "retry-after": "1" };
    response.writeHead(decision.status, { "content-type": "application/json", ...headers });
    response.end(JSON.stringify(decision.kind === "payment-required"
      ? decision.body
      : decision.kind === "released"
        ? decision.resource
        : { state: decision.kind }));
  } catch (error) {
    if (error instanceof LayerXApplicationStateError) {
      const status = error.state === "pending" ? 202 : error.state === "refused" ? 402 : 503;
      response.writeHead(status, {
        "content-type": "application/json",
        ...(error.state === "pending" || error.state === "unknown" ? { "retry-after": "1" } : {}),
      }).end(JSON.stringify({ state: error.state, detail: error.message }));
      return;
    }
    const state = error instanceof MiddlewareError
      ? error.code === "payment-pending"
        ? "pending"
        : error.code === "fulfillment-conflict"
          ? "unknown"
          : "refused"
      : "unknown";
    const status = state === "pending" ? 202 : state === "refused" ? 400 : 503;
    response.writeHead(status, {
      "content-type": "application/json",
      ...(state === "pending" || state === "unknown" ? { "retry-after": "1" } : {}),
    }).end(JSON.stringify({ state, error: error instanceof MiddlewareError ? error.code : "internal-fault" }));
  }
}).listen(port, "127.0.0.1", () => {
  process.stdout.write(`${JSON.stringify({ environment: config.name, listening: port, path: "/paid" })}\n`);
});
