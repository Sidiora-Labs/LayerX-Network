import { MiddlewareError, type SellerPaymentAuthority, type SellerSettlementRequest, type SellerSettlementOutcome } from "./index.js";

export interface GrantDrawExecution {
  execute(request: { readonly principal: string; readonly requestDigest: string; readonly receive: string;
    readonly idempotencyKey: string; readonly requirements: SellerSettlementRequest["requirements"] }): Promise<SellerSettlementOutcome>;
}

export class GrantPaymentAuthority implements SellerPaymentAuthority {
  public constructor(private readonly draws: GrantDrawExecution, private readonly exact: SellerPaymentAuthority) {}
  public settle(request: SellerSettlementRequest): Promise<SellerSettlementOutcome> {
    if (request.requirements.scheme === "exact") return this.exact.settle(request);
    if (request.requirements.scheme !== "metered" && request.requirements.scheme !== "subscription") throw new MiddlewareError("unsupported-payment");
    const payload = request.payload.payload;
    if (Object.keys(payload).length !== 2 || typeof payload["receive"] !== "string"
      || !/^[0-9a-f]{1466}$/u.test(payload["receive"]) || typeof payload["idempotencyKey"] !== "string"
      || !/^[0-9a-f]{64}$/u.test(payload["idempotencyKey"])) throw new MiddlewareError("invalid-payment-payload");
    return this.draws.execute({ principal: request.principal, requestDigest: request.requestDigest,
      receive: payload["receive"], idempotencyKey: payload["idempotencyKey"], requirements: request.requirements });
  }
}
