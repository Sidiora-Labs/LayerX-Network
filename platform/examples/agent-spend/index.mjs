import { AgentMiddleware } from "@sidiora/layerx-agent-middleware";
import { PlatformSdkError, ProductionClient, SDK_ERROR_CODES, SecretBytes } from "@sidiora/layerx-sdk";

const SDK_CODES = new Set(SDK_ERROR_CODES);
const RETRY_CLASSES = new Set(["never", "safe", "after", "unknown-outcome"]);

const required = (name) => {
  const value = process.env[name];
  if (value === undefined || value.length === 0) throw new Error(`missing_${name.toLowerCase()}`);
  return value;
};

const object = (value) => {
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error("invalid_service_response");
  return value;
};

const hex32 = (value) => {
  if (typeof value !== "string" || !/^[0-9a-fA-F]{64}$/.test(value)) throw new Error("invalid_service_response");
  return Uint8Array.from({ length: 32 }, (_, index) => Number.parseInt(value.slice(index * 2, index * 2 + 2), 16));
};

const endpoint = (name) => {
  const url = new URL(required(name));
  if (url.protocol !== "https:" && url.hostname !== "127.0.0.1" && url.hostname !== "localhost") throw new Error(`insecure_${name.toLowerCase()}`);
  return url;
};

class ServiceClient {
  constructor(url, token) {
    this.url = url;
    this.token = token;
  }

  async call(payload, idempotencyKey) {
    const headers = new Headers({ accept: "application/json", "content-type": "application/json" });
    this.token.withBytes((bytes) => headers.set("authorization", `Bearer ${new TextDecoder("utf-8", { fatal: true }).decode(bytes)}`));
    if (idempotencyKey !== undefined) headers.set("idempotency-key", idempotencyKey);
    let response;
    try {
      response = await fetch(this.url, { method: "POST", headers, body: JSON.stringify(payload) });
    } catch {
      throw new PlatformSdkError({ code: "transport-failure", retry: "unknown-outcome" });
    }
    const body = await response.json().catch(() => undefined);
    if (!response.ok) {
      const envelope = body === undefined ? {} : object(body);
      const failure = envelope.error === undefined ? envelope : object(envelope.error);
      const code = typeof failure.code === "string" && SDK_CODES.has(failure.code)
        ? failure.code
        : "internal-fault";
      const retry = typeof failure.retry === "string" && RETRY_CLASSES.has(failure.retry)
        ? failure.retry
        : "unknown-outcome";
      throw new PlatformSdkError({
        code,
        retry,
        ...(Number.isSafeInteger(failure.retry_after_ms) ? { retryAfterMs: failure.retry_after_ms } : {}),
        ...(Number.isSafeInteger(failure.protocol_result_code) ? { protocolResultCode: failure.protocol_result_code } : {}),
      });
    }
    return body;
  }
}

class AgentTransport {
  constructor(service) { this.service = service; }
  async call(call) {
    if (call.plane !== "agent") throw new PlatformSdkError({ code: "unavailable-capability", retry: "never" });
    const response = object(await this.service.call({ operation: call.operation, request: call.request }, call.idempotencyKey));
    if (response.ok !== true || response.result === undefined) throw new PlatformSdkError({ code: "decode-failure", retry: "never" });
    return response.result;
  }
}

class BudgetLedger {
  constructor(service) { this.service = service; }
  async reserve(request) {
    const response = object(await this.service.call({
      action: "reserve",
      request: {
        tenant: request.tenant,
        idempotency_key: request.idempotencyKey,
        request_digest: request.requestDigest,
        amount: request.amount,
        asset: request.asset,
      },
    }, request.idempotencyKey));
    return response.kind === "reserved"
      ? { kind: "reserved", reservation: reservationFromWire(response.reservation) }
      : response;
  }
  async hold(transition) {
    return reservationFromWire(await this.service.call({
      action: "hold",
      transition: transitionWire(transition, {
        approval_id: transition.approvalId,
        canonical_bytes_digest: transition.canonicalBytesDigest,
      }),
    }, transitionKey(transition, "hold")));
  }
  async commit(transition) {
    return reservationFromWire(await this.service.call({
      action: "commit",
      transition: transitionWire(transition, { receipt_digest: transition.receiptDigest }),
    }, transitionKey(transition, "commit")));
  }
  async release(transition) {
    return reservationFromWire(await this.service.call({
      action: "release",
      transition: transitionWire(transition, { refusal: refusalWire(transition.refusal) }),
    }, transitionKey(transition, "release")));
  }
}

const transitionWire = (transition, specific) => ({
  reservation_id: transition.reservationId,
  request_digest: transition.requestDigest,
  amount: transition.amount,
  asset: transition.asset,
  ...specific,
});

const transitionKey = (transition, action) => `${transition.requestDigest}:${action}`;

const refusalWire = (refusal) => ({
  code: refusal.code,
  retry: refusal.retry,
  ...(refusal.retryAfterMs === undefined ? {} : { retry_after_ms: refusal.retryAfterMs }),
  ...(refusal.protocolResultCode === undefined ? {} : { protocol_result_code: refusal.protocolResultCode }),
  ...(refusal.submissionState === undefined ? {} : { submission_state: refusal.submissionState }),
});

const reservationFromWire = (value) => {
  const reservation = object(value);
  const base = {
    reservationId: reservation.reservation_id,
    requestDigest: reservation.request_digest,
    amount: reservation.amount,
    asset: reservation.asset,
  };
  if (reservation.state === "reserved") return { ...base, state: "reserved" };
  if (reservation.state === "held") {
    return {
      ...base,
      state: "held",
      approvalId: reservation.approval_id,
      canonicalBytesDigest: reservation.canonical_bytes_digest,
    };
  }
  if (reservation.state === "committed") {
    return { ...base, state: "committed", receiptDigest: reservation.receipt_digest };
  }
  if (reservation.state === "released") {
    const refusal = object(reservation.refusal);
    return {
      ...base,
      state: "released",
      refusal: {
        code: refusal.code,
        retry: refusal.retry,
        ...(refusal.retry_after_ms === undefined ? {} : { retryAfterMs: refusal.retry_after_ms }),
        ...(refusal.protocol_result_code === undefined ? {} : { protocolResultCode: refusal.protocol_result_code }),
        ...(refusal.submission_state === undefined ? {} : { submissionState: refusal.submission_state }),
      },
    };
  }
  throw new Error("invalid_service_response");
};

class RemoteSigner {
  constructor(service) { this.service = service; }
  async sign(prepared) {
    const response = object(await this.service.call(
      { signing_preimage: prepared.signing_preimage, disclosure: prepared.disclosure },
      prepared.preparation_ref,
    ));
    if (typeof response.signature !== "string") throw new Error("invalid_service_response");
    return response.signature;
  }
}

class ReceiptResolver {
  constructor(service) { this.service = service; }
  async resolve(receiptRef) {
    const response = object(await this.service.call({ receipt_ref: receiptRef }));
    const batch = object(response.authorized_batch);
    if (typeof response.canonical_receipt_base64 !== "string") throw new Error("invalid_service_response");
    return {
      canonicalReceipt: Uint8Array.from(Buffer.from(response.canonical_receipt_base64, "base64")),
      authorizedBatch: {
        batchId: hex32(batch.batch_id),
        asset: hex32(batch.asset),
        previousStateRoot: hex32(batch.previous_state_root),
        resultingStateRoot: hex32(batch.resulting_state_root),
        sequencerPublicKey: hex32(batch.sequencer_public_key),
      },
    };
  }
}

const token = new SecretBytes(new TextEncoder().encode(required("LAYERX_TOKEN")));
const agentService = new ServiceClient(endpoint("LAYERX_AGENT_RPC_URL"), token);
const middleware = new AgentMiddleware({
  client: new ProductionClient(new AgentTransport(agentService)),
  budgets: new BudgetLedger(new ServiceClient(endpoint("LAYERX_BUDGET_SERVICE_URL"), token)),
  signer: new RemoteSigner(new ServiceClient(endpoint("LAYERX_SIGNER_SERVICE_URL"), token)),
  receipts: new ReceiptResolver(new ServiceClient(endpoint("LAYERX_RECEIPT_SERVICE_URL"), token)),
});

try {
  const request = object(JSON.parse(required("LAYERX_SPEND_REQUEST_JSON")));
  const result = await middleware.spend(request);
  process.stdout.write(JSON.stringify({
    kind: result.kind,
    ...(result.kind === "verified" ? { receiptDigest: Buffer.from(result.verification.receiptDigest).toString("hex") } : {}),
    ...(result.kind === "approval-hold" ? { approvalId: result.approval.approvalId } : {}),
    ...(result.kind === "refused" || result.kind === "budget-refused"
      ? {
          code: result.code,
          retry: result.retry,
          ...(result.retryAfterMs === undefined ? {} : { retryAfterMs: result.retryAfterMs }),
        }
      : {}),
    ...("reservation" in result ? { reservationState: result.reservation.state } : {}),
  }) + "\n");
  if (result.kind !== "verified" && result.kind !== "approval-hold" && result.kind !== "pending") process.exitCode = 2;
} finally {
  token.destroy();
}
