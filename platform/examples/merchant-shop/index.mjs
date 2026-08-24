import { createServer } from "node:http";
import { mkdir, open, readFile, rename } from "node:fs/promises";
import { basename, dirname, join } from "node:path";
import { MerchantMiddleware } from "@sidiora/layerx-merchant-middleware";
import { MiddlewareError, SellerMiddleware, encodeSettlementHeader } from "@sidiora/layerx-seller-middleware";

const required = (name) => {
  const value = process.env[name];
  if (value === undefined || value.length === 0) throw new Error(`missing_${name.toLowerCase()}`);
  return value;
};

const safeName = (value) => {
  if (!/^[A-Za-z0-9._-]{1,255}$/.test(value) || basename(value) !== value) throw new Error("invalid_identifier");
  return value;
};

const exactObject = (value) => {
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error("invalid_response");
  return value;
};

const hex32 = (value) => {
  const digits = typeof value === "string" && value.startsWith("0x") ? value.slice(2) : value;
  if (typeof digits !== "string" || !/^[0-9a-fA-F]{64}$/.test(digits)) throw new Error("invalid_response");
  return Uint8Array.from({ length: 32 }, (_, index) => Number.parseInt(digits.slice(index * 2, index * 2 + 2), 16));
};

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

const syncDirectory = async (path) => {
  const directory = await open(dirname(path), "r");
  try {
    await directory.sync();
  } finally {
    await directory.close();
  }
};

class JsonCatalog {
  constructor(items) {
    this.items = new Map(items.map((item) => [safeName(item.sku), Object.freeze({ ...item })]));
  }

  async get(sku) {
    return this.items.get(sku);
  }
}

class FileOrders {
  constructor(directory) {
    this.directory = directory;
  }

  path(orderId) {
    return join(this.directory, `${safeName(orderId)}.json`);
  }

  async open(request) {
    await mkdir(this.directory, { recursive: true, mode: 0o700 });
    const orderId = request.checkoutKey;
    const path = this.path(orderId);
    const order = { orderId, checkoutKey: request.checkoutKey, requestDigest: request.requestDigest, state: "awaiting-payment", quote: request.quote };
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

  async markPaid(orderId, requestDigest, receiptDigest, transaction) {
    const order = await this.getRequired(orderId);
    if (order.requestDigest !== requestDigest) throw new Error("order_conflict");
    if (order.state === "paid-verified") {
      if (order.receiptDigest !== receiptDigest || order.transaction !== transaction) throw new Error("order_conflict");
      return order;
    }
    const paid = { ...order, state: "paid-verified", receiptDigest, transaction };
    await atomicJson(this.path(orderId), paid);
    return paid;
  }

  async markRefused(orderId, requestDigest) {
    const order = await this.getRequired(orderId);
    if (order.requestDigest !== requestDigest || order.state === "paid-verified") throw new Error("order_conflict");
    const refused = { ...order, state: "refused" };
    await atomicJson(this.path(orderId), refused);
    return refused;
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
    this.url = new URL(url);
    this.token = token;
    if (this.url.protocol !== "https:" && this.url.hostname !== "127.0.0.1" && this.url.hostname !== "localhost") {
      throw new Error("insecure_settlement_url");
    }
  }

  async settle(request) {
    const response = await fetch(this.url, {
      method: "POST",
      headers: { authorization: `Bearer ${this.token}`, "content-type": "application/json" },
      body: JSON.stringify(request),
    });
    const result = exactObject(await response.json());
    if (!response.ok) throw new MiddlewareError("payment-refused");
    if (result.state === "pending") return { kind: "pending" };
    if (result.state === "refused") return { kind: "refused", reason: String(result.reason ?? "refused") };
    if (result.state !== "settled" || typeof result.receipt_base64 !== "string") throw new Error("invalid_response");
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
      if (stored.requestDigest !== proposed.requestDigest) throw new Error("fulfillment_conflict");
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
    const authorizedBatch = {
      batch_id: Buffer.from(proposed.authorizedBatch.batchId).toString("hex"),
      asset: Buffer.from(proposed.authorizedBatch.asset).toString("hex"),
      previous_state_root: Buffer.from(proposed.authorizedBatch.previousStateRoot).toString("hex"),
      resulting_state_root: Buffer.from(proposed.authorizedBatch.resultingStateRoot).toString("hex"),
      sequencer_public_key: Buffer.from(proposed.authorizedBatch.sequencerPublicKey).toString("hex"),
    };
    await atomicJson(path, {
      requestDigest: proposed.requestDigest,
      receipt: Buffer.from(proposed.canonicalReceipt).toString("base64"),
      authorizedBatch,
      resource,
    });
    return { ...proposed, resource };
  }
}

const catalogValue = JSON.parse(await readFile(required("LAYERX_CATALOG_FILE"), "utf8"));
if (!Array.isArray(catalogValue)) throw new Error("invalid_catalog");
const orders = new FileOrders(process.env.LAYERX_ORDER_DIR ?? "./orders");
const authority = new SettlementAuthority(required("LAYERX_SETTLEMENT_URL"), required("LAYERX_SETTLEMENT_TOKEN"));
const fulfillments = new FileFulfillments(process.env.LAYERX_FULFILLMENT_DIR ?? "./fulfillments");
const merchant = new MerchantMiddleware({
  catalog: new JsonCatalog(catalogValue),
  orders,
  sellers: { create: (paymentRequired) => new SellerMiddleware({ paymentRequired, authority, fulfillments }) },
  resourceUrl: (checkoutKey) => new URL(`/checkout/${encodeURIComponent(checkoutKey)}`, required("LAYERX_PUBLIC_URL")).toString(),
});

const readBody = async (request) => {
  const chunks = [];
  let size = 0;
  for await (const chunk of request) {
    size += chunk.length;
    if (size > 64 * 1024) throw new Error("request_too_large");
    chunks.push(chunk);
  }
  return JSON.parse(Buffer.concat(chunks).toString("utf8"));
};

const port = Number(process.env.PORT ?? "8080");
if (!Number.isSafeInteger(port) || port <= 0 || port > 65535) throw new Error("invalid_port");
createServer(async (request, response) => {
  try {
    if (request.method !== "POST" || request.url !== "/checkout") return response.writeHead(404).end();
    const body = exactObject(await readBody(request));
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
        : {};
    response.writeHead(status, { "content-type": "application/json", ...headers }).end(JSON.stringify({ kind: result.kind, order: result.order }));
  } catch (error) {
    response.writeHead(400, { "content-type": "application/json" }).end(JSON.stringify({ error: error.code ?? "invalid_request" }));
  }
}).listen(port, "127.0.0.1", () => process.stdout.write(JSON.stringify({ listening: port, path: "/checkout" }) + "\n"));
