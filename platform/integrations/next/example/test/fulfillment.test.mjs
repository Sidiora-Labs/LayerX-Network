import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, mkdir, readFile, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { FileFulfillmentRepository } from "../lib/fulfillment.mjs";

const fixture = JSON.parse(await readFile(new URL("../../../../sdk/conformance/fixtures/receipt-positive-v2.json", import.meta.url), "utf8"));
const decode = value => Uint8Array.from(Buffer.from(value, "hex"));
const proposed = {
  idempotencyKey: "ab".repeat(32), requestDigest: "cd".repeat(32),
  canonicalReceipt: decode(fixture.canonical_receipt_hex),
  authorizedBatch: {
    batchId: decode(fixture.authorized_batch.batch_id_hex),
    asset: decode(fixture.authorized_batch.asset_hex),
    previousStateRoot: decode(fixture.authorized_batch.previous_state_root_hex),
    resultingStateRoot: decode(fixture.authorized_batch.resulting_state_root_hex),
    sequencerPublicKey: decode(fixture.authorized_batch.sequencer_public_key_hex),
  },
};

test("concurrent fulfillment and reopen release one durable resource", async () => {
  const root = await mkdtemp(join(tmpdir(), "next-fulfillment-"));
  try {
    const source = join(root, "resource.txt");
    await writeFile(source, "paid resource");
    let releases = 0;
    const release = async () => {
      releases += 1;
      return { contentType: "text/plain", body: await readFile(source, "utf8") };
    };
    const repository = new FileFulfillmentRepository(join(root, "state"));
    const results = await Promise.all(Array.from({ length: 16 }, () => repository.fulfill(proposed, release)));
    assert.equal(releases, 1);
    assert(results.every(value => value.resource.body === "paid resource"));
    const reopened = new FileFulfillmentRepository(join(root, "state"));
    assert.equal((await reopened.fulfill(proposed, release)).resource.body, "paid resource");
    assert.equal(releases, 1);
    await assert.rejects(reopened.fulfill({ ...proposed, requestDigest: "ef".repeat(32) }, release));
    assert.equal(releases, 1);
  } finally {
    await rm(root, { recursive: true });
  }
});


test("legacy committed fulfillment migrates without another release", async () => {
  const root = await mkdtemp(join(tmpdir(), "next-fulfillment-legacy-"));
  try {
    const directory = join(root, "state");
    await mkdir(directory, { mode: 0o700 });
    const resource = { contentType: "text/plain", body: "previously released" };
    await writeFile(join(directory, `${proposed.idempotencyKey}.json`), JSON.stringify({
      requestDigest: proposed.requestDigest,
      receipt: Buffer.from(proposed.canonicalReceipt).toString("base64"),
      resource,
    }), { mode: 0o600 });
    const repository = new FileFulfillmentRepository(directory);
    let releases = 0;
    const release = async () => { releases += 1; return resource; };
    assert.deepEqual((await repository.fulfill(proposed, release)).resource, resource);
    assert.deepEqual((await new FileFulfillmentRepository(directory).fulfill(proposed, release)).resource, resource);
    assert.equal(releases, 0);
  } finally {
    await rm(root, { recursive: true });
  }
});

test("failed resource read requires explicit reconciliation before a real retry", async () => {
  const root = await mkdtemp(join(tmpdir(), "next-fulfillment-retry-"));
  try {
    const source = join(root, "resource.txt");
    const directory = join(root, "state");
    const release = async () => ({ contentType: "text/plain", body: await readFile(source, "utf8") });
    await assert.rejects(new FileFulfillmentRepository(directory).fulfill(proposed, release), { code: "ENOENT" });
    await writeFile(source, "available after retry");
    await assert.rejects(new FileFulfillmentRepository(directory).fulfill(proposed, release), { code: "fulfillment-outcome-unknown" });
    await new FileFulfillmentRepository(directory).reconcile(proposed, release);
    assert.equal((await new FileFulfillmentRepository(directory).fulfill(proposed, release)).resource.body, "available after retry");
  } finally {
    await rm(root, { recursive: true });
  }
});


test("a process crash after release retains its durable claim until actual reconciliation", async () => {
  const { spawn } = await import("node:child_process");
  const { once } = await import("node:events");
  const root = await mkdtemp(join(tmpdir(), "next-fulfillment-crash-"));
  try {
    const source = join(root, "released.json");
    const directory = join(root, "state");
    const input = join(root, "input.json");
    await writeFile(input, JSON.stringify({ ...proposed, canonicalReceipt: [...proposed.canonicalReceipt] }));
    const module = new URL("../lib/fulfillment.mjs", import.meta.url).href;
    const child = spawn(process.execPath, ["--input-type=module", "-e", `
      import { readFile, open } from "node:fs/promises";
      import { FileFulfillmentRepository } from ${JSON.stringify(module)};
      const proposed = JSON.parse(await readFile(process.argv[1], "utf8"));
      proposed.canonicalReceipt = Uint8Array.from(proposed.canonicalReceipt);
      await new FileFulfillmentRepository(process.argv[2]).fulfill(proposed, async () => {
        const file = await open(process.argv[3], "wx", 0o600);
        await file.writeFile(JSON.stringify({ contentType: "text/plain", body: "released before crash" }));
        await file.sync();
        await file.close();
        process.kill(process.pid, "SIGKILL");
      });
    `, input, directory, source], { stdio: ["ignore", "ignore", "inherit"] });
    const [code, signal] = await once(child, "exit");
    assert.equal(code, null);
    assert.equal(signal, "SIGKILL");
    const released = JSON.parse(await readFile(source, "utf8"));
    const repository = new FileFulfillmentRepository(directory);
    const secondRelease = async () => {
      await writeFile(source, "duplicate", { flag: "wx" });
      return released;
    };
    await assert.rejects(repository.fulfill(proposed, secondRelease), { code: "fulfillment-outcome-unknown" });
    assert.deepEqual(JSON.parse(await readFile(source, "utf8")), released);
    await assert.rejects(repository.reconcile({ ...proposed, requestDigest: "ef".repeat(32) }, async () => released), { code: "fulfillment-conflict" });
    assert.deepEqual((await repository.reconcile(proposed, async () => JSON.parse(await readFile(source, "utf8")))).resource, released);
    assert.deepEqual((await new FileFulfillmentRepository(directory).fulfill(proposed, secondRelease)).resource, released);
    assert.deepEqual(JSON.parse(await readFile(source, "utf8")), released);
  } finally {
    await rm(root, { recursive: true });
  }
});
