import WebSocket from "ws";
import { JsonRpcError } from "./rpc.js";

export type SubscriptionTopic = "receipts" | "checkpoints" | "account";

export interface SubscriptionEvent {
  readonly result: Record<string, unknown>;
  readonly cursor: bigint;
}

const MAX_CURSOR = 18446744073709551615n;

function object(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function remote(value: Record<string, unknown>): never {
  const error = value.error;
  if (!object(error) || !Number.isSafeInteger(error.code) || typeof error.message !== "string") throw new Error("Invalid subscription error");
  throw new JsonRpcError(error.code as number, error.message, error.data);
}

export function subscriptionCursor(value: unknown): bigint {
  if (typeof value !== "string" || !/^(0|[1-9][0-9]*)$/u.test(value)) throw new Error("Invalid subscription cursor");
  const cursor = BigInt(value);
  if (cursor > MAX_CURSOR) throw new Error("Invalid subscription cursor");
  return cursor;
}

export function subscriptionAcknowledgement(value: unknown, requestId: string): string {
  if (!object(value) || Object.keys(value).length !== 3 || value.jsonrpc !== "2.0" || value.id !== requestId) throw new Error("Invalid subscription acknowledgement");
  if ("error" in value) remote(value);
  if (typeof value.result !== "string" || value.result.length === 0 || Buffer.byteLength(value.result) > 256) throw new Error("Invalid subscription identifier");
  return value.result;
}

export function unsubscribeAcknowledgement(value: unknown, requestId: string): true {
  if (!object(value) || Object.keys(value).length !== 3 || value.jsonrpc !== "2.0" || value.id !== requestId) throw new Error("Invalid unsubscribe acknowledgement");
  if ("error" in value) remote(value);
  if (value.result !== true) throw new Error("Invalid unsubscribe acknowledgement");
  return true;
}

export function subscriptionNotification(value: unknown, subscription: string): SubscriptionEvent {
  if (!object(value) || Object.keys(value).length !== 3 || value.jsonrpc !== "2.0" || value.method !== "lx_subscription" || !object(value.params) || Object.keys(value.params).length !== 3 || value.params.subscription !== subscription || !object(value.params.result)) throw new Error("Invalid subscription notification");
  return { result: value.params.result, cursor: subscriptionCursor(value.params.cursor) };
}

export function subscriptionSelector(topic: SubscriptionTopic, account?: string, cursor?: bigint): string[] {
  if (!["receipts", "checkpoints", "account"].includes(topic) || (topic === "account") !== (account !== undefined) || (account !== undefined && !/^[0-9a-f]{64}$/u.test(account))) throw new Error("Invalid subscription selector");
  if (cursor !== undefined && (cursor < 0n || cursor > MAX_CURSOR)) throw new Error("Invalid subscription cursor");
  const selector = account === undefined ? [topic] : [topic, account];
  return cursor === undefined ? selector : [...selector, cursor.toString()];
}

export async function* subscribeRpc(endpoint: URL, authorization: string | undefined, requestId: string, topic: SubscriptionTopic, account?: string, cursor?: bigint, signal?: AbortSignal): AsyncGenerator<SubscriptionEvent> {
  const params = subscriptionSelector(topic, account, cursor);
  if (signal?.aborted) throw new Error("Subscription cancelled");
  const url = new URL(endpoint);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  url.pathname = `${url.pathname.replace(/\/$/u, "")}/ws`;
  const cancelId = `${requestId}:unsubscribe`;
  const socket = new WebSocket(url, { headers: authorization === undefined ? {} : { authorization }, handshakeTimeout: 30000, maxPayload: 9 * 1048576, followRedirects: false, perMessageDeflate: false });
  const queue: SubscriptionEvent[] = [];
  let subscription: string | undefined;
  let lastCursor = cursor;
  let failure: unknown;
  let wake: (() => void) | undefined;
  let settleCancel: (() => void) | undefined;
  let resolveReady!: () => void;
  let rejectReady!: (error: unknown) => void;
  const ready = new Promise<void>((resolve, reject) => { resolveReady = resolve; rejectReady = reject; });
  const fail = (error: unknown): void => { failure ??= error; rejectReady(error); wake?.(); settleCancel?.(); socket.terminate(); };
  const timer = setTimeout(() => fail(new Error("Subscription acknowledgement deadline exceeded")), 30000);
  const abort = (): void => fail(new Error("Subscription cancelled"));
  signal?.addEventListener("abort", abort, { once: true });
  socket.on("error", () => fail(new Error("Subscription transport failed")));
  socket.on("close", () => { failure ??= new Error("Subscription closed; reconnect from the last cursor and reconcile through reads"); rejectReady(failure); wake?.(); settleCancel?.(); });
  socket.on("open", () => socket.send(JSON.stringify({ jsonrpc: "2.0", id: requestId, method: "lx_subscribe", params })));
  socket.on("message", (data, binary) => {
    try {
      if (binary) throw new Error("Binary subscription frame refused");
      const bytes = Array.isArray(data) ? Buffer.concat(data) : data instanceof ArrayBuffer ? new Uint8Array(data) : data;
      const value: unknown = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
      if (subscription === undefined) { subscription = subscriptionAcknowledgement(value, requestId); clearTimeout(timer); resolveReady(); return; }
      if (object(value) && value.id === cancelId) { unsubscribeAcknowledgement(value, cancelId); settleCancel?.(); return; }
      if (queue.length >= 16) throw new Error("Subscription queue overflow; reconcile through reads");
      const event = subscriptionNotification(value, subscription);
      if (lastCursor !== undefined && event.cursor <= lastCursor) throw new Error("Subscription cursor regression or duplicate; reconcile through reads");
      queue.push(event);
      lastCursor = event.cursor;
      wake?.();
    } catch (error) { fail(error); }
  });
  const cancel = async (): Promise<void> => {
    if (subscription === undefined || socket.readyState !== WebSocket.OPEN) return;
    socket.send(JSON.stringify({ jsonrpc: "2.0", id: cancelId, method: "lx_unsubscribe", params: [subscription] }), () => undefined);
    await new Promise<void>(resolve => {
      const deadline = setTimeout(() => { settleCancel = undefined; resolve(); }, 5000);
      settleCancel = () => { clearTimeout(deadline); settleCancel = undefined; resolve(); };
    });
  };
  try {
    await ready;
    for (;;) {
      if (failure !== undefined) throw failure;
      const event = queue.shift();
      if (event !== undefined) { yield event; continue; }
      await new Promise<void>(resolve => { wake = resolve; });
      wake = undefined;
    }
  } finally {
    clearTimeout(timer);
    signal?.removeEventListener("abort", abort);
    await cancel();
    socket.terminate();
  }
}
