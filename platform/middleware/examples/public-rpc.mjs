import { readFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { parseArgs } from "node:util";
import { PaymentRpc, decodePaymentRequiredHeader, verifyRpcPayment, rpcHex } from "../seller/dist/index.js";

const { values } = parseArgs({ options: {
  help: { type: "boolean" }, rpc: { type: "string" }, did: { type: "string" },
  faucet: { type: "string" }, "public-key": { type: "string" }, "claim-key": { type: "string" },
  activity: { type: "string" }, offer: { type: "string" }, authority: { type: "string" }, payer: { type: "string" },
} });
values.rpc ??= process.env.LAYERX_RPC_URL;
values.faucet ??= process.env.LAYERX_FAUCET_URL;
values.did ??= process.env.LAYERX_DID;
if (values.help) {
  process.stdout.write("Usage: node public-rpc.mjs --rpc https://HOST/rpc --did DID [--faucet https://HOST/v1/faucet/claims --public-key HEX32 --claim-key KEY] [--activity SIGNED_HEX_FILE --offer PAYMENT_REQUIRED_HEADER_FILE --authority TRUSTED_AUTHORITY_JSON --payer ACCOUNT_HEX32]\nURLs may come from LAYERX_RPC_URL and LAYERX_FAUCET_URL; DID from LAYERX_DID. Use LAYERX_RPC_TOKEN for submission and LAYERX_FAUCET_TOKEN for faucet authentication. The payment example requires executed commitment.\n");
} else {
  if (!values.rpc || !values.did) throw new Error("rpc-and-did-required");
  if (new URL(values.rpc).port === "18545" || (values.faucet && new URL(values.faucet).port === "18545")) throw new Error("persistent-host-chain-forbidden");
  const rpc = new PaymentRpc(values.rpc, process.env.LAYERX_RPC_TOKEN ? { authorization: `Bearer ${process.env.LAYERX_RPC_TOKEN}` } : {});
  if (values.faucet) {
    const url = new URL(values.faucet);
    if (url.username || url.password || url.search || url.hash || url.pathname !== "/v1/faucet/claims"
      || (url.protocol !== "https:" && !(url.protocol === "http:" && ["localhost", "127.0.0.1", "[::1]"].includes(url.hostname)))) throw new Error("invalid-faucet-url");
    rpcHex(values["public-key"], 32);
    if (!values["claim-key"] || !process.env.LAYERX_FAUCET_TOKEN) throw new Error("faucet-claim-key-and-token-required");
    const response = await fetch(url, { method: "POST", redirect: "error", signal: AbortSignal.timeout(30_000),
      headers: { "content-type": "application/json", authorization: `Bearer ${process.env.LAYERX_FAUCET_TOKEN}`, "idempotency-key": values["claim-key"] },
      body: JSON.stringify({ did: values.did, public_key: values["public-key"] }) });
    await response.body?.cancel();
    process.stdout.write(`Faucet HTTP status: ${response.status}; confirm the account balance before spending.\n`);
    if (!response.ok) process.exitCode = 1;
  }
  const sequence = await rpc.call("lx_getSequence", [values.did]);
  process.stdout.write(JSON.stringify({ sequence }) + "\n");
  if (values.activity) {
    if (!values.offer || !values.authority || !values.payer) throw new Error("offer-authority-and-payer-required");
    const required = decodePaymentRequiredHeader((await readFile(values.offer, "utf8")).trim());
    const offer = required.accepts.find(v => ["exact", "metered", "subscription"].includes(v.scheme) && (v.extra?.layerx?.commitment ?? "executed") === "executed");
    if (!offer) throw new Error("executed-offer-required");
    const canonical = (await readFile(values.activity, "utf8")).trim();
    const activityId = createHash("sha256").update("LXP/v1/activity-id\0").update(rpcHex(canonical)).digest("hex");
    const configured = JSON.parse(await readFile(values.authority, "utf8"));
    const authority = Object.fromEntries(["batchId", "asset", "previousStateRoot", "resultingStateRoot", "sequencerPublicKey"].map(key => [key, rpcHex(configured[key], 32)]));
    const result = await rpc.send(canonical, "executed");
    const payment = await verifyRpcPayment(result, activityId, values.payer, offer, authority);
    if (payment.kind === "pending") {
      process.stdout.write(JSON.stringify({ state: "pending", activity_id: activityId }) + "\n");
      process.exitCode = 2;
    } else {
      if (offer.scheme !== "exact" && (payment.verification.receipt.moduleId !== 1 || payment.verification.receipt.operation !== 6)) throw new Error("grant-receive-receipt-required");
      const digest = createHash("sha256").update("LXP/v1/merkle-leaf\0").update(payment.canonicalReceipt).digest("hex");
      process.stdout.write(JSON.stringify({ transaction: `lxp:${digest}`, commitment: "executed" }) + "\n");
    }
  }
}
