import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { DatabaseSync } from "node:sqlite";
import { parsePlatformSpec } from "../generate.mjs";

const toolRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repositoryRoot = resolve(toolRoot, "../..");
const dataPath = join(toolRoot, "data/codebase-map.json");

test("platform spec parser retains delivery state and exact dependencies", () => {
  const parsed = parsePlatformSpec(`
[req.9]
title = "Money Movement"

[task.9]
title = "Movement group"
status = "pending"

[task.9.1]
title = "Resolve a transfer"
status = "implemented"
wave = 4
requires = ["8.2"]
symbols = ["route::resolve"]
touches = ["src/route.rs"]
reqs = ["9.1"]
verify_cmd = "make test-route"
`);
  assert.deepEqual(parsed.requirements, [{ id: "9", title: "Money Movement" }]);
  assert.equal(parsed.tasks.length, 2);
  assert.equal(parsed.tasks[0].deliverable, false);
  assert.deepEqual(parsed.tasks[1], {
    id: "9.1",
    title: "Resolve a transfer",
    status: "implemented",
    wave: 4,
    section: null,
    requires: ["8.2"],
    reqs: ["9.1"],
    symbols: ["route::resolve"],
    touches: ["src/route.rs"],
    verifyCommand: "make test-route",
    deliverable: true,
  });
});

test("generated snapshot is internally referentially complete", { skip: !existsSync(dataPath) }, () => {
  const map = JSON.parse(readFileSync(dataPath, "utf8"));
  assert.equal(map.schemaVersion, 1);
  const unique = (items) => new Set(items.map((item) => item.id));
  const fileIds = unique(map.files);
  const symbolIds = unique(map.symbols);
  const packageIds = unique(map.packages);
  const areaIds = unique(map.areas);
  assert.equal(fileIds.size, map.files.length, "file IDs must be unique");
  assert.equal(symbolIds.size, map.symbols.length, "symbol IDs must be unique");
  assert.equal(packageIds.size, map.packages.length, "package IDs must be unique");
  assert.equal(areaIds.size, map.areas.length, "area IDs must be unique");
  for (const symbol of map.symbols) assert.ok(fileIds.has(symbol.file), `symbol ${symbol.id} has a file`);
  for (const edge of map.fileEdges) {
    assert.ok(fileIds.has(edge.source), `file edge source ${edge.source} exists`);
    assert.ok(fileIds.has(edge.target), `file edge target ${edge.target} exists`);
  }
  for (const edge of map.symbolEdges) {
    assert.ok(symbolIds.has(edge.target), `symbol edge target ${edge.target} exists`);
    if (edge.source) assert.ok(symbolIds.has(edge.source), `symbol edge source ${edge.source} exists`);
  }
  for (const edge of map.packageEdges) {
    assert.ok(packageIds.has(edge.source));
    assert.ok(packageIds.has(edge.target));
  }
  for (const edge of map.areaEdges) {
    assert.ok(areaIds.has(edge.source));
    assert.ok(areaIds.has(edge.target));
  }
  for (const entry of map.imports) {
    assert.ok(fileIds.has(entry.source), `import source ${entry.source} exists`);
    if (entry.target) assert.ok(fileIds.has(entry.target), `import target ${entry.target} exists`);
  }
  for (const route of map.routes) assert.ok(fileIds.has(route.file), `route file ${route.file} exists`);
  const taskIds = new Set(map.tasks.map((task) => task.id));
  for (const task of map.tasks) {
    for (const required of task.requires) assert.ok(taskIds.has(required), `task dependency ${required} exists`);
  }
  for (const link of map.taskLinks) {
    assert.ok(taskIds.has(link.task), `task link ${link.task} exists`);
    if (link.file) assert.ok(fileIds.has(link.file), `task file ${link.file} exists`);
    if (link.symbol) assert.ok(symbolIds.has(link.symbol), `task symbol ${link.symbol} exists`);
  }
});

test("generated counts and commit match the live repository sources", { skip: !existsSync(dataPath) }, () => {
  const map = JSON.parse(readFileSync(dataPath, "utf8"));
  const git = (args) => execFileSync("git", args, { cwd: repositoryRoot, encoding: "utf8" }).trim();
  const database = new DatabaseSync(join(repositoryRoot, ".codegraph/graph.db"), { readOnly: true });
  assert.equal(map.meta.commit, git(["rev-parse", "HEAD"]));
  const trackedRecords = execFileSync("git", ["ls-files", "--stage", "-z"], {
    cwd: repositoryRoot,
    maxBuffer: 32 * 1024 * 1024,
  }).toString("utf8").split("\0").filter(Boolean).map((record) => {
    const tab = record.indexOf("\t");
    return { path: record.slice(tab + 1), blob: record.slice(0, tab).split(" ")[1] };
  });
  assert.equal(map.coverage.trackedFiles, trackedRecords.length);
  const generatedTracked = new Map(map.files.filter((file) => file.tracked).map((file) => [file.path, file.gitBlob]));
  assert.equal(generatedTracked.size, trackedRecords.length);
  for (const record of trackedRecords) assert.equal(generatedTracked.get(record.path), record.blob, `Git blob for ${record.path}`);
  assert.equal(map.coverage.graphFiles, database.prepare("SELECT count(*) count FROM files").get().count);
  assert.equal(map.coverage.symbols, database.prepare("SELECT count(*) count FROM symbols").get().count);
  assert.equal(map.coverage.references, database.prepare("SELECT count(*) count FROM refs").get().count);
  assert.equal(map.coverage.imports, database.prepare("SELECT count(*) count FROM imports").get().count);
  assert.equal(map.coverage.routes, database.prepare("SELECT count(*) count FROM routes").get().count);
  const generatedIndexedPaths = new Set(map.files.filter((file) => file.indexed).map((file) => file.path));
  const liveIndexedPaths = database.prepare("SELECT path FROM files").all().map((row) => row.path);
  assert.equal(generatedIndexedPaths.size, liveIndexedPaths.length);
  for (const path of liveIndexedPaths) assert.ok(generatedIndexedPaths.has(path), `indexed path ${path} is visible`);
  assert.equal(map.coverage.unionFiles, map.coverage.indexedUntrackedFiles + map.coverage.trackedFiles);
  assert.equal(map.coverage.graphFiles, map.coverage.indexedTrackedFiles + map.coverage.indexedUntrackedFiles);
  const specStatus = JSON.parse(execFileSync("cg", ["spec", "status", "--json"], { cwd: repositoryRoot, encoding: "utf8" }));
  assert.equal(map.coverage.tasks, specStatus.tasks);
  const statusCounts = map.tasks.filter((task) => task.deliverable).reduce((counts, task) => {
    counts[task.status] = (counts[task.status] ?? 0) + 1;
    return counts;
  }, {});
  assert.equal(statusCounts.done, specStatus.done);
  assert.equal(statusCounts.implemented, specStatus.implemented);
  assert.equal(statusCounts.in_progress, specStatus.in_progress);
  assert.equal(statusCounts.pending, specStatus.pending);
});

test("browser renderer does not inject repository strings as HTML", () => {
  const source = readFileSync(join(toolRoot, "app.js"), "utf8");
  assert.doesNotMatch(source, /\.innerHTML\s*=/);
  assert.doesNotMatch(source, /insertAdjacentHTML/);
  assert.doesNotMatch(source, /\.style\.[a-zA-Z]+\s*=/);
  assert.match(source, /textContent/);
});

test("browser module completes its first render against the generated snapshot", { skip: !existsSync(dataPath) }, async () => {
  class ClassList {
    values = new Set();
    add(...names) { names.forEach((name) => this.values.add(name)); }
    remove(...names) { names.forEach((name) => this.values.delete(name)); }
    toggle(name, force) {
      const enabled = force === undefined ? !this.values.has(name) : force;
      if (enabled) this.values.add(name); else this.values.delete(name);
      return enabled;
    }
  }
  class FakeElement {
    constructor(tag = "div", id = "") {
      this.tagName = tag.toUpperCase();
      this.id = id;
      this.children = [];
      this.dataset = {};
      this.classList = new ClassList();
      this.hidden = false;
      this.value = "";
      this.parentElement = null;
      this.textContent = "";
      this.width = 0;
      this.height = 0;
      this.listeners = new Map();
    }
    append(...children) {
      for (const child of children) {
        this.children.push(child);
        if (child && typeof child === "object") child.parentElement = this;
      }
    }
    replaceChildren(...children) { this.children = []; this.append(...children); }
    addEventListener(type, handler) {
      const handlers = this.listeners.get(type) ?? [];
      handlers.push(handler);
      this.listeners.set(type, handlers);
    }
    click() { for (const handler of this.listeners.get("click") ?? []) handler({ target: this }); }
    setAttribute(name, value) { this[name] = value; }
    setPointerCapture() {}
    focus() { fakeDocument.activeElement = this; }
    showModal() { this.open = true; }
    remove() {
      if (this.parentElement) this.parentElement.children = this.parentElement.children.filter((child) => child !== this);
      this.removed = true;
    }
    getBoundingClientRect() { return { width: 960, height: 640, left: 0, top: 0 }; }
    getContext() {
      return {
        setTransform() {}, clearRect() {}, beginPath() {}, moveTo() {}, lineTo() {}, stroke() {}, arc() {},
        fill() {}, closePath() {}, fillText() {},
      };
    }
  }
  const ids = [
    "app", "loading", "snapshot-label", "dirty-indicator", "branch-label", "commit-label", "metric-strip",
    "view-nav", "search", "search-results", "inventory", "inventory-title", "inventory-count", "inventory-note",
    "breadcrumbs", "view-kicker", "view-title", "render-note", "back-button", "fit-button", "fit-toolbar-button",
    "help-button", "help-dialog", "graph", "canvas-shell", "graph-empty", "legend", "inspector",
    "inspector-empty", "inspector-content",
  ];
  const nodes = new Map(ids.map((id) => [id, new FakeElement(id === "graph" ? "canvas" : "div", id)]));
  const graphParent = new FakeElement("section", "graph-parent");
  nodes.get("graph").parentElement = graphParent;
  const fakeDocument = {
    activeElement: null,
    getElementById: (id) => nodes.get(id),
    createElement: (tag) => new FakeElement(tag),
    addEventListener() {},
  };
  const saved = new Map();
  function replaceGlobal(name, value) {
    saved.set(name, Object.getOwnPropertyDescriptor(globalThis, name));
    Object.defineProperty(globalThis, name, { configurable: true, writable: true, value });
  }
  replaceGlobal("document", fakeDocument);
  replaceGlobal("window", globalThis);
  replaceGlobal("navigator", { clipboard: { writeText: async () => {} } });
  replaceGlobal("ResizeObserver", class { observe() {} });
  replaceGlobal("requestAnimationFrame", () => 1);
  replaceGlobal("fetch", async () => ({ ok: true, json: async () => JSON.parse(readFileSync(dataPath, "utf8")) }));
  try {
    const module = await import(`../app.js?smoke=${Date.now()}`);
    await module.initialization;
    assert.equal(nodes.get("app").hidden, false);
    assert.equal(nodes.get("loading").removed, true);
    assert.equal(nodes.get("view-nav").children.length, 7);
    assert.equal(nodes.get("metric-strip").children.length, 6);
    assert.match(nodes.get("render-note").textContent, /nodes/);
    const expectedTitles = [
      "System relationships", "Manifest-scope relationships", "File relationships", "Resolved symbol calls",
      "Import relationships", "Route to handler map", "Task dependency graph",
    ];
    for (let position = 0; position < nodes.get("view-nav").children.length; position += 1) {
      nodes.get("view-nav").children[position].click();
      assert.equal(nodes.get("view-title").textContent, expectedTitles[position]);
      assert.match(nodes.get("render-note").textContent, /nodes/);
    }
  } finally {
    for (const [name, descriptor] of saved) {
      if (descriptor) Object.defineProperty(globalThis, name, descriptor);
      else delete globalThis[name];
    }
  }
});
