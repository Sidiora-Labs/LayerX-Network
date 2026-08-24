import { createServer } from "node:http";
import { mkdir, open, readFile, rename } from "node:fs/promises";
import { basename, dirname, join, resolve } from "node:path";
import { MerchantError, MerchantMiddleware, MerchantSettlementWebhooks } from "@sidiora/layerx-merchant-middleware";
import {
  MiddlewareError,
  SellerMiddleware,
  VerifiedWebhookConsumer,
  encodeSettlementHeader,
} from "@sidiora/layerx-seller-middleware";
import {
  LayerXApplicationStateError,
  ReceiptAuthorityClient,
  exactObject,
  hex32,
  loadApplicationConfig,
  requiredEnvironment,
  secureUrl,
} from "./runtime.mjs";

export function platform_ref_merchant() {
  return "merchant-middleware-receipt-backed-orders-and-webhooks";
}

const safeName = (value) => {
  if (!/^[A-Za-z0-9._-]{1,255}$/u.test(value) || basename(value) !== value) throw new Error("invalid_identifier");
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

const atomicJson = async (path, value) => {
  const temporary = `${path}.${process.pid}.${crypto.randomUUID()}.tmp`;
  const file = await open(temporary, "wx", 0o600);
  try {
    await file.writeFile(JSON.stringify(value), "utf8");
    await file.sync();
  } finally {
    await file.close();
  }
  await rename(temporary, path);
  await syncDirectory(path);
};

class DeclaredCatalog {
  constructor(item) {
    this.item = Object.freeze(item);
  }

  async get(sku) {
    return sku === this.item.sku ? this.item : undefined;
  }
}

class FileOrders {
  constructor(directory) {
    this.directory = directory;
    this.serial = new Map();
  }

  path(orderId) {
    return join(this.directory, `${safeName(orderId)}.json`);
  }

  async open(request) {
    await mkdir(this.directory, { recursive: true, mode: 0o700 });
    const orderId = request.checkoutKey;
    const path = this.path(orderId);
    const order = { orderId, ...request, state: "awaiting-payment" };
    try {
      const file = await open(path, "wx", 0o600);
      try {
        await file.writeFile(JSON.stringify(order), "utf8");
        await file.sync();
      } finally {
        await file.close();
      }
      await syncDirectory(path);
      return order;
    } catch (error) {
      if (error.code !== "EEXIST") throw error;
      return this.getRequired(orderId);
    }
  }

  async releaseResource(orderId) {
    return this.getRequired(orderId);
  }

  async locked(orderId, operation) {
    const previous = this.serial.get(orderId) ?? Promise.resolve();
    const current = previous.then(operation, operation);
    const tracked = current.catch(() => undefined);
    this.serial.set(orderId, tracked);
    try {
      return await current;
    } finally {
      if (this.serial.get(orderId) === tracked) this.serial.delete(orderId);
    }
  }

  async markPaid(orderId, requestDigest, receiptDigest, transaction) {
    return this.locked(orderId, async () => {
      const order = await this.getRequired(orderId);
      if (order.requestDigest !== requestDigest) throw new MerchantError("order-conflict");
      if (order.state === "paid-verified") {
        if (order.receiptDigest !== receiptDigest || order.transaction !== transaction) {
          throw new MerchantError("order-conflict");
        }
        return order;
      }
      if (order.state === "refused") throw new MerchantError("order-conflict");
      const paid = { ...order, state: "paid-verified", receiptDigest, transaction };
      await atomicJson(this.path(orderId), paid);
      return paid;
    });
  }

  async markRefused(orderId, requestDigest) {
    return this.locked(orderId, async () => {
      const order = await this.getRequired(orderId);
      if (order.requestDigest !== requestDigest || order.state === "paid-verified") {
        throw new MerchantError("order-conflict");
      }
      const refused = { ...order, state: "refused" };
      await atomicJson(this.path(orderId), refused);
      return refused;
    });
  }

  async get(orderId) {
    try {
      return exactObject(JSON.parse(await readFile(this.path(orderId), "utf8")));
    } catch (error) {
      if (error.code === "ENOENT") return undefined;
      throw error;
    }
  }

  async getRequired(orderId) {
    const order = await this.get(orderId);
    if (order === undefined) throw new Error("order_missing");
    return order;
  }
}

class SettlementAuthority {
  constructor(url, token) {
    this.url = secureUrl(url);
    this.token = token;
  }

  async settle(request) {
    let response;
    try {
      response = await fetch(this.url, {
        method: "POST",
        headers: { authorization: `Bearer ${this.token}`, "content-type": "application/json" },
        body: JSON.stringify(request),
      });
    } catch {
      throw new LayerXApplicationStateError("unknown", "settlement_authority_unreachable");
    }
    const body = await response.json().catch(() => undefined);
    if (response.status === 202) return { kind: "pending" };
    if (response.status === 408 || response.status === 409 || response.status === 425 || response.status >= 500) {
      throw new LayerXApplicationStateError("unknown", `settlement_authority_http_${response.status}`);
    }
    if (!response.ok) return { kind: "refused", reason: `settlement_authority_http_${response.status}` };
    const result = exactObject(body?.result ?? body);
    if (result.state === "pending") return { kind: "pending" };
    if (result.state === "refused") return { kind: "refused", reason: String(result.reason ?? "refused") };
    if (result.state === "unknown") throw new LayerXApplicationStateError("unknown", "settlement_unknown");
    if (result.state !== "settled" || typeof result.receipt_base64 !== "string") {
      throw new LayerXApplicationStateError("unknown", "invalid_settlement_response");
    }
    return {
      kind: "settled",
      canonicalReceipt: Uint8Array.from(Buffer.from(result.receipt_base64, "base64")),
      authorizedBatch: parseBatch(result.authorized_batch),
    };
  }
}

class FileFulfillments {
  constructor(directory) {
    this.directory = directory;
  }

  async fulfill(proposed, release) {
    await mkdir(this.directory, { recursive: true, mode: 0o700 });
    const path = join(this.directory, `${safeName(proposed.idempotencyKey)}.json`);
    try {
      const stored = exactObject(JSON.parse(await readFile(path, "utf8")));
      if (stored.requestDigest !== proposed.requestDigest) throw new MiddlewareError("fulfillment-conflict");
      return {
        ...proposed,
        canonicalReceipt: Uint8Array.from(Buffer.from(stored.receipt, "base64")),
        authorizedBatch: parseBatch(stored.authorizedBatch),
        resource: stored.resource,
      };
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
      return this.fulfill(proposed, release);
    }
  }
}

class FileWebhookDeliveries {
  constructor(directory) {
    this.directory = directory;
    this.serial = new Map();
  }

  async locked(deliveryId, operation) {
    const previous = this.serial.get(deliveryId) ?? Promise.resolve();
    const current = previous.then(operation, operation);
    const tracked = current.catch(() => undefined);
    this.serial.set(deliveryId, tracked);
    try {
      return await current;
    } finally {
      if (this.serial.get(deliveryId) === tracked) this.serial.delete(deliveryId);
    }
  }

  async claim(claim) {
    return this.locked(claim.deliveryId, async () => {
      await mkdir(this.directory, { recursive: true, mode: 0o700 });
      const path = join(this.directory, `${safeName(claim.deliveryId)}.json`);
      let stored;
      try {
        stored = exactObject(JSON.parse(await readFile(path, "utf8")));
      } catch (error) {
        if (error.code !== "ENOENT") throw error;
      }
      if (stored !== undefined && stored.payloadDigest !== claim.payloadDigest) return "conflict";
      if (stored?.state === "completed") return "completed";
      if (stored?.state === "processing" && stored.leaseUntilMs > Date.now()) return "processing";
      await atomicJson(path, { ...claim, state: "processing" });
      return "claimed";
    });
  }

  async complete(deliveryId, payloadDigest) {
    await this.transition(deliveryId, payloadDigest, "completed");
  }

  async release(deliveryId, payloadDigest) {
    await this.transition(deliveryId, payloadDigest, "released");
  }

  async transition(deliveryId, payloadDigest, state) {
    await this.locked(deliveryId, async () => {
      const path = join(this.directory, `${safeName(deliveryId)}.json`);
      const stored = exactObject(JSON.parse(await readFile(path, "utf8")));
      if (stored.payloadDigest !== payloadDigest) throw new MiddlewareError("webhook-replay");
      await atomicJson(path, { ...stored, state });
    });
  }
}

const parseWebhookKeys = (value) => {
  const input = exactObject(JSON.parse(value));
  const result = {};
  for (const [keyId, publicKey] of Object.entries(input)) {
    if (!/^[A-Za-z0-9._-]{1,64}$/u.test(keyId)) throw new Error("invalid_webhook_key_id");
    result[keyId] = hex32(publicKey);
  }
  if (Object.keys(result).length === 0) throw new Error("missing_webhook_key");
  return result;
};

const readBody = async (request) => {
  const chunks = [];
  let size = 0;
  for await (const chunk of request) {
    size += chunk.length;
    if (size > 64 * 1024) throw new Error("request_too_large");
    chunks.push(chunk);
  }
  return Uint8Array.from(Buffer.concat(chunks));
};

const singleHeader = (request, name) => {
  const value = request.headers[name];
  if (typeof value !== "string") throw new MiddlewareError("invalid-webhook");
  return value;
};

export async function runMerchantApplication(moduleUrl, application) {
  const config = await loadApplicationConfig(moduleUrl, application);
  const token = requiredEnvironment(config.tokenEnvironment);
  const state = resolve(config.directory, config.stateDirectory);
  const orders = new FileOrders(join(state, "orders"));
  const authority = new SettlementAuthority(config.settlementUrl, token);
  const fulfillments = new FileFulfillments(join(state, "fulfillments"));
  const merchant = new MerchantMiddleware({
    catalog: new DeclaredCatalog({
      sku: "metered-report",
      title: "Receipt-backed market report",
      unitAmount: requiredEnvironment(config.priceEnvironment),
      asset: requiredEnvironment(config.assetEnvironment),
      payTo: requiredEnvironment(config.payToEnvironment),
      scheme: config.scheme,
      network: config.network,
      maxTimeoutSeconds: 60,
    }),
    orders,
    sellers: { create: (paymentRequired) => new SellerMiddleware({ paymentRequired, authority, fulfillments }) },
    resourceUrl: (checkoutKey) => new URL(`/orders/${encodeURIComponent(checkoutKey)}`, config.publicUrl).toString(),
  });
  const receiptAuthority = new ReceiptAuthorityClient(config.receiptAuthorityUrl, token);
  const webhook = new MerchantSettlementWebhooks(
    new VerifiedWebhookConsumer({
      publicKeys: parseWebhookKeys(requiredEnvironment(config.webhookKeysEnvironment)),
      deliveries: new FileWebhookDeliveries(join(state, "webhook-deliveries")),
    }),
    orders,
    { resolve: (receiptRef) => receiptAuthority.resolveReference(receiptRef) },
  );
  const port = Number(config.port);
  if (!Number.isSafeInteger(port) || port <= 0 || port > 65535) throw new Error("invalid_port");
  createServer(async (request, response) => {
    try {
      if (request.method === "GET" && request.url?.startsWith("/orders/")) {
        const order = await orders.get(decodeURIComponent(request.url.slice(8)));
        response.writeHead(order === undefined ? 404 : 200, { "content-type": "application/json" })
          .end(JSON.stringify(order ?? { state: "missing" }));
        return;
      }
      if (request.method === "POST" && request.url === "/webhooks/settlement") {
        const raw = await readBody(request);
        const result = await webhook.consume(raw, {
          id: singleHeader(request, "layerx-webhook-id"),
          timestamp: singleHeader(request, "layerx-webhook-timestamp"),
          keyId: singleHeader(request, "layerx-webhook-key-id"),
          signature: singleHeader(request, "layerx-webhook-signature"),
        });
        response.writeHead(result === "processing" ? 202 : 200, { "content-type": "application/json" })
          .end(JSON.stringify({ state: result }));
        return;
      }
      if (request.method !== "POST" || request.url !== "/checkout") {
        response.writeHead(404).end();
        return;
      }
      const body = exactObject(JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(await readBody(request))));
      const result = await merchant.checkout(
        String(body.principal),
        String(body.checkout_key),
        body.lines,
        typeof request.headers["payment-signature"] === "string" ? request.headers["payment-signature"] : undefined,
      );
      const status = result.kind === "payment-required" ? 402 : result.kind === "pending" ? 202 : result.kind === "paid" ? 200 : 402;
      const headers = result.kind === "payment-required"
        ? result.decision.headers
        : result.kind === "paid"
          ? { "payment-response": encodeSettlementHeader(result.settlement) }
          : result.kind === "pending" ? { "retry-after": "1" } : {};
      response.writeHead(status, { "content-type": "application/json", ...headers })
        .end(JSON.stringify({ state: result.kind, order: result.order }));
    } catch (error) {
      const state = error instanceof LayerXApplicationStateError
        ? error.state
        : error instanceof MiddlewareError && error.code === "payment-pending"
          ? "pending"
          : error instanceof MiddlewareError && error.code === "fulfillment-conflict"
            ? "unknown"
            : error instanceof MiddlewareError || error instanceof MerchantError || error instanceof SyntaxError
          ? "refused"
          : "unknown";
      const status = state === "pending" ? 202 : state === "refused" ? 400 : 503;
      response.writeHead(status, {
        "content-type": "application/json",
        ...(state === "pending" || state === "unknown" ? { "retry-after": "1" } : {}),
      }).end(JSON.stringify({ state, error: error.code ?? error.message }));
    }
  }).listen(port, "127.0.0.1", () => {
    process.stdout.write(`${JSON.stringify({ application, environment: config.name, listening: port, checkout: "/checkout", webhook: "/webhooks/settlement" })}\n`);
  });
}
