import { DatabaseSync } from "node:sqlite";
import { verifyReceipt, type AuthorizedReceiptBatch } from "../verifier.js";
import { validateGrantDraw, type GrantOffer } from "./grant.js";
import { bindReceiveActivity } from "./activity.js";

export class PreparedGrantDraws {
  readonly #db: DatabaseSync;
  public constructor(path: string, private readonly actor: string, private readonly network: number,
    private readonly rpc: { send(canonical: string, commitment: "executed" | "batched" | "finalised"): Promise<Record<string, unknown>>; receipt(activity: string): Promise<Record<string, unknown>> },
    private readonly authority: (receipt: Uint8Array) => Promise<AuthorizedReceiptBatch>) {
    this.#db = new DatabaseSync(path);
    this.#db.exec("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=30000; CREATE TABLE IF NOT EXISTS grant_draws (idempotency_key TEXT PRIMARY KEY, principal TEXT NOT NULL, request_digest TEXT NOT NULL, canonical BLOB NOT NULL, receive BLOB NOT NULL, activity_id TEXT NOT NULL UNIQUE, period_key TEXT UNIQUE, attempted INTEGER NOT NULL DEFAULT 0, attempted_at INTEGER)");
  }
  public close(): void { this.#db.close(); }
  public register(principal: string, requestDigest: string, canonical: Uint8Array, receive: Uint8Array, key: string, periodKey?: string): void {
    if (!principal || !/^[0-9a-f]{64}$/u.test(requestDigest) || (periodKey !== undefined && !periodKey)) throw new Error("invalid-draw-registration");
    const activity = bindReceiveActivity(canonical, receive, this.actor, this.network, key);
    const existing = this.#db.prepare("SELECT * FROM grant_draws WHERE idempotency_key = ?").get(key);
    if (existing) {
      if (existing["principal"] !== principal || existing["request_digest"] !== requestDigest || existing["activity_id"] !== activity || existing["period_key"] !== (periodKey ?? null)) throw new Error("draw-registration-conflict");
      return;
    }
    this.#db.prepare("INSERT INTO grant_draws (idempotency_key, principal, request_digest, canonical, receive, activity_id, period_key) VALUES (?, ?, ?, ?, ?, ?, ?)").run(key, principal, requestDigest, canonical, receive, activity, periodKey ?? null);
  }
  public async execute(request: { readonly principal: string; readonly requestDigest: string; readonly receive: string; readonly idempotencyKey: string; readonly requirements: unknown }): Promise<{ readonly kind: "pending" } | { readonly kind: "settled"; readonly canonicalReceipt: Uint8Array; readonly authorizedBatch: AuthorizedReceiptBatch }> {
    const row = this.#db.prepare("SELECT * FROM grant_draws WHERE idempotency_key = ?").get(request.idempotencyKey);
    if (!row || row["principal"] !== request.principal || row["request_digest"] !== request.requestDigest
      || Buffer.from(row["receive"] as Uint8Array).toString("hex") !== request.receive) throw new Error("unregistered-grant-draw");
    const offer = request.requirements as GrantOffer;
    const receive = validateGrantDraw(row["receive"] as Uint8Array, offer, request.idempotencyKey, this.network, BigInt((row["attempted_at"] as number | null) ?? Math.floor(Date.now() / 1000)));
    if (offer.scheme === "subscription" && row["period_key"] === null) throw new Error("subscription-period-required");
    const claimed = this.#db.prepare("UPDATE grant_draws SET attempted = 1, attempted_at = ? WHERE idempotency_key = ? AND attempted = 0").run(Math.floor(Date.now() / 1000), request.idempotencyKey);
    let result: Record<string, unknown>;
    try {
      result = claimed.changes === 1 ? await this.rpc.send(Buffer.from(row["canonical"] as Uint8Array).toString("hex"), offer.extra.layerx.commitment) : await this.rpc.receipt(row["activity_id"] as string);
    } catch (error) {
      if (isProtocolPending(error)) return { kind: "pending" };
      throw error;
    }
    if (result["activity_id"] !== row["activity_id"]) throw new Error("draw-activity-mismatch");
    if (result["state"] === "pending") return { kind: "pending" };
    if ((result["state"] !== undefined && result["state"] !== "completed") || typeof result["receipt"] !== "string" || (result["receipt"].length > 2097152 || !/^(?:[0-9a-f]{2})+$/u.test(result["receipt"]))) throw new Error("draw-receipt-unavailable");
    const canonicalReceipt = Uint8Array.from(Buffer.from(result["receipt"], "hex"));
    const authorizedBatch = await this.authority(canonicalReceipt);
    const verified = await verifyReceipt(canonicalReceipt, authorizedBatch);
    const hex = (value: Uint8Array) => Buffer.from(value).toString("hex");
    if (verified.receipt.moduleId !== 1 || verified.receipt.operation !== 6
      || hex(verified.receipt.activityId) !== row["activity_id"] || hex(verified.receipt.from) !== receive.from
      || hex(verified.receipt.to) !== receive.to || hex(verified.receipt.asset) !== receive.asset
      || verified.receipt.amount !== BigInt(receive.amount)) throw new Error("draw-receipt-mismatch");
    return { kind: "settled", canonicalReceipt, authorizedBatch };
  }
}

function isProtocolPending(error: unknown): boolean {
  if (!(error instanceof Error) || !("code" in error) || !("data" in error)
    || (error as { readonly code?: unknown }).code !== -32001) return false;
  const data = (error as { readonly data?: unknown }).data;
  return data !== null && typeof data === "object" && !Array.isArray(data)
    && (data as Record<string, unknown>)["state"] === "pending";
}
