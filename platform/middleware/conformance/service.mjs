import { spawn } from "node:child_process";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  encodePaymentPayloadHeader,
  verifyPaymentReceipt,
} from "@sidiora/layerx-seller-middleware";
import {
  BuyerMiddleware,
  LayerXPaymentHttpTransport,
} from "@sidiora/layerx-buyer-middleware";
import { ProductionClient, SecretBytes } from "@sidiora/layerx-sdk";
import {
  ConformanceSequencer,
  buildSignedReceipt,
  buyerPayload,
  encodeBase64,
  fixedBytes,
  toHex,
} from "./dist/receipts.js";
import { FixedBatchResolver, assert } from "./dist/support.js";

const EXAMPLE_ENTRY = fileURLToPath(new URL("../../examples/paid-api/index.mjs", import.meta.url));

// Boots the shipped paid-api seller example as a real HTTP service and drives
// it with the buyer and seller middleware over the wire, proving both roles
// against a running server rather than an in-process object graph.
export async function runServiceScenarios(suite) {
  const sequencer = await ConformanceSequencer.generate();
  const payTo = fixedBytes(0xaa);
  const asset = fixedBytes(0xbb);
  const amount = 250_000n;
  const facts = {
    asset,
    payTo,
    amount,
    from: fixedBytes(0xcc),
    batchId: fixedBytes(0xd1),
    previousStateRoot: fixedBytes(0xd2),
    resultingStateRoot: fixedBytes(0xd3),
  };
  const receipt = await buildSignedReceipt(sequencer, facts);
  const resolver = new FixedBatchResolver(receipt.authorizedBatch);

  const workDir = await mkdtemp(join(tmpdir(), "layerx-mw-conformance-"));
  const resourceFile = join(workDir, "resource.json");
  const resourceBody = JSON.stringify({ secret: "paid-resource-body", price: amount.toString() });
  await writeFile(resourceFile, resourceBody, "utf8");

  const port = 8080 + Math.floor(Math.random() * 2000);
  const environment = {
    ...process.env,
    PORT: port.toString(),
    LAYERX_RESOURCE_FILE: resourceFile,
    LAYERX_FULFILLMENT_DIR: join(workDir, "fulfillments"),
    LAYERX_RESOURCE_URL: `http://127.0.0.1:${port}/paid`,
    LAYERX_RESOURCE_DESCRIPTION: "conformance paid resource",
    LAYERX_X402_SCHEME: "exact",
    LAYERX_X402_NETWORK: "layerx:testnet",
    LAYERX_PRICE: amount.toString(),
    LAYERX_ASSET: toHex(asset),
    LAYERX_PAY_TO: toHex(payTo),
    LAYERX_PAYMENT_TIMEOUT_SECONDS: "120",
    LAYERX_AUTHORIZED_BATCH_JSON: JSON.stringify({
      batchId: toHex(receipt.authorizedBatch.batchId),
      asset: toHex(receipt.authorizedBatch.asset),
      previousStateRoot: toHex(receipt.authorizedBatch.previousStateRoot),
      resultingStateRoot: toHex(receipt.authorizedBatch.resultingStateRoot),
      sequencerPublicKey: toHex(receipt.authorizedBatch.sequencerPublicKey),
    }),
  };

  let child;
  try {
    child = spawn(process.execPath, [EXAMPLE_ENTRY], { env: environment, stdio: ["ignore", "pipe", "pipe"] });
    await waitForListening(child, 15_000);
    const base = `http://127.0.0.1:${port}/paid`;
    const buyer = new BuyerMiddleware({
      client: new ProductionClient(new LayerXPaymentHttpTransport({
        baseUrl: `http://127.0.0.1:${port}/`,
        bearerToken: new SecretBytes(new TextEncoder().encode("unused-conformance-token")),
      })),
      source: "acct:conformance-buyer",
      supported: [{ scheme: "exact", network: "layerx:testnet" }],
      authorizedBatches: resolver,
    });

    await suite.check("service: an unpaid request is answered with a 402 offer the buyer can parse", async () => {
      const response = await fetch(base, { method: "GET", headers: { accept: "application/json" } });
      assert(response.status === 402, `expected a 402, received ${response.status}`);
      const header = response.headers.get("PAYMENT-REQUIRED");
      assert(header !== null, "the 402 must carry a PAYMENT-REQUIRED offer");
      await response.text();
      const parsed = buyer.parseOffer(header ?? "");
      assert(parsed.accepted.amount === amount.toString(), "the parsed offer must quote the server price");
      assert(parsed.accepted.payTo === toHex(payTo), "the parsed offer must name the server payee");
    });

    let capturedVerified = false;
    await suite.check("service: a valid receipt unlocks the resource and settles over the wire", async () => {
      const offerResponse = await fetch(base, { method: "GET", headers: { accept: "application/json" } });
      const header = offerResponse.headers.get("PAYMENT-REQUIRED") ?? "";
      await offerResponse.text();
      const parsed = buyer.parseOffer(header);
      const payload = buyerPayload({ requirements: parsed.accepted, paymentRequired: parsed.required }, receipt, "service-happy");
      const paid = await fetch(base, {
        method: "GET",
        headers: { accept: "application/json", "PAYMENT-SIGNATURE": encodePaymentPayloadHeader(payload) },
      });
      assert(paid.status === 200, `expected the resource to be released, received ${paid.status}`);
      const body = await paid.text();
      assert(body === resourceBody, "the released body must match the protected resource");
      const settlementHeader = paid.headers.get("PAYMENT-RESPONSE");
      assert(settlementHeader !== null, "a released resource must carry a PAYMENT-RESPONSE settlement");
      const prepared = await preparedPayment(parsed, receipt, resolver);
      const captured = await buyer.captureSettlement(settlementHeader ?? "", prepared);
      capturedVerified = captured.verification.level === "sequencer-signed";
      assert(capturedVerified, "the buyer must verify the settlement receipt");
    });

    await suite.check("service: a tampered payment cannot unlock the resource", async () => {
      const offerResponse = await fetch(base, { method: "GET", headers: { accept: "application/json" } });
      const header = offerResponse.headers.get("PAYMENT-REQUIRED") ?? "";
      await offerResponse.text();
      const parsed = buyer.parseOffer(header);
      const tamperedBytes = receipt.canonicalReceipt.slice();
      tamperedBytes[tamperedBytes.length - 1] ^= 0x01;
      const tampered = { ...receipt, evidence: { ...receipt.evidence, receipt: encodeBase64(tamperedBytes) } };
      const payload = buyerPayload({ requirements: parsed.accepted, paymentRequired: parsed.required }, tampered, "service-tamper");
      const response = await fetch(base, {
        method: "GET",
        headers: { accept: "application/json", "PAYMENT-SIGNATURE": encodePaymentPayloadHeader(payload) },
      });
      await response.text();
      assert(response.status !== 200, "a tampered payment must never release the resource");
    });

    await suite.check("service: replaying the same payment is idempotent", async () => {
      const offerResponse = await fetch(base, { method: "GET", headers: { accept: "application/json" } });
      const header = offerResponse.headers.get("PAYMENT-REQUIRED") ?? "";
      await offerResponse.text();
      const parsed = buyer.parseOffer(header);
      const payload = buyerPayload({ requirements: parsed.accepted, paymentRequired: parsed.required }, receipt, "service-happy");
      const encoded = encodePaymentPayloadHeader(payload);
      const first = await fetch(base, { method: "GET", headers: { accept: "application/json", "PAYMENT-SIGNATURE": encoded } });
      const second = await fetch(base, { method: "GET", headers: { accept: "application/json", "PAYMENT-SIGNATURE": encoded } });
      const firstBody = await first.text();
      const secondBody = await second.text();
      assert(first.status === 200 && second.status === 200, "both replays must release the resource");
      assert(firstBody === secondBody, "a replayed payment must return the same resource");
      assert(capturedVerified, "the earlier settlement capture must have verified");
    });
  } finally {
    if (child !== undefined) {
      child.kill("SIGKILL");
    }
    await rm(workDir, { recursive: true, force: true });
  }
}

async function preparedPayment(parsed, receipt, resolver) {
  const batch = await resolver.resolve();
  const verification = await verifyPaymentReceipt(
    { canonicalReceipt: receipt.canonicalReceipt, authorizedBatch: batch },
    parsed.accepted,
  );
  const payload = buyerPayload({ requirements: parsed.accepted, paymentRequired: parsed.required }, receipt, "service-happy");
  return {
    offer: parsed,
    quote: {
      quote_id: "quote-service",
      description_copy_key: "move.description",
      mechanism: "transfer",
      money: { amount: parsed.accepted.amount, currency: parsed.accepted.asset },
      fee_estimate: { amount: "0", currency: parsed.accepted.asset },
      fee_ceiling: { amount: "0", currency: parsed.accepted.asset },
      arrival_estimate: "2026-01-01T00:00:00.000Z",
      expires_at: "2999-01-01T00:00:00.000Z",
    },
    journey: {
      journey_id: "journey-service",
      kind: "move",
      state: "done",
      state_copy_key: "journey.done",
      evidence: [],
      stages: [],
      started_at: "2026-01-01T00:00:00.000Z",
      updated_at: "2026-01-01T00:00:00.000Z",
    },
    payload,
    paymentHeader: encodePaymentPayloadHeader(payload),
    idempotencyKey: "service-happy",
    canonicalReceipt: receipt.canonicalReceipt,
    authorizedBatch: batch,
    verification,
    receiptDigest: receipt.receiptDigest,
  };
}

function waitForListening(child, timeoutMs) {
  return new Promise((resolve, reject) => {
    let settled = false;
    let buffer = "";
    const timer = setTimeout(() => {
      if (!settled) {
        settled = true;
        reject(new Error("paid-api example did not report a listening port in time"));
      }
    }, timeoutMs);
    const finish = (error) => {
      if (settled) {
        return;
      }
      settled = true;
      clearTimeout(timer);
      if (error === undefined) {
        resolve();
      } else {
        reject(error);
      }
    };
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (chunk) => {
      buffer += chunk;
      if (buffer.includes("\"listening\"")) {
        finish();
      }
    });
    child.stderr.setEncoding("utf8");
    child.stderr.on("data", () => undefined);
    child.on("exit", (code) => finish(new Error(`paid-api example exited early with code ${code ?? "unknown"}`)));
    child.on("error", (error) => finish(error instanceof Error ? error : new Error(String(error))));
  });
}
