import { createHash, createPrivateKey, createPublicKey, sign } from "node:crypto";
import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join } from "node:path";
const fixture = JSON.parse(readFileSync(new URL("../../../sdk/conformance/fixtures/receipt-positive-v2.json", import.meta.url), "utf8"));
const bytes = (value) => Buffer.from(value, "hex");
const hash = (...parts) => createHash("sha256").update(Buffer.concat(parts)).digest();
const root = fileURLToPath(new URL("../../../../", import.meta.url));
const build = join(root, ".lane-target", "pay6-fixtures");
mkdirSync(build, { recursive: true });
const executable = join(build, "batch-fixture");
const run = (command, args) => {
  const result = spawnSync(command, args, { cwd: root, encoding: "utf8" });
  if (result.error || result.status !== 0) {
    throw new Error(`batch-fixture-command-failed: ${result.error?.message ?? result.stderr}`);
  }
  return result.stdout.trim();
};
run(process.env.CC ?? "gcc", [
  "-std=c17", "-pedantic", "-Werror", "-Wall", "-Wextra", "-Wconversion", "-Wshadow", "-Wvla", "-O2", "-Iinclude",
  "platform/middleware/conformance/pay6/batch_fixture.c", "src/sequencer/lxp_batch_header.c",
  "src/codec/lxp_codec.c", "src/protocol/lxp_arena.c", "src/protocol/lxp_protocol.c",
  "src/protocol/lxp_u128.c", "src/crypto/lxp_hash.c", "src/crypto/lxp_ct.c", "-o", executable,
]);
const key = createPrivateKey({ key: Buffer.concat([bytes("302e020100300506032b657004220420"), Buffer.alloc(32, 0x33)]), format: "der", type: "pkcs8" });
const publicKey = createPublicKey(key).export({ format: "der", type: "spki" }).subarray(-32);
if (publicKey.toString("hex") !== fixture.authorized_batch.sequencer_public_key_hex) throw new Error("fixture-key-mismatch");
const sequencerId = Buffer.alloc(32, 7);
const header = bytes(run(executable, [
  String(fixture.expected.protocol_version), "7", String(fixture.expected.timestamp_ms),
  fixture.authorized_batch.previous_state_root_hex, fixture.authorized_batch.resulting_state_root_hex,
  hash(Buffer.from("LXP/v1/merkle-leaf\0"), bytes(fixture.canonical_receipt_hex)).toString("hex"),
  sequencerId.toString("hex"),
]));
if (header.length !== 354) throw new Error("fixture-header-width");
const signature = sign(null, hash(Buffer.from("LXP/v1/batch-header\0"), header), key);
process.stdout.write(JSON.stringify({ header: header.toString("hex"), signature: signature.toString("hex"), sequencer_id: sequencerId.toString("hex") }, null, 2) + "\n");
