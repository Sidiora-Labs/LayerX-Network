import assert from "node:assert/strict";
import test from "node:test";
import { ProductionClient, SecretBytes } from "@sidiora/layerx-sdk";
import { MiddlewareError, encodePaymentRequiredHeader } from "@sidiora/layerx-seller-middleware";
import { BuyerMiddleware, LayerXPaymentHttpTransport, accountIdentifiers } from "@sidiora/layerx-buyer-middleware";
import { CONFORMANCE_PROTOCOL_VERSION } from "../dist/receipts.js";
import { SERVICE_PAYEE_ACCOUNT, assertServiceAccountPayTo, servicePayee } from "../service.mjs";

const toHex = (value) => Buffer.from(value).toString("hex");

const buyer = new BuyerMiddleware({
  client: new ProductionClient(new LayerXPaymentHttpTransport({
    baseUrl: "http://127.0.0.1:1/",
    bearerToken: new SecretBytes(new TextEncoder().encode("unused-conformance-token")),
  })),
  source: "acct:conformance-buyer",
  protocolVersion: CONFORMANCE_PROTOCOL_VERSION,
  supported: [{ scheme: "exact", network: "layerx:testnet" }],
  authorizedBatches: { resolve: () => Promise.reject(new Error("no receipt is resolved in this test")) },
});

const servedOffer = (account, payTo) => ({
  x402Version: 2,
  resource: {
    url: "http://127.0.0.1:8081/paid",
    description: "conformance paid resource",
  },
  accepts: [{
    scheme: "exact",
    network: "layerx:testnet",
    amount: "250000",
    asset: "bb".repeat(32),
    payTo,
    maxTimeoutSeconds: 120,
    extra: { layerx: { commitment: "executed", account, currency: "LXP" } },
  }],
});

test("the conformance payee is the account identifier the advertised account derives", async () => {
  const payee = toHex(await servicePayee(SERVICE_PAYEE_ACCOUNT));
  const identifiers = (await accountIdentifiers(SERVICE_PAYEE_ACCOUNT)).map(toHex);
  assert.ok(identifiers.includes(payee), "the derived payee must be one of the account's identifiers");
  await assertServiceAccountPayTo(SERVICE_PAYEE_ACCOUNT, payee);
  for (const identifier of identifiers) await assertServiceAccountPayTo(SERVICE_PAYEE_ACCOUNT, identifier);
});

test("a payee that the advertised account does not derive is refused by variable name", async () => {
  await assert.rejects(
    assertServiceAccountPayTo(SERVICE_PAYEE_ACCOUNT, "aa".repeat(32)),
    (error) => error instanceof Error
      && error.message.includes("LAYERX_PAY_TO")
      && error.message.includes("LAYERX_ACCOUNT")
      && error.message.includes(SERVICE_PAYEE_ACCOUNT),
  );
});

test("the served offer parses for the buyer and names the derived payee", async () => {
  const payee = toHex(await servicePayee(SERVICE_PAYEE_ACCOUNT));
  const header = encodePaymentRequiredHeader(servedOffer(SERVICE_PAYEE_ACCOUNT, payee));
  const parsed = buyer.parseOffer(header);
  assert.equal(parsed.accepted.payTo, payee);
  assert.equal(parsed.accepted.extra.layerx.account, SERVICE_PAYEE_ACCOUNT);
  assert.equal(parsed.accepted.amount, "250000");
  await assertServiceAccountPayTo(parsed.accepted.extra.layerx.account, parsed.accepted.payTo);
});

test("an offer the buyer cannot support is refused rather than parsed", () => {
  const unsupported = servedOffer(SERVICE_PAYEE_ACCOUNT, "aa".repeat(32));
  const header = encodePaymentRequiredHeader({
    ...unsupported,
    accepts: [{ ...unsupported.accepts[0], network: "layerx:unknown" }],
  });
  assert.throws(
    () => buyer.parseOffer(header),
    (error) => error instanceof MiddlewareError && error.code === "unsupported-payment",
  );
});
