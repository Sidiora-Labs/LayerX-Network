import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const dashboard = JSON.parse(readFileSync("platform/hosted/dashboard/web/package.json", "utf8"));
const lock = JSON.parse(readFileSync("human/apps/web/package-lock.json", "utf8"));

for (const dependencies of [dashboard.dependencies, dashboard.devDependencies]) {
  for (const [name, expected] of Object.entries(dependencies)) {
    if (name === "@layerx/ui") {
      assert.equal(expected, "file:../../../../human/apps/web/packages/layerx-ui");
      assert.equal(lock.packages["packages/layerx-ui"]?.name, name);
      continue;
    }
    assert.equal(
      lock.packages[`node_modules/${name}`]?.version,
      expected,
      `${name} must exactly match human/apps/web/package-lock.json`,
    );
  }
}
