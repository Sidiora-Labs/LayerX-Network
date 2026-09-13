import assert from "node:assert/strict";
import test from "node:test";

import { formatCopy } from "../copy/format.ts";
import { copyEntries } from "../copy/catalog.ts";
import { copyEntry, human_copy_catalog } from "../copy/runtime.ts";

test("browser copy preserves every catalogued message and refuses unknown keys", () => {
  assert.equal(human_copy_catalog().size, copyEntries.length);
  for (const { key, message } of copyEntries) {
    assert.deepEqual(copyEntry(key), { key, message });
  }
  assert.equal(human_copy_catalog().get("missing.copy.key"), undefined);
  assert.throws(() => copyEntry("missing.copy.key"), /Unknown copy key/u);
});

test("the copy formatter supports ICU plural branches", () => {
  assert.equal(formatCopy("approval.count", { count: 0 }), "No approvals waiting");
  assert.equal(formatCopy("approval.count", { count: 1 }), "1 approval waiting");
  assert.equal(formatCopy("approval.count", { count: 3 }), "3 approvals waiting");
});

test("the copy formatter supports ICU selection branches", () => {
  assert.equal(formatCopy("movement.direction", { direction: "inbound" }), "Money in");
  assert.equal(formatCopy("movement.direction", { direction: "outbound" }), "Money out");
});
