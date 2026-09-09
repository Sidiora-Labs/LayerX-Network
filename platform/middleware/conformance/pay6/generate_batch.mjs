import { createHash, createPrivateKey, createPublicKey, sign } from "node:crypto";
import { readFileSync } from "node:fs";
const fixture = JSON.parse(readFileSync(new URL("../../../sdk/conformance/fixtures/receipt-positive-v2.json", import.meta.url), "utf8"));
const bytes = (value) => Buffer.from(value, "hex");
const integer = (value, size) => {
  let remaining = BigInt(value);
  const result = Buffer.alloc(size);
  for (let index = size - 1; index >= 0; index--) { result[index] = Number(remaining & 255n); remaining >>= 8n; }
  if (remaining !== 0n) throw new Error("overflow");
  return result;
};
const bounded = (value) => Buffer.concat([integer(value.length, 4), value]);
const hash = (...parts) => createHash("sha256").update(Buffer.concat(parts)).digest();
const key = createPrivateKey({ key: Buffer.concat([bytes("302e020100300506032b657004220420"), Buffer.alloc(32, 0x33)]), format: "der", type: "pkcs8" });
const publicKey = createPublicKey(key).export({ format: "der", type: "spki" }).subarray(-32);
if (publicKey.toString("hex") !== fixture.authorized_batch.sequencer_public_key_hex) throw new Error("fixture-key-mismatch");
const sequencerId = Buffer.alloc(32, 7);
const fields = [integer(2, 2), integer(7, 4), integer(1, 8), integer(1, 8), integer(1, 8), integer(1, 8),
  bounded(bytes(fixture.authorized_batch.previous_state_root_hex)), bounded(bytes(fixture.authorized_batch.resulting_state_root_hex)),
  bounded(Buffer.alloc(32)), bounded(hash(Buffer.from("LXP/v1/merkle-leaf\0"), bytes(fixture.canonical_receipt_hex))),
  bounded(Buffer.alloc(32)), bounded(Buffer.alloc(32, 9)), bounded(Buffer.alloc(32)), integer(fixture.expected.timestamp_ms, 8), bounded(sequencerId)];
const header = Buffer.concat([bytes("000117010f"), ...fields.flatMap((field, index) => [integer(index + 1, 1), field])]);
const signature = sign(null, hash(Buffer.from("LXP/v1/batch-header\0"), header), key);
process.stdout.write(JSON.stringify({ header: header.toString("hex"), signature: signature.toString("hex"), sequencer_id: sequencerId.toString("hex") }, null, 2) + "\n");
