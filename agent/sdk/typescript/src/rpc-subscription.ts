import WebSocket from "ws";
import { JsonRpcError } from "./rpc.js";

export type SubscriptionTopic = "receipts" | "checkpoints" | "account";

function object(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function subscriptionAcknowledgement(value: unknown, requestId: string): string {
  if (!object(value) || Object.keys(value).length !== 3 || value.jsonrpc !== "2.0" || value.id !== requestId) throw new Error("Invalid subscription acknowledgement");
  if ("error" in value) {
    const error = value.error;
    if (!object(error) || !Number.isSafeInteger(error.code) || typeof error.message !== "string") throw new Error("Invalid subscription error");
    throw new JsonRpcError(error.code as number, error.message, error.data);
  }
  if (typeof value.result !== "string" || value.result.length === 0 || Buffer.byteLength(value.result) > 256) throw new Error("Invalid subscription identifier");
  return value.result;
}

export function subscriptionNotification(value: unknown, subscription: string): Record<string, unknown> {
  if (!object(value) || Object.keys(value).length !== 3 || value.jsonrpc !== "2.0" || value.method !== "lx_subscription" || !object(value.params) || Object.keys(value.params).length !== 2 || value.params.subscription !== subscription || !object(value.params.result)) throw new Error("Invalid subscription notification");
  return value.params.result;
}

export async function* subscribeRpc(endpoint: URL, authorization: string | undefined, requestId: string, topic: SubscriptionTopic, account?: string, signal?: AbortSignal): AsyncGenerator<Record<string, unknown>> {
  if (!["receipts", "checkpoints", "account"].includes(topic) || (topic === "account") !== (account !== undefined) || (account !== undefined && !/^[0-9a-f]{64}$/u.test(account))) throw new Error("Invalid subscription selector");
  if (signal?.aborted) throw new Error("Subscription cancelled");
  const url = new URL(endpoint);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  url.pathname = `${url.pathname.replace(/\/$/u, "")}/ws`;
  const socket = new WebSocket(url, { headers: authorization === undefined ? {} : { authorization }, handshakeTimeout: 30000, maxPayload: 9 * 1048576, followRedirects: false, perMessageDeflate: false });
  const queue: Record<string, unknown>[] = [];
  let subscription: string | undefined;
  let failure: unknown;
  let wake: (() => void) | undefined;
  let resolveReady!: () => void;
  let rejectReady!: (error: unknown) => void;
  const ready = new Promise<void>((resolve, reject) => { resolveReady = resolve; rejectReady = reject; });
  const fail = (error: unknown): void => { failure ??= error; rejectReady(error); wake?.(); socket.terminate(); };
  const timer = setTimeout(() => fail(new Error("Subscription acknowledgement deadline exceeded")), 30000);
  const abort = (): void => fail(new Error("Subscription cancelled"));
  signal?.addEventListener("abort", abort, { once: true });
  socket.on("error", () => fail(new Error("Subscription transport failed")));
  socket.on("close", () => { failure ??= new Error("Subscription closed; reconnect and reconcile through reads"); rejectReady(failure); wake?.(); });
  socket.on("open", () => socket.send(JSON.stringify({ jsonrpc: "2.0", id: requestId, method: "lx_subscribe", params: account === undefined ? [topic] : [topic, account] })));
  socket.on("message", (data, binary) => {
    try {
      if (binary) throw new Error("Binary subscription frame refused");
      const bytes = Array.isArray(data) ? Buffer.concat(data) : data instanceof ArrayBuffer ? new Uint8Array(data) : data;
      const value: unknown = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
      if (subscription === undefined) { subscription = subscriptionAcknowledgement(value, requestId); clearTimeout(timer); resolveReady(); return; }
      if (queue.length >= 16) throw new Error("Subscription queue overflow; reconcile through reads");
      queue.push(subscriptionNotification(value, subscription));
      wake?.();
    } catch (error) { fail(error); }
  });
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
    socket.terminate();
  }
}
