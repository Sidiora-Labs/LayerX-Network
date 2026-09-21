import assert from "node:assert/strict";
import test from "node:test";
import { DEFAULT_PROTOCOL_VERSION, ProductionClient, STATE_COMMITMENT_PROTOCOL_VERSION, SecretBytes } from "@sidiora/layerx-sdk";
import { MiddlewareError } from "@sidiora/layerx-seller-middleware";
import { BuyerMiddleware, LayerXPaymentHttpTransport, accountIdentifiers } from "../dist/index.js";

const buyerConfig = (protocolVersion) => ({
  client: new ProductionClient(new LayerXPaymentHttpTransport({
    baseUrl: "http://127.0.0.1:1/",
    bearerToken: new SecretBytes(new TextEncoder().encode("unused-buyer-test-token")),
  })),
  source: "acct:buyer-protocol-test",
  protocolVersion,
  supported: [{ scheme: "exact", network: "layerx:beta" }],
  authorizedBatches: { resolve: () => Promise.reject(new Error("no receipt is resolved in this test")) },
});

test("a buyer configuration without a usable protocol version is refused by name", () => {
  for (const declared of [undefined, null, 0, 1, 4, "3", 3.5]) {
    assert.throws(
      () => new BuyerMiddleware(buyerConfig(declared)),
      (error) => error instanceof MiddlewareError && error.code === "missing-protocol-version",
      `declared protocol version ${String(declared)} must be refused by name`,
    );
  }
});

test("a buyer carries the protocol version its configuration declares", () => {
  assert.equal(new BuyerMiddleware(buyerConfig(STATE_COMMITMENT_PROTOCOL_VERSION)).protocolVersion, STATE_COMMITMENT_PROTOCOL_VERSION);
  assert.equal(new BuyerMiddleware(buyerConfig(DEFAULT_PROTOCOL_VERSION)).protocolVersion, DEFAULT_PROTOCOL_VERSION);
});

test("the advertised account derives the payee the buyer requires an offer to name", async () => {
  const account = "agent:did:layerx:conformance-seller:main";
  const identifiers = await accountIdentifiers(account);
  assert.equal(identifiers.length, 2);
  for (const identifier of identifiers) assert.equal(identifier.length, 32);
  const native = Buffer.from(identifiers[identifiers.length - 1]).toString("hex");
  assert.match(native, /^[0-9a-f]{64}$/u);
  assert.notEqual(native, "aa".repeat(32));
});
