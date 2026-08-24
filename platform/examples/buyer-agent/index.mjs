import { BuyerMiddleware, LayerXPaymentHttpTransport } from "@sidiora/layerx-buyer-middleware";
import { PlatformSdkError, ProductionClient, SecretBytes } from "@sidiora/layerx-sdk";
import {
  LayerXApplicationStateError,
  ReceiptAuthorityClient,
  loadApplicationConfig,
  requiredEnvironment,
} from "../support/runtime.mjs";

export function platform_ref_buyer() {
  return "buyer-middleware-metered-api-receipt-verified";
}

const config = await loadApplicationConfig(import.meta.url, "buyer-agent");
const rawToken = requiredEnvironment(config.tokenEnvironment);
const token = new SecretBytes(new TextEncoder().encode(rawToken));
const authority = new ReceiptAuthorityClient(config.receiptAuthorityUrl, rawToken);
const buyer = new BuyerMiddleware({
  client: new ProductionClient(new LayerXPaymentHttpTransport({ baseUrl: config.humanUrl, bearerToken: token })),
  source: requiredEnvironment(config.sourceEnvironment),
  supported: [{ scheme: config.scheme, network: config.network }],
  authorizedBatches: authority,
});

try {
  const result = await buyer.fetch(
    config.resourceUrl,
    { method: "GET", headers: { accept: "application/json" } },
    requiredEnvironment(config.idempotencyEnvironment),
  );
  if (result.kind === "pending" || result.kind === "unknown" || result.kind === "refused") {
    process.stdout.write(`${JSON.stringify({ environment: config.name, state: result.kind, result })}\n`);
    process.exitCode = result.kind === "pending" ? 2 : result.kind === "unknown" ? 3 : 4;
  } else if (result.kind === "not-payment-required") {
    await result.response.body?.cancel();
    throw new LayerXApplicationStateError("refused", `metered_resource_did_not_require_payment_http_${result.response.status}`);
  } else {
    if (!result.response.ok) {
      await result.response.body?.cancel();
      throw new LayerXApplicationStateError("unknown", `paid_resource_http_${result.response.status}`);
    }
    const body = await result.response.text();
    process.stdout.write(`${JSON.stringify({
      environment: config.name,
      state: "paid",
      status: result.response.status,
      receiptDigest: result.payment.receiptDigest,
      verification: result.settlement.verification.level,
      body,
    })}\n`);
  }
} catch (error) {
  const stateError = classifyBoundaryError(error);
  if (stateError === undefined) throw error;
  process.stdout.write(`${JSON.stringify({ environment: config.name, state: stateError.state, detail: stateError.message })}\n`);
  process.exitCode = stateError.state === "pending" ? 2 : stateError.state === "unknown" ? 3 : 4;
} finally {
  token.destroy();
}

function classifyBoundaryError(error) {
  if (error instanceof LayerXApplicationStateError) return error;
  if (error instanceof PlatformSdkError) {
    const refused = ["invalid-argument", "capability-refusal", "policy-refusal", "budget-refusal", "unavailable-capability"].includes(error.code);
    return new LayerXApplicationStateError(refused ? "refused" : "unknown", error.code);
  }
  if (error?.name === "MiddlewareError" && typeof error.code === "string") {
    if (error.code === "payment-pending") return new LayerXApplicationStateError("pending", error.code);
    const refused = [
      "invalid-payment-required",
      "invalid-payment-payload",
      "requirements-mismatch",
      "unsupported-payment",
      "payment-refused",
      "verification-failure",
    ].includes(error.code);
    return new LayerXApplicationStateError(refused ? "refused" : "unknown", error.code);
  }
  if (error instanceof TypeError) return new LayerXApplicationStateError("unknown", "resource_transport_failure");
  return undefined;
}
