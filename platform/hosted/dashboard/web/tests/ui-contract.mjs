import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const source = await readFile(new URL("../app/page.tsx", import.meta.url), "utf8");
for (const surface of ["API keys", "Requests", "Webhook endpoints", "Delivery log", "test payments", "verified receipts"]) {
  assert.match(source, new RegExp(surface, "i"), `developer surface missing: ${surface}`);
}
assert.match(source, /from "@layerx\/ui"/, "dashboard must consume the owner UI package");
assert.doesNotMatch(source, /x-layerx-principal/i, "dashboard must never trust an identity header");
