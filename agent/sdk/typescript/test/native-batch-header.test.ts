import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

import {
  decodeBatchHeader,
  verifyBatchInclusion,
  type MerkleProof,
  type SequencerAuthorization,
} from "../src/verifier.js";

function digest(domain: string, ...parts: readonly Uint8Array[]): Buffer {
  const hash = createHash("sha256").update(domain);
  for (const part of parts) hash.update(part);
  return hash.digest();
}

function proof(leaves: readonly Uint8Array[], index: number): MerkleProof {
  let level = leaves.map(leaf => digest("LXP/v1/merkle-leaf\0", leaf));
  let position = index;
  const siblings: Uint8Array[] = [];
  while (level.length > 1) {
    siblings.push(level[position ^ 1] ?? level[position]!);
    const next: Buffer[] = [];
    for (let offset = 0; offset < level.length; offset += 2) {
      next.push(digest("LXP/v1/merkle-internal\0", level[offset]!, level[offset + 1] ?? level[offset]!));
    }
    position = Math.floor(position / 2);
    level = next;
  }
  return { leafIndex: index, leafCount: leaves.length, siblings };
}

for (const [directory, names] of [
  ["custody/daemon-credit-receipt", ["credit.receipt", "maintenance.receipt"]],
  ["programs/maintained-multicall", ["receipt-0", "receipt-1", "maintenance.receipt"]],
] as const) {
  const read = (name: string): Buffer => readFileSync(new URL(`../../../../../tests/fixtures/${directory}/${name}`, import.meta.url));
  const canonical = read("header");
  const header = decodeBatchHeader(canonical);
  const signature = read("header.signature");
  const leaves = names.map(read);
  const authority: SequencerAuthorization = {
    sequencerId: header.sequencerId,
    publicKey: read("sequencer.public"),
    firstBatchNumber: header.batchNumber,
    lastBatchNumber: header.batchNumber,
  };
  assert.equal(canonical.length, 354);
  assert.equal(header.protocolVersion, 3);
  assert.equal(header.lastSequence - header.firstSequence + 1n, BigInt(leaves.length));
  const verify = (leaf: Uint8Array, path: MerkleProof, bytes = canonical, signed = signature, trusted = authority) =>
    verifyBatchInclusion("receipt", leaf, path, bytes, signed, trusted, { protocolVersion: 3 });
  for (const [index, leaf] of leaves.entries()) {
    const result = await verify(leaf, proof(leaves, index));
    assert.equal(result.level, "batch-included");
    assert.deepEqual(result.header, header);
    assert.deepEqual(result.root, header.receiptMerkleRoot);
    assert.deepEqual(Buffer.from(result.headerDigest), digest("LXP/v1/batch-header\0", canonical));
  }
  for (let length = 0; length < canonical.length; length++) {
    assert.throws(() => decodeBatchHeader(canonical.subarray(0, length)));
  }
  assert.throws(() => decodeBatchHeader(Buffer.concat([canonical, Buffer.of(0)])));
  for (const outer of [0, 1, 2, 3, 4, 65535]) {
    for (const inner of [0, 1, 2, 3, 4, 65535]) {
      const changed = Buffer.from(canonical);
      changed.writeUInt16BE(outer, 0);
      changed.writeUInt16BE(inner, 6);
      if (outer === inner && [1, 2, 3].includes(outer)) {
        assert.equal(decodeBatchHeader(changed).protocolVersion, outer);
      } else {
        assert.throws(() => decodeBatchHeader(changed));
      }
      if (outer !== 3 || inner !== 3) await assert.rejects(verify(leaves[0]!, proof(leaves, 0), changed));
    }
  }
  for (const offset of [2, 3, 4, 5, 8, 13, 22, 31, 40, 49, 50, 86, 123, 160, 197, 234, 271, 308, 317]) {
    const changed = Buffer.from(canonical);
    changed[offset] = changed[offset]! ^ 1;
    assert.throws(() => decodeBatchHeader(changed));
  }
  for (const offset of [10, 14, 24, 33, 42, 55, 92, 130, 165, 205, 240, 280, 313, 330, 353]) {
    const changed = Buffer.from(canonical);
    changed[offset] = changed[offset]! ^ 1;
    await assert.rejects(verify(leaves[0]!, proof(leaves, 0), changed));
  }
  await assert.rejects(verify(leaves[0]!, proof(leaves, 1)));
  await assert.rejects(verify(leaves[0]!, proof(leaves, 0), canonical, Buffer.alloc(64)));
  await assert.rejects(verify(leaves[0]!, proof(leaves, 0), canonical, signature, { ...authority, publicKey: Buffer.alloc(32) }));
  await assert.rejects(verify(leaves[0]!, proof(leaves, 0), canonical, signature, { ...authority, sequencerId: Buffer.alloc(32) }));
  await assert.rejects(verify(leaves[0]!, proof(leaves, 0), canonical, signature, { ...authority, firstBatchNumber: header.batchNumber + 1n }));
  await assert.rejects(verify(leaves[0]!, proof(leaves, 0), canonical, signature, { ...authority, lastBatchNumber: header.batchNumber - 1n }));
}

const checkpoint = JSON.parse(readFileSync(new URL("../../../../../tests/vectors/checkpoint/fresh.json", import.meta.url), "utf8")) as {
  header: { bytes: string };
};
assert.equal(decodeBatchHeader(Buffer.from(checkpoint.header.bytes.replace(/^0x/, ""), "hex")).protocolVersion, 2);
for (const name of ["batch", "checkpoint"]) {
  const fixture = JSON.parse(readFileSync(new URL(`../../../../../platform/middleware/conformance/pay6/${name}-version-mismatch.json`, import.meta.url), "utf8")) as {
    header?: string;
    checkpoint_evidence?: { canonical_header: string };
  };
  const malformed = Buffer.from(fixture.header ?? fixture.checkpoint_evidence!.canonical_header, "hex");
  assert.equal(malformed.readUInt16BE(0), 1);
  assert.equal(malformed.readUInt16BE(6), 2);
  assert.throws(() => decodeBatchHeader(malformed));
}
console.log("native batch headers and complete receipt inclusions passed with strict version binding");
