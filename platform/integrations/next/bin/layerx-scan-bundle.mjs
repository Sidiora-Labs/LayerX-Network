#!/usr/bin/env node
import { readFile, readdir, stat } from "node:fs/promises";
import { join, relative, resolve } from "node:path";
import {
  bundleScanReport,
  collectSecretNames,
  collectSecretValues,
  scanBundleArtifacts,
} from "../dist/scan.js";

const DEFAULT_ROOTS = [".next/static", "public", "out"];
const MAXIMUM_ARTIFACT_BYTES = 32 * 1024 * 1024;

const roots = process.argv.slice(2).length > 0 ? process.argv.slice(2) : DEFAULT_ROOTS;

const collect = async (root, base, artifacts) => {
  let entries;
  try {
    entries = await readdir(root, { withFileTypes: true });
  } catch (error) {
    if (error.code === "ENOENT") return artifacts;
    throw error;
  }
  for (const entry of entries) {
    const path = join(root, entry.name);
    if (entry.isDirectory()) {
      await collect(path, base, artifacts);
      continue;
    }
    if (!entry.isFile()) continue;
    const info = await stat(path);
    if (info.size > MAXIMUM_ARTIFACT_BYTES) {
      throw new Error(`artifact_too_large:${path}`);
    }
    artifacts.push({ path: relative(base, path), bytes: new Uint8Array(await readFile(path)) });
  }
  return artifacts;
};

const base = resolve(".");
const artifacts = [];
for (const root of roots) {
  await collect(resolve(root), base, artifacts);
}
if (artifacts.length === 0) {
  throw new Error("no_browser_artifacts");
}

const findings = scanBundleArtifacts({
  artifacts,
  secretValues: collectSecretValues(process.env),
  secretNames: collectSecretNames(process.env),
});

process.stdout.write(`${bundleScanReport(findings)}\n`);
process.exit(findings.length === 0 ? 0 : 1);
