import { spawn } from "node:child_process";
import { createHash, generateKeyPairSync, randomBytes } from "node:crypto";
import { access, constants, mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { BuyerMiddleware, LayerXPaymentHttpTransport } from "@sidiora/layerx-buyer-middleware";
import { ProductionClient, SecretBytes } from "@sidiora/layerx-sdk";
import {
  ReceiptAuthorityClient,
  applyEndpointOverride,
  exactObject,
  optionalEnvironment,
  requiredEnvironment,
  secureBaseUrl,
} from "./support/runtime.mjs";

const root = resolve(import.meta.dirname, "../..");
const manifest = exactObject(JSON.parse(await readFile(resolve(import.meta.dirname, "reference-apps.json"), "utf8")));
const expectedApplications = ["buyer-agent", "paid-api", "merchant-shop", "marketplace"];
if (manifest.version !== 1 || !Array.isArray(manifest.applications) || manifest.applications.length !== 4) {
  throw new Error("invalid_reference_application_manifest");
}

const EMULATOR_PROTOCOL_VERSION = "3";
const EMULATOR_PREFUND = "1000000000";
const EMULATOR_SEED_MOVE = "100000";
const EMULATOR_DEFAULT_PRICE = "1000";
const EMULATOR_READY_TIMEOUT_MS = 30_000;
const EMULATOR_READY_POLL_MS = 100;
const EMULATOR_KEY_NAMES = Object.freeze(["reference-buyer", "reference-seller", "reference-merchant", "reference-marketplace"]);

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
  if (environment !== "emulator") {
    await runApplications(environment, {});
    return;
  }
  const emulator = await startEmulator();
  try {
    const inputs = await deriveEmulatorInputs(emulator);
    process.stdout.write(`${JSON.stringify({
      environment: "emulator",
      state: "provisioned",
      endpoint: emulator.endpoint,
      derived: Object.keys(inputs).sort(),
    })}\n`);
    await runApplications("emulator", inputs);
  } finally {
    await stopEmulator(emulator);
  }
}

async function runApplications(environment, inputs) {
  const paid = applicationEntry("paid-api");
  const merchant = applicationEntry("merchant-shop");
  const buyer = applicationEntry("buyer-agent");
  const marketplace = applicationEntry("marketplace");
  const services = [];
  try {
    services.push(await startService(paid.commands[environment], inputs));
    services.push(await startService(merchant.commands[environment], inputs));
    await command(buyer.commands[environment], inputs);
    await merchantCheckout(environment, inputs);
    if (environment === "emulator") {
      await command(marketplace.commands[environment], inputs);
      await command(["npm", "run", "list:emulator", "--workspace", marketplace.package], inputs);
      await command(["npm", "run", "buy:emulator", "--workspace", marketplace.package], inputs);
    } else {
      await command(marketplace.commands[environment], inputs);
    }
    process.stdout.write(`${JSON.stringify({ environment, state: "completed", applications: manifest.applications.map((value) => value.name) })}\n`);
  } finally {
    for (const service of services.reverse()) stopService(service);
  }
}

function applicationEntry(name) {
  const entry = manifest.applications.find((value) => value.name === name);
  if (entry === undefined) throw new Error(`missing_reference_application_${name}`);
  return entry;
}

async function resolveCliBinary() {
  const explicit = optionalEnvironment("LAYERX_BIN");
  if (explicit !== undefined) return explicit;
  const local = resolve(root, "build/bin/layerx");
  try {
    await access(local, constants.X_OK);
    return local;
  } catch {
    return "layerx";
  }
}

async function startEmulator() {
  const buyerConfig = await environmentConfig("buyer-agent", "emulator");
  const endpoint = secureBaseUrl(buyerConfig.humanUrl);
  if (endpoint.hostname !== "127.0.0.1" && endpoint.hostname !== "localhost") {
    throw new Error("emulator_endpoint_must_be_loopback");
  }
  const listen = `127.0.0.1:${endpoint.port === "" ? "80" : endpoint.port}`;
  const cli = await resolveCliBinary();
  const profile = await mkdtemp(resolve(tmpdir(), "layerx-reference-apps-"));
  const cliEnvironment = {
    LAYERX_BIN: cli,
    LAYERX_CONFIG: resolve(profile, "config.json"),
    LAYERX_CREDENTIAL_STORE: optionalEnvironment("LAYERX_CREDENTIAL_STORE") ?? "file",
    LAYERX_CREDENTIAL_PASSPHRASE: optionalEnvironment("LAYERX_CREDENTIAL_PASSPHRASE") ?? randomBytes(24).toString("hex"),
  };
  const suppliedSeed = optionalEnvironment("LAYERX_EMULATOR_SEED_FILE");
  let seedFile;
  let anchor;
  if (suppliedSeed === undefined) {
    const provisioned = await runCli(cli, cliEnvironment, ["emulator", "provision"]);
    seedFile = requiredText(provisioned.sequencer_seed_file, "emulator_provision_omitted_seed_file");
    anchor = requiredHex32(provisioned.sequencer_trust_anchor, "emulator_provision_omitted_trust_anchor");
  } else {
    seedFile = suppliedSeed;
    anchor = requiredHex32(
      (await readFile(resolve(dirname(seedFile), "sequencer.anchor"), "utf8")).trim(),
      "emulator_seed_file_has_no_published_trust_anchor",
    );
  }
  const keys = {};
  for (const name of EMULATOR_KEY_NAMES) {
    const created = await runCli(cli, cliEnvironment, ["key", "create", name]);
    keys[name] = {
      did: requiredText(created.did, "layerx_key_create_omitted_did"),
      publicKey: requiredHex32(created.public_key, "layerx_key_create_omitted_public_key"),
    };
  }
  await runCli(cli, cliEnvironment, ["key", "default", "reference-marketplace"]);
  const prefunds = [
    `${keys["reference-buyer"].did},${anchor},${EMULATOR_PREFUND}`,
    `${keys["reference-seller"].did},${anchor},0`,
    `${keys["reference-merchant"].did},${anchor},0`,
    `${keys["reference-marketplace"].did},${keys["reference-marketplace"].publicKey},${EMULATOR_PREFUND}`,
  ];
  const child = spawn(cli, [
    "--json",
    "emulator",
    "up",
    "--sequencer-seed-file",
    seedFile,
    "--listen",
    listen,
    "--network-id",
    "402",
    "--protocol-version",
    EMULATOR_PROTOCOL_VERSION,
    "--time-ms",
    Date.now().toString(),
    ...prefunds.flatMap((value) => ["--prefund", value]),
  ], {
    cwd: root,
    env: { ...process.env, ...cliEnvironment },
    stdio: ["ignore", "inherit", "inherit"],
    detached: true,
  });
  const emulator = { child, cli, cliEnvironment, endpoint, keys, profile, anchor, owned: suppliedSeed === undefined ? profile : undefined };
  let exited;
  child.once("exit", (code, signal) => { exited = `${signal ?? code ?? "failed"}`; });
  const deadline = Date.now() + EMULATOR_READY_TIMEOUT_MS;
  for (;;) {
    if (exited !== undefined) throw new Error(`layerx_emulator_up_exited_${exited}`);
    if (await emulatorReady(endpoint)) return emulator;
    if (Date.now() >= deadline) {
      stopService(child);
      throw new Error("layerx_emulator_start_timeout");
    }
    await new Promise((resolvePromise) => setTimeout(resolvePromise, EMULATOR_READY_POLL_MS));
  }
}

async function emulatorReady(endpoint) {
  let response;
  try {
    response = await fetch(new URL("healthz", endpoint), { headers: { accept: "application/json" } });
  } catch {
    return false;
  }
  if (!response.ok) {
    await response.body?.cancel();
    return false;
  }
  const body = exactObject(await response.json());
  return exactObject(body.result ?? body).status === "ready";
}

async function stopEmulator(emulator) {
  stopService(emulator.child);
  await rm(emulator.profile, { recursive: true, force: true });
}

async function deriveEmulatorInputs(emulator) {
  const token = optionalEnvironment("LAYERX_EMULATOR_TOKEN") ?? randomBytes(32).toString("hex");
  const advertised = await readEmulatorJson(emulator.endpoint, "v1/sequencer", token);
  requiredHex32(advertised.sequencer_public_key, "emulator_omitted_sequencer_public_key");
  await runCli(emulator.cli, emulator.cliEnvironment, [
    "environment",
    "use",
    "emulator",
    "--endpoint",
    emulator.endpoint.toString().replace(/\/$/u, ""),
    "--network-id",
    String(advertised.network_id),
    "--sequencer-trust-anchor",
    emulator.anchor,
  ]);
  const source = accountName(emulator.keys["reference-buyer"].did);
  const sellerAccount = accountName(emulator.keys["reference-seller"].did);
  const merchantAccount = accountName(emulator.keys["reference-merchant"].did);
  const seeded = await seedEmulatorTransfer(emulator.endpoint, token, source, sellerAccount);
  const accounts = await readEmulatorAccounts(emulator.endpoint, token);
  const webhook = emulatorWebhookKey();
  const price = optionalEnvironment("LAYERX_EMULATOR_PRICE") ?? EMULATOR_DEFAULT_PRICE;
  const derived = {
    LAYERX_BIN: emulator.cliEnvironment.LAYERX_BIN,
    LAYERX_CONFIG: emulator.cliEnvironment.LAYERX_CONFIG,
    LAYERX_CREDENTIAL_STORE: emulator.cliEnvironment.LAYERX_CREDENTIAL_STORE,
    LAYERX_CREDENTIAL_PASSPHRASE: emulator.cliEnvironment.LAYERX_CREDENTIAL_PASSPHRASE,
    LAYERX_EMULATOR_TOKEN: token,
    LAYERX_EMULATOR_ASSET: seeded.asset,
    LAYERX_EMULATOR_SOURCE: source,
    LAYERX_EMULATOR_SELLER: accountIdentifier(accounts, sellerAccount),
    LAYERX_EMULATOR_SELLER_ACCOUNT: sellerAccount,
    LAYERX_EMULATOR_MERCHANT: accountIdentifier(accounts, merchantAccount),
    LAYERX_EMULATOR_MERCHANT_ACCOUNT: merchantAccount,
    LAYERX_EMULATOR_CURRENCY: seeded.currency,
    LAYERX_EMULATOR_PRICE: price,
    LAYERX_EMULATOR_PAYMENT_KEY: randomBytes(16).toString("hex"),
    LAYERX_EMULATOR_MERCHANT_CHECKOUT_KEY: randomBytes(16).toString("hex"),
    LAYERX_EMULATOR_WEBHOOK_PUBLIC_KEYS_JSON: webhook,
    LAYERX_EMULATOR_MARKETPLACE_KEY: randomBytes(16).toString("hex"),
    LAYERX_EMULATOR_MARKETPLACE_PROGRAM_ID: randomBytes(32).toString("hex"),
    LAYERX_EMULATOR_MARKETPLACE_LISTING_ID: randomBytes(32).toString("hex"),
    LAYERX_EMULATOR_MARKETPLACE_RECEIPT_DIGEST: seeded.receiptDigest,
  };
  for (const name of Object.keys(derived)) {
    const supplied = optionalEnvironment(name);
    if (supplied !== undefined) derived[name] = supplied;
  }
  return derived;
}

function accountName(did) {
  if (!/^did:[a-z0-9]+:[A-Za-z0-9._-]{1,128}$/u.test(did)) throw new Error("invalid_emulator_did");
  return `agent:${did}:main`;
}

function accountIdentifier(accounts, name) {
  const account = accounts.find((entry) => exactObject(entry).name === name);
  if (account === undefined) throw new Error(`emulator_state_omitted_account_${name}`);
  return requiredHex32(account.id, "emulator_state_omitted_account_identifier");
}

function emulatorWebhookKey() {
  const { publicKey } = generateKeyPairSync("ed25519");
  const raw = publicKey.export({ format: "der", type: "spki" }).subarray(-32).toString("hex");
  return JSON.stringify({ [`emulator-${randomBytes(4).toString("hex")}`]: requiredHex32(raw, "invalid_webhook_public_key") });
}

async function seedEmulatorTransfer(endpoint, token, source, destination) {
  const quote = await postEmulatorJson(endpoint, "v1/moves/quote", token, undefined, {
    source,
    destination,
    money: { amount: EMULATOR_SEED_MOVE, currency: "LXP" },
  });
  const journey = await postEmulatorJson(endpoint, "v1/moves", token, randomBytes(16).toString("hex"), {
    quote_id: requiredText(quote.quote_id, "emulator_move_quote_omitted_identifier"),
  });
  if (journey.state !== "done" && journey.state !== "done-finalised") {
    throw new Error(`emulator_seed_move_${String(journey.state)}`);
  }
  const evidence = (Array.isArray(journey.evidence) ? journey.evidence : []).find(
    (entry) => exactObject(entry).class === "layerx-receipt",
  );
  if (evidence === undefined) throw new Error("emulator_seed_move_omitted_receipt_evidence");
  const reference = requiredText(evidence.source_ref, "emulator_seed_move_omitted_receipt_reference");
  const receiptDigest = requiredHex32(
    requiredText(evidence.evidence_id, "emulator_seed_move_omitted_evidence_id").replace(/^evd_/u, ""),
    "emulator_seed_move_omitted_receipt_digest",
  );
  const receipt = await readEmulatorJson(endpoint, reference.replace(/^\//u, ""), token);
  const authority = exactObject(receipt.authority);
  return {
    asset: requiredHex32(authority.asset, "emulator_receipt_omitted_asset"),
    currency: requiredText(exactObject(quote.money).currency, "emulator_move_quote_omitted_currency"),
    receiptDigest,
  };
}

async function readEmulatorAccounts(endpoint, token) {
  const state = await readEmulatorJson(endpoint, "v1/state", token);
  if (!Array.isArray(state.accounts)) throw new Error("emulator_state_omitted_accounts");
  return state.accounts;
}

async function readEmulatorJson(endpoint, path, token) {
  const response = await fetch(new URL(path, endpoint), {
    headers: { accept: "application/json", authorization: `Bearer ${token}` },
  });
  return emulatorResult(response, path);
}

async function postEmulatorJson(endpoint, path, token, idempotencyKey, body) {
  const headers = new Headers({
    accept: "application/json",
    "content-type": "application/json",
    authorization: `Bearer ${token}`,
  });
  if (idempotencyKey !== undefined) headers.set("idempotency-key", idempotencyKey);
  const response = await fetch(new URL(path, endpoint), { method: "POST", headers, body: JSON.stringify(body) });
  return emulatorResult(response, path);
}

async function emulatorResult(response, path) {
  const body = await response.json().catch(() => undefined);
  if (!response.ok) {
    throw new Error(`emulator_${path.replace(/[^a-z0-9]+/gu, "_")}_http_${response.status}_${JSON.stringify(body ?? null)}`);
  }
  const envelope = exactObject(body);
  return exactObject(envelope.result ?? envelope);
}

function requiredText(value, failure) {
  if (typeof value !== "string" || value.length === 0) throw new Error(failure);
  return value;
}

function requiredHex32(value, failure) {
  const digits = typeof value === "string" && value.startsWith("0x") ? value.slice(2) : value;
  if (typeof digits !== "string" || !/^[0-9a-f]{64}$/u.test(digits)) throw new Error(failure);
  return digits;
}

function runCli(cli, cliEnvironment, commandLine) {
  return new Promise((resolvePromise, reject) => {
    const child = spawn(cli, ["--json", ...commandLine], {
      cwd: root,
      env: { ...process.env, ...cliEnvironment },
      stdio: ["ignore", "pipe", "pipe"],
    });
    const stdout = [];
    const stderr = [];
    child.stdout.on("data", (chunk) => stdout.push(chunk));
    child.stderr.on("data", (chunk) => stderr.push(chunk));
    child.once("error", () => reject(new Error(`layerx_cli_unavailable_${commandLine[0]}_set_LAYERX_BIN`)));
    child.once("close", (code, signal) => {
      const output = Buffer.concat(code === 0 ? stdout : stderr).toString("utf8").trim();
      let value;
      try {
        value = exactObject(JSON.parse(output));
      } catch {
        reject(new Error(`layerx_cli_${commandLine.join("_")}_${signal ?? code ?? "failed"}_${output}`));
        return;
      }
      if (code !== 0 || value.ok !== true) {
        reject(new Error(`layerx_cli_${commandLine.join("_")}_${String(value.error?.detail ?? code)}`));
        return;
      }
      resolvePromise(exactObject(value.data));
    });
  });
}

async function merchantCheckout(environment, inputs) {
  const buyerConfig = await environmentConfig("buyer-agent", environment);
  const merchantConfig = await environmentConfig("merchant-shop", environment);
  const rawToken = scenarioEnvironment(inputs, buyerConfig.tokenEnvironment);
  const token = new SecretBytes(new TextEncoder().encode(rawToken));
  try {
    const buyer = new BuyerMiddleware({
      client: new ProductionClient(new LayerXPaymentHttpTransport({ baseUrl: buyerConfig.humanUrl, bearerToken: token })),
      source: scenarioEnvironment(inputs, buyerConfig.sourceEnvironment),
      supported: [{ scheme: buyerConfig.scheme, network: buyerConfig.network }],
      authorizedBatches: new ReceiptAuthorityClient(buyerConfig.receiptAuthorityUrl, rawToken),
    });
    const checkoutKey = scenarioEnvironment(inputs, environment === "emulator"
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

function scenarioEnvironment(inputs, name) {
  if (!/^[A-Z][A-Z0-9_]{0,127}$/u.test(name)) throw new Error("invalid_environment_binding");
  const value = inputs[name] ?? process.env[name];
  if (value === undefined || value.length === 0) throw new Error(`missing_${name.toLowerCase()}`);
  return value;
}

async function environmentConfig(application, environment) {
  const entry = manifest.applications.find((value) => value.name === application);
  const value = exactObject(JSON.parse(await readFile(resolve(root, entry.path, entry.config), "utf8")));
  return applyEndpointOverride(environment, exactObject(value.environments[environment]));
}

function startService(commandLine, inputs) {
  return new Promise((resolvePromise, reject) => {
    const child = spawn(commandLine[0], commandLine.slice(1), {
      cwd: root,
      env: { ...process.env, ...inputs },
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

function command(commandLine, inputs) {
  return new Promise((resolvePromise, reject) => {
    const child = spawn(commandLine[0], commandLine.slice(1), {
      cwd: root,
      env: { ...process.env, ...inputs },
      stdio: "inherit",
    });
    child.once("error", reject);
    child.once("exit", (code) => code === 0 ? resolvePromise() : reject(new Error(`reference_command_exited_${code}`)));
  });
}
