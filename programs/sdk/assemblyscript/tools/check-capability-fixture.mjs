import { readFileSync } from "node:fs";

const fixture = readFileSync(new URL("../../vectors/capability-boundary.kvx", import.meta.url), "utf8");
const source = readFileSync(new URL("../tests/safety.ts", import.meta.url), "utf8");
const fixtureMatch = fixture.match(/\[vector\.mixed_v1\][\s\S]*?encoded_hex\s*=\s*"([0-9a-f]+)"/);
const sourceMatch = source.match(/const MIXED_FIXTURE_HEX = "([0-9a-f]+)";/);

if (fixtureMatch === null || sourceMatch === null || fixtureMatch[1] !== sourceMatch[1]) {
  throw new Error("AssemblyScript capability parity fixture is stale");
}
