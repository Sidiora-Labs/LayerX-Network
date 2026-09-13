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

test("failed resource read rolls back and permits a real retry", async () => {
  const root = await mkdtemp(join(tmpdir(), "next-fulfillment-retry-"));
  try {
    const source = join(root, "resource.txt");
    const directory = join(root, "state");
    const release = async () => ({ contentType: "text/plain", body: await readFile(source, "utf8") });
    await assert.rejects(new FileFulfillmentRepository(directory).fulfill(proposed, release), { code: "ENOENT" });
    await writeFile(source, "available after retry");
    assert.equal((await new FileFulfillmentRepository(directory).fulfill(proposed, release)).resource.body, "available after retry");
  } finally {
    await rm(root, { recursive: true });
  }
});
