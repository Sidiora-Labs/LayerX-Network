import { type AuthorizedReceiptBatch, type ReceiptVerification } from "@sidiora/layerx-sdk";
import { MiddlewareError, verifyPaymentReceipt, paymentCommitment, type PaymentRequirements, type PaymentCommitmentResolver } from "./index.js";

export function rpcObject(value: unknown): Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error("invalid-rpc-object");
  return value as Record<string, unknown>;
}

export function rpcHex(value: unknown, size?: number): Uint8Array {
  if (typeof value !== "string" || value.length === 0 || value.length > 2_097_152
    || !/^(?:[0-9a-f]{2})+$/u.test(value) || (size !== undefined && value.length !== size * 2)) throw new Error("invalid-rpc-hex");
  return Uint8Array.from(value.match(/../gu)!, (pair) => Number.parseInt(pair, 16));
}

export class PaymentRpcError extends Error {
  public constructor(public readonly code: number, public readonly data: unknown) { super(`JSON-RPC error ${code}`); }
}

export class PaymentRpc {
  #id = 0;
  readonly #url: URL;
  public constructor(endpoint: string, private readonly headers: Readonly<Record<string, string>> = {}) {
    this.#url = new URL(endpoint);
    if (this.#url.username || this.#url.password || this.#url.hash || this.#url.search
      || this.#url.pathname !== "/rpc"
      || (this.#url.protocol !== "https:" && !(this.#url.protocol === "http:" && ["localhost", "127.0.0.1", "[::1]"].includes(this.#url.hostname)))) throw new Error("invalid-rpc-endpoint");
  }
  public async call(method: string, params: readonly unknown[]): Promise<Record<string, unknown>> {
    const id = ++this.#id;
    const response = await fetch(this.#url, { method: "POST", redirect: "error", signal: AbortSignal.timeout(30_000),
      headers: { ...this.headers, "content-type": "application/json" }, body: JSON.stringify({ jsonrpc: "2.0", id, method, params }) });
    if (!response.ok) { await response.body?.cancel(); throw new Error(`rpc-http-${response.status}`); }
    const reader = response.body?.getReader();
    if (!reader) throw new Error("missing-rpc-body");
    const chunks: Uint8Array[] = []; let size = 0;
    for (;;) {
      const part = await reader.read(); if (part.done) break;
      size += part.value.length;
      if (size > 8_388_608) { await reader.cancel(); throw new Error("rpc-body-too-large"); }
      chunks.push(part.value);
    }
    const bytes = new Uint8Array(size); let offset = 0;
    for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.length; }
    const body = rpcObject(JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes)));
    if (body["jsonrpc"] !== "2.0" || body["id"] !== id || (("error" in body) === ("result" in body))) throw new Error("invalid-rpc-envelope");
    if ("error" in body) {
      const error = rpcObject(body["error"]);
      if (!Number.isInteger(error["code"]) || typeof error["message"] !== "string") throw new Error("invalid-rpc-error");
      throw new PaymentRpcError(error["code"] as number, error["data"]);
    }
    return rpcObject(body["result"]);
  }
  public async send(canonicalHex: string, commitment: "executed" | "batched" | "finalised"): Promise<Record<string, unknown>> {
    rpcHex(canonicalHex);
    if (canonicalHex.length > 1_048_576 || !["executed", "batched", "finalised"].includes(commitment)) throw new Error("invalid-rpc-submit");
    return this.call("lx_sendActivity", [canonicalHex, commitment]);
  }
  public receipt(activityId: string): Promise<Record<string, unknown>> { rpcHex(activityId, 32); return this.call("lx_getReceipt", [activityId]); }
  public status(activityId: string): Promise<Record<string, unknown>> { rpcHex(activityId, 32); return this.call("lx_getActivityStatus", [activityId]); }
}

export async function verifyRpcPayment(
  result: Record<string, unknown>, expectedActivity: string, expectedPayer: string,
  requirements: PaymentRequirements, authorizedBatch: AuthorizedReceiptBatch,
  commitments?: PaymentCommitmentResolver,
): Promise<{ readonly kind: "pending" } | { readonly kind: "verified"; readonly canonicalReceipt: Uint8Array; readonly authorizedBatch: AuthorizedReceiptBatch; readonly verification: ReceiptVerification }> {
  rpcHex(expectedActivity, 32); rpcHex(expectedPayer, 32);
  if (result["activity_id"] !== expectedActivity) throw new MiddlewareError("verification-failure");
  if (result["state"] === "pending") return { kind: "pending" };
  if (result["state"] !== undefined && result["state"] !== "completed") throw new MiddlewareError("payment-refused");
  const canonicalReceipt = rpcHex(result["receipt"]);
  const commitment = paymentCommitment(requirements.extra);
  if (result["commitment"] !== undefined && result["commitment"] !== commitment) throw new MiddlewareError("verification-failure");
  const verification = await verifyPaymentReceipt({ canonicalReceipt, authorizedBatch }, requirements, commitments);
  const hex = (value: Uint8Array) => Array.from(value, (byte) => byte.toString(16).padStart(2, "0")).join("");
  if (hex(verification.receipt.activityId) !== expectedActivity || hex(verification.receipt.from) !== expectedPayer) throw new MiddlewareError("verification-failure");
  return { kind: "verified", canonicalReceipt, authorizedBatch, verification };
}

export function rpcBatchEvidence(result: Record<string, unknown>, activityId: string, receipt: Uint8Array,
  trusted: Pick<import("./commitment.js").PaymentCommitmentEvidence, "networkId" | "authorization">): import("./commitment.js").PaymentCommitmentEvidence {
  const bundle = rpcObject(result["batch_evidence"]);
  const signed = rpcObject(bundle["signed_header"]);
  const proof = rpcObject(bundle["proof"]);
  const same = (left: Uint8Array, right: Uint8Array) => left.length === right.length && left.every((b, i) => b === right[i]);
  if (bundle["kind"] !== "receipt" || bundle["activity_id"] !== activityId
    || !same(rpcHex(bundle["canonical_value"]), receipt)
    || !same(rpcHex(signed["public_key"], 32), trusted.authorization.publicKey)
    || !same(rpcHex(signed["sequencer_id"], 32), trusted.authorization.sequencerId)
    || !Number.isSafeInteger(proof["leaf_index"]) || !Number.isSafeInteger(proof["leaf_count"])
    || !Array.isArray(proof["siblings"]) || proof["siblings"].length > 64) throw new MiddlewareError("verification-failure");
  return { ...trusted, canonicalHeader: rpcHex(signed["canonical_header"]), headerSignature: rpcHex(signed["signature"], 64),
    proof: { leafIndex: proof["leaf_index"] as number, leafCount: proof["leaf_count"] as number, siblings: proof["siblings"].map(value => rpcHex(value, 32)) } };
}
