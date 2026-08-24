import { spawn } from "node:child_process";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { BuyerMiddleware, LayerXPaymentHttpTransport } from "@sidiora/layerx-buyer-middleware";
import { ProductionClient, SecretBytes } from "@sidiora/layerx-sdk";
import { ReceiptAuthorityClient, exactObject, requiredEnvironment } from "./support/runtime.mjs";

const root = resolve(import.meta.dirname, "../..");
const manifest = exactObject(JSON.parse(await readFile(resolve(import.meta.dirname, "reference-apps.json"), "utf8")));
const expectedApplications = ["buyer-agent", "paid-api", "merchant-shop", "marketplace"];
if (manifest.version !== 1 || !Array.isArray(manifest.applications) || manifest.applications.length !== 4) {
  throw new Error("invalid_reference_application_manifest");
}

const arguments_ = process.argv.slice(2);
if (arguments_.length === 1 && arguments_[0] === "--check") {
  await checkManifest();
} else if (arguments_.length === 2 && arguments_[0] === "--scenario" && ["emulator", "testnet"].includes(arguments_[1])) {
  await runScenario(arguments_[1]);
} else {
  throw new Error("usage_--check_or_--scenario_emulator_or_testnet");
}

async function checkManifest() {
  const names = new Set();
  for (const application of manifest.applications) {
    if (names.has(application.name)) throw new Error("duplicate_reference_application");
    names.add(application.name);
    const directory = resolve(root, application.path);
    const packageDocument = exactObject(JSON.parse(await readFile(resolve(directory, "package.json"), "utf8")));
    const configDocument = exactObject(JSON.parse(await readFile(resolve(directory, application.config), "utf8")));
    if (packageDocument.name !== application.package || configDocument.application !== application.name) {
      throw new Error(`reference_application_identity_mismatch_${application.name}`);
    }
    if (application.compatibilityPackage !== undefined) {
      const compatibility = exactObject(JSON.parse(await readFile(resolve(root, "platform/examples/merchant-checkout/package.json"), "utf8")));
      if (compatibility.name !== application.compatibilityPackage) throw new Error("invalid_merchant_compatibility_package");
    }
    for (const environment of ["emulator", "testnet"]) {
      const command = application.commands[environment];
      if (!Array.isArray(command) || command.length < 2 || command.some((part) => typeof part !== "string" || part.length === 0)) {
        throw new Error(`invalid_reference_command_${application.name}_${environment}`);
      }
      const expected = ["npm", "run", `start:${environment}`, "--workspace", application.package];
      if (JSON.stringify(command) !== JSON.stringify(expected)) {
        throw new Error(`reference_command_drift_${application.name}_${environment}`);
      }
      exactObject(configDocument.environments[environment]);
      if (packageDocument.scripts[`start:${environment}`] === undefined) {
        throw new Error(`missing_reference_script_${application.name}_${environment}`);
      }
    }
  }
  if (JSON.stringify([...names]) !== JSON.stringify(expectedApplications)) {
    throw new Error("reference_application_manifest_drift");
  }
  const sourceFiles = [
    "platform/examples/buyer-agent/index.mjs",
    "platform/examples/paid-api/index.mjs",
    "platform/examples/support/merchant-app.mjs",
    "platform/examples/marketplace/index.mjs",
    "platform/examples/marketplace/program/src/lib.rs",
  ];
  const sources = (await Promise.all(sourceFiles.map((path) => readFile(resolve(root, path), "utf8")))).join("\n");
  for (const application of manifest.applications) {
    if (!sources.includes(application.symbol)) throw new Error(`missing_reference_symbol_${application.symbol}`);
  }
  if (sources.includes("LAYERX_AUTHORIZED_BATCH_JSON") || /NEXT_PUBLIC_|window\.localStorage/u.test(sources)) {
    throw new Error("reference_application_contains_fixture_or_browser_secret_surface");
  }
  process.stdout.write(`${JSON.stringify({ checked: [...names], environments: ["emulator", "testnet"] })}\n`);
}

async function runScenario(environment) {
  await checkManifest();
  const paid = manifest.applications.find((value) => value.name === "paid-api");
  const merchant = manifest.applications.find((value) => value.name === "merchant-shop");
  const buyer = manifest.applications.find((value) => value.name === "buyer-agent");
  const marketplace = manifest.applications.find((value) => value.name === "marketplace");
  const services = [];
  try {
    services.push(await startService(paid.commands[environment]));
    services.push(await startService(merchant.commands[environment]));
    await command(buyer.commands[environment]);
    await merchantCheckout(environment);
    await command(marketplace.commands[environment]);
    process.stdout.write(`${JSON.stringify({ environment, state: "completed", applications: manifest.applications.map((value) => value.name) })}\n`);
  } finally {
    for (const service of services.reverse()) stopService(service);
  }
}

async function merchantCheckout(environment) {
  const buyerConfig = await environmentConfig("buyer-agent", environment);
  const merchantConfig = await environmentConfig("merchant-shop", environment);
  const rawToken = requiredEnvironment(buyerConfig.tokenEnvironment);
  const token = new SecretBytes(new TextEncoder().encode(rawToken));
  try {
    const buyer = new BuyerMiddleware({
      client: new ProductionClient(new LayerXPaymentHttpTransport({ baseUrl: buyerConfig.humanUrl, bearerToken: token })),
      source: requiredEnvironment(buyerConfig.sourceEnvironment),
      supported: [{ scheme: buyerConfig.scheme, network: buyerConfig.network }],
      authorizedBatches: new ReceiptAuthorityClient(buyerConfig.receiptAuthorityUrl, rawToken),
    });
    const checkoutKey = requiredEnvironment(environment === "emulator"
      ? "LAYERX_EMULATOR_MERCHANT_CHECKOUT_KEY"
      : "LAYERX_TESTNET_MERCHANT_CHECKOUT_KEY");
    const body = JSON.stringify({
      principal: "reference-buyer",
      checkout_key: checkoutKey,
      lines: [{ sku: "metered-report", quantity: 1 }],
    });
    const result = await buyer.fetch(
      `http://127.0.0.1:${merchantConfig.port}/checkout`,
      { method: "POST", headers: { "content-type": "application/json" }, body },
      `${checkoutKey}-payment`,
    );
    if (result.kind !== "paid" || !result.response.ok) throw new Error(`merchant_reference_${result.kind}`);
    await result.response.body?.cancel();
  } finally {
    token.destroy();
  }
}

async function environmentConfig(application, environment) {
  const entry = manifest.applications.find((value) => value.name === application);
  const value = exactObject(JSON.parse(await readFile(resolve(root, entry.path, entry.config), "utf8")));
  return exactObject(value.environments[environment]);
}

function startService(commandLine) {
  return new Promise((resolvePromise, reject) => {
    const child = spawn(commandLine[0], commandLine.slice(1), {
      cwd: root,
      env: process.env,
      stdio: ["ignore", "pipe", "inherit"],
      detached: true,
    });
    let started = false;
    let output = "";
    const timeout = setTimeout(() => {
      stopService(child);
      reject(new Error("reference_service_start_timeout"));
    }, 30_000);
    child.once("error", (error) => {
      clearTimeout(timeout);
      reject(error);
    });
    child.once("exit", (code) => {
      clearTimeout(timeout);
      if (!started) reject(new Error(`reference_service_exited_${code}`));
    });
    child.stdout.on("data", (chunk) => {
      process.stdout.write(chunk);
      output += chunk.toString("utf8");
      if (!started && output.includes("\"listening\"")) {
        started = true;
        clearTimeout(timeout);
        resolvePromise(child);
      }
    });
  });
}

function stopService(child) {
  if (child.pid === undefined) return;
  try {
    process.kill(-child.pid, "SIGTERM");
  } catch (error) {
    if (error.code !== "ESRCH") throw error;
  }
}

function command(commandLine) {
  return new Promise((resolvePromise, reject) => {
    const child = spawn(commandLine[0], commandLine.slice(1), { cwd: root, env: process.env, stdio: "inherit" });
    child.once("error", reject);
    child.once("exit", (code) => code === 0 ? resolvePromise() : reject(new Error(`reference_command_exited_${code}`)));
  });
}
