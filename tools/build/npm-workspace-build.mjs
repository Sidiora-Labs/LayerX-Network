#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const DEPENDENCY_FIELDS = ["dependencies", "devDependencies", "peerDependencies", "optionalDependencies"];

function fail(message) {
  process.stderr.write(`npm-workspace-build: ${message}\n`);
  process.exit(1);
}

function manifest(relative) {
  const path = join(ROOT, relative, "package.json");
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    fail(`cannot read ${path}: ${error.message}`);
  }
}

function declaredWorkspaces() {
  const patterns = manifest(".").workspaces;
  if (!Array.isArray(patterns) || patterns.length === 0) {
    fail("the root package.json declares no workspaces");
  }
  const globbed = patterns.filter((pattern) => /[*?[\]{}]/u.test(pattern));
  if (globbed.length > 0) {
    fail(`the workspace build order needs literal workspace paths; rewrite ${globbed.join(", ")}`);
  }
  return patterns;
}

function loadGraph() {
  const paths = declaredWorkspaces();
  const packages = paths.map((path) => {
    const declared = manifest(path);
    if (typeof declared.name !== "string" || declared.name.length === 0) {
      fail(`workspace ${path} declares no name`);
    }
    const dependencies = new Set();
    for (const field of DEPENDENCY_FIELDS) {
      for (const name of Object.keys(declared[field] ?? {})) {
        dependencies.add(name);
      }
    }
    return { path, name: declared.name, dependencies, build: typeof declared.scripts?.build === "string" };
  });
  const byName = new Map();
  for (const entry of packages) {
    const existing = byName.get(entry.name);
    if (existing !== undefined) {
      fail(`workspaces ${existing.path} and ${entry.path} both declare ${entry.name}`);
    }
    byName.set(entry.name, entry);
  }
  for (const entry of packages) {
    entry.requires = [...entry.dependencies].filter((name) => name !== entry.name && byName.has(name));
  }
  return packages;
}

function buildOrder(packages) {
  const pending = new Map(packages.map((entry) => [entry.name, new Set(entry.requires)]));
  const ordered = [];
  while (ordered.length < packages.length) {
    const ready = packages.filter((entry) => pending.has(entry.name) && pending.get(entry.name).size === 0);
    if (ready.length === 0) {
      const cycle = [...pending.keys()].join(", ");
      fail(`the workspace dependencies do not order; a cycle remains between ${cycle}`);
    }
    for (const entry of ready) {
      pending.delete(entry.name);
      ordered.push(entry);
    }
    for (const remaining of pending.values()) {
      for (const entry of ready) {
        remaining.delete(entry.name);
      }
    }
  }
  return ordered;
}

const packages = loadGraph();
const ordered = buildOrder(packages);

if (process.argv.slice(2).includes("--print-order")) {
  for (const entry of ordered) {
    process.stdout.write(`${entry.build ? "build" : "skip "} ${entry.name} (${entry.path})\n`);
  }
  process.exit(0);
}

if (process.argv.length > 2) {
  fail(`unknown argument ${process.argv.slice(2).join(" ")}; the only option is --print-order`);
}

for (const entry of ordered) {
  if (!entry.build) {
    continue;
  }
  const result = spawnSync("npm", ["run", "build", "--workspace", entry.name], { cwd: ROOT, stdio: "inherit" });
  if (result.error !== undefined) {
    fail(`cannot run npm for ${entry.name}: ${result.error.message}`);
  }
  if (result.signal !== null) {
    fail(`${entry.name} was terminated by ${result.signal}`);
  }
  if (result.status !== 0) {
    process.stderr.write(`npm-workspace-build: ${entry.name} failed its build with exit code ${result.status}\n`);
    process.exit(result.status);
  }
}
