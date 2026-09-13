import { readFile, writeFile } from "node:fs/promises";

import { copyEntries } from "./catalog.ts";

const keys = new Set<string>();
for (const entry of copyEntries) {
  if (keys.has(entry.key)) throw new Error(`Duplicate copy key: ${entry.key}`);
  keys.add(entry.key);
}
const output = [
  "export const runtimeMessages: readonly (readonly [string, string])[] = [",
  ...copyEntries.map(({ key, message }) => `  ${JSON.stringify([key, message])},`),
  "];",
  "",
].join("\n");
const target = new URL("./messages.generated.ts", import.meta.url);
if (process.argv.includes("--check")) {
  if (await readFile(target, "utf8") !== output) {
    throw new Error("Runtime copy is stale; run npm run build:copy");
  }
} else {
  await writeFile(target, output);
}
