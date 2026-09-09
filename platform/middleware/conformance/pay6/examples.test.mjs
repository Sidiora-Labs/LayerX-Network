import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
const root = new URL("../../../../", import.meta.url).pathname;
for (const [binary, script] of [[process.execPath, "platform/middleware/examples/public-rpc.mjs"], ["python3", "platform/middleware/examples/public_rpc.py"]]) {
  test(`${binary} example reads endpoint environment and refuses persistent host port`, () => {
    const env = { PATH: process.env.PATH, PYTHONPATH: root + "agent/sdk/python", LAYERX_RPC_URL: "http://127.0.0.1:18545/rpc", LAYERX_DID: "did:lxp:example" };
    const help = spawnSync(binary, [root + script, "--help"], { env });
    assert.equal(help.status, 0, help.stderr.toString());
    const refused = spawnSync(binary, [root + script], { env });
    assert.notEqual(refused.status, 0);
    assert.match(refused.stderr.toString(), /persistent-host-chain-forbidden/);
  });
}
