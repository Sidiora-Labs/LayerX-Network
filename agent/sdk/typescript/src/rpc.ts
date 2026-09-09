import { subscribeRpc, type SubscriptionTopic } from "./rpc-subscription.js";
export type { SubscriptionTopic } from "./rpc-subscription.js";
import { createHash } from "node:crypto";
import * as http from "node:http";
import * as https from "node:https";
import { LayerXKeyCredential } from "./agent-http.js";

export type Commitment = "executed" | "batched" | "finalised";
export class JsonRpcError extends Error {
  public constructor(public readonly code: number, message: string, public readonly data?: unknown) { super(message); }
}

export class JsonRpcClient {
  readonly #endpoint: URL;
  #id = 0n;
  public constructor(endpoint: string | URL, private readonly credential?: LayerXKeyCredential) {
    this.#endpoint = new URL(endpoint);
    const u = this.#endpoint;
    if (!["https:", "http:"].includes(u.protocol) || u.username || u.password || u.search || u.hash ||
        (u.protocol === "http:" && !["localhost", "127.0.0.1", "[::1]"].includes(u.hostname))) throw new Error("Invalid RPC endpoint");
    const path = u.pathname.replace(/\/$/u, "");
    u.pathname = path.endsWith("/rpc") ? path : `${path}/rpc`;
  }
  public wallet(nativeAsset: string) { return {
    accounts: (did: string): Promise<Record<string, unknown>> => this.getBalances(did),
    balance: (did: string, asset: string): Promise<Record<string, unknown>> => this.getBalance(walletAccount(did, asset, nativeAsset)),
  }; }
  public subscribe(topic: SubscriptionTopic, account?: string, signal?: AbortSignal): AsyncGenerator<Record<string, unknown>> {
    let authorization: string | undefined;
    this.credential?.use(value => { authorization = value; });
    return subscribeRpc(this.#endpoint, authorization, (++this.#id).toString(), topic, account, signal);
  }
  public getAccount(account: string): Promise<Record<string, unknown>> { return this.call("lx_getAccount", [account]); }
  public getBalance(account: string): Promise<Record<string, unknown>> { return this.call("lx_getBalance", [account]); }
  public getBalances(did: string): Promise<Record<string, unknown>> { return this.call("lx_getBalances", [did]); }
  public getSequence(did: string): Promise<Record<string, unknown>> { return this.call("lx_getSequence", [did]); }
  public getReceipt(activity: string): Promise<Record<string, unknown>> { return this.call("lx_getReceipt", [activity]); }
  public getActivityStatus(activity: string): Promise<Record<string, unknown>> { return this.call("lx_getActivityStatus", [activity]); }
  public getBatchHeader(batch: string): Promise<Record<string, unknown>> { return this.call("lx_getBatchHeader", [batch]); }
  public getCheckpoint(checkpoint: string): Promise<Record<string, unknown>> { return this.call("lx_getCheckpoint", [checkpoint]); }
  public getNodeInfo(): Promise<Record<string, unknown>> { return this.call("lx_getNodeInfo", []); }
  public listAssets(): Promise<Record<string, unknown>> { return this.call("lx_listAssets", []); }
  public getAsset(asset: string): Promise<Record<string, unknown>> {
    if (!/^[0-9a-f]{64}$/u.test(asset)) throw new Error("Invalid asset identifier");
    return this.call("lx_getAsset", [asset]);
  }
  public estimateFee(canonical: Uint8Array): Promise<Record<string, unknown>> {
    if (canonical.length === 0 || canonical.length > 524288) throw new Error("Invalid activity length");
    return this.call("lx_estimateFee", [Buffer.from(canonical).toString("hex")]);
  }
  public getProof(kind: "activity" | "receipt" | "account", activity: string, account?: string): Promise<Record<string, unknown>> {
    if ((kind === "account") !== (account !== undefined)) throw new Error("Invalid proof selector");
    return this.call("lx_getProof", account === undefined ? [kind, activity] : [kind, activity, account]);
  }
  public sendActivity(canonical: Uint8Array, commitment: Commitment): Promise<Record<string, unknown>> {
    if (canonical.length === 0 || canonical.length > 524288 || !["executed", "batched", "finalised"].includes(commitment)) throw new Error("Invalid submission");
    return this.call("lx_sendActivity", [Buffer.from(canonical).toString("hex"), commitment]);
  }
  private async call(method: string, params: readonly string[]): Promise<Record<string, unknown>> {
    const id = (++this.#id).toString();
    const body = Buffer.from(JSON.stringify({ jsonrpc: "2.0", id, method, params }));
    if (body.length > 1048576 + 4096) throw new Error("RPC request too large");
    const headers: Record<string, string> = { "content-type": "application/json", accept: "application/json", "content-length": String(body.length) };
    if (this.credential) this.credential.use(value => { headers.authorization = value; });
    const bytes = await new Promise<Buffer>((resolve, reject) => {
      const request = (this.#endpoint.protocol === "https:" ? https : http).request(this.#endpoint, { method: "POST", headers }, response => {
        const chunks: Buffer[] = [];
        let size = 0;
        if (response.statusCode !== 200 || response.headers["content-type"]?.split(";")[0]?.trim() !== "application/json") {
          response.destroy(); reject(new Error("Invalid RPC HTTP response")); return;
        }
        response.on("data", (chunk: Buffer) => {
          size += chunk.length;
          if (size > 9 * 1048576) { request.destroy(new Error("RPC response too large")); return; }
          chunks.push(chunk);
        });
        response.on("error", reject);
        response.on("end", () => resolve(Buffer.concat(chunks)));
      });
      const timer = setTimeout(() => request.destroy(new Error("RPC deadline exceeded")), 30000);
      request.on("close", () => clearTimeout(timer));
      request.on("error", reject);
      request.end(body);
    });
    return decodeJsonRpcResponse(JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes)), id);
  }
}

export function decodeJsonRpcResponse(value: unknown, id: string): Record<string, unknown> {
  if (!object(value) || Object.keys(value).length !== 3 || value.jsonrpc !== "2.0" || value.id !== id) throw new Error("Invalid RPC response");
  if ("error" in value) {
    const e = value.error;
    if (!object(e) || !Number.isSafeInteger(e.code) || typeof e.message !== "string") throw new Error("Invalid RPC error");
    throw new JsonRpcError(e.code as number, e.message, e.data);
  }
  if (!object(value.result)) throw new Error("Invalid RPC result");
  return value.result;
}
function object(value: unknown): value is Record<string, unknown> { return typeof value === "object" && value !== null && !Array.isArray(value); }

export function walletAccount(did: string, asset: string, nativeAsset: string): string {
  if (!/^[a-z0-9._:-]{1,255}$/u.test(did) || did.startsWith(":") || did.endsWith(":") || did.includes("::") || did.includes(":asset:") ||
      !/^[0-9a-f]{64}$/u.test(asset) || !/^[0-9a-f]{64}$/u.test(nativeAsset)) throw new Error("Invalid wallet selector");
  const name = Buffer.from(asset === nativeAsset ? `agent:${did}:main` : `agent:${did}:asset:${asset}`);
  const length = Buffer.alloc(4); length.writeUInt32BE(name.length);
  return createHash("sha256").update("LX:ACCOUNT:v1").update(length).update(name).digest("hex");
}
