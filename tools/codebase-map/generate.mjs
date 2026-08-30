import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { existsSync, lstatSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, extname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { DatabaseSync } from "node:sqlite";

const toolRoot = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(toolRoot, "../..");
const databasePath = join(repositoryRoot, ".codegraph/graph.db");
const specPath = join(repositoryRoot, "spec/layerx-platform/spec.kvx");
const outputPath = join(toolRoot, "data/codebase-map.json");

const GRAPH_SCHEMA_VERSION = "10";
const GENERATED_SCHEMA_VERSION = 1;

function git(args, options = {}) {
  return execFileSync("git", args, {
    cwd: repositoryRoot,
    encoding: options.encoding ?? "utf8",
    maxBuffer: 256 * 1024 * 1024,
  });
}

function zeroSeparated(buffer) {
  return buffer.toString("utf8").split("\0").filter(Boolean);
}

function parseTrackedFiles() {
  const records = zeroSeparated(git(["ls-files", "--stage", "-z"], { encoding: "buffer" }));
  return records.map((record) => {
    const tab = record.indexOf("\t");
    const [mode, blob, stage] = record.slice(0, tab).split(" ");
    return { path: record.slice(tab + 1), mode, blob, stage: Number(stage) };
  });
}

function parseStatus() {
  const records = zeroSeparated(
    git(["status", "--porcelain=v1", "-z", "--untracked-files=all"], { encoding: "buffer" }),
  );
  const statuses = new Map();
  for (let index = 0; index < records.length; index += 1) {
    const record = records[index];
    const code = record.slice(0, 2);
    const path = record.slice(3);
    statuses.set(path, code);
    if (/[RC]/.test(code)) index += 1;
  }
  return statuses;
}

function inferLanguage(path) {
  const exact = new Map([
    ["Cargo.toml", "toml"], ["Cargo.lock", "toml"], ["go.mod", "go-module"],
    ["go.sum", "go-module"], ["package.json", "json"], ["Makefile", "make"],
    ["Dockerfile", "docker"], ["Jenkinsfile", "groovy"],
  ]);
  const name = path.split("/").at(-1);
  if (exact.has(name)) return exact.get(name);
  const extension = extname(path).toLowerCase();
  return new Map([
    [".rs", "rust"], [".c", "c"], [".h", "c"], [".cc", "cpp"], [".cpp", "cpp"],
    [".ts", "typescript"], [".tsx", "typescript"], [".js", "javascript"],
    [".mjs", "javascript"], [".cjs", "javascript"], [".jsx", "javascript"],
    [".py", "python"], [".go", "go"], [".java", "java"], [".kt", "kotlin"],
    [".kts", "kotlin"], [".swift", "swift"], [".sol", "solidity"], [".cs", "csharp"],
    [".toml", "toml"], [".json", "json"], [".kvx", "kvx"], [".md", "markdown"],
    [".yml", "yaml"], [".yaml", "yaml"], [".xml", "xml"], [".sh", "shell"],
    [".sql", "sql"], [".proto", "protobuf"], [".graphql", "graphql"],
    [".html", "html"], [".css", "css"], [".scss", "scss"], [".svelte", "svelte"],
    [".vue", "vue"], [".wasm", "wasm"], [".wat", "wat"], [".lock", "lockfile"],
  ]).get(extension) ?? "other";
}

function areaFor(path) {
  return path.includes("/") ? path.slice(0, path.indexOf("/")) : "(root)";
}

function workingMetadata(path, changed) {
  const absolutePath = join(repositoryRoot, path);
  if (!existsSync(absolutePath)) return { workingSize: null, workingHash: null, deleted: true };
  const stat = lstatSync(absolutePath);
  if (!stat.isFile()) return { workingSize: stat.size, workingHash: null, deleted: false };
  let workingHash = null;
  if (changed && stat.size <= 16 * 1024 * 1024) {
    workingHash = createHash("sha256").update(readFileSync(absolutePath)).digest("hex");
  }
  return { workingSize: stat.size, workingHash, deleted: false };
}

function parseManifestName(path) {
  const absolutePath = join(repositoryRoot, path);
  if (!existsSync(absolutePath) || lstatSync(absolutePath).size > 2 * 1024 * 1024) return null;
  const source = readFileSync(absolutePath, "utf8");
  const filename = path.split("/").at(-1);
  try {
    if (filename === "package.json") return JSON.parse(source).name ?? null;
  } catch {
    return null;
  }
  if (filename === "Cargo.toml") {
    const packageSection = source.match(/(?:^|\n)\[package\]\s*\n([\s\S]*?)(?=\n\[|$)/)?.[1] ?? "";
    return packageSection.match(/^name\s*=\s*["']([^"']+)["']/m)?.[1] ?? null;
  }
  if (filename === "go.mod") return source.match(/^module\s+([^\s]+)\s*$/m)?.[1] ?? null;
  if (filename === "pyproject.toml") {
    const projectSection = source.match(/(?:^|\n)\[project\]\s*\n([\s\S]*?)(?=\n\[|$)/)?.[1] ?? "";
    return projectSection.match(/^name\s*=\s*["']([^"']+)["']/m)?.[1] ?? null;
  }
  if (filename === "Package.swift") return source.match(/name\s*:\s*"([^"]+)"/)?.[1] ?? null;
  if (filename === "pom.xml") return source.match(/<artifactId>([^<]+)<\/artifactId>/)?.[1] ?? null;
  return null;
}

function discoverPackages(paths) {
  const manifestNames = new Set([
    "Cargo.toml", "package.json", "go.mod", "pyproject.toml", "Package.swift", "pom.xml",
    "build.gradle", "build.gradle.kts",
  ]);
  const manifests = paths
    .filter((path) => manifestNames.has(path.split("/").at(-1)))
    .map((manifest) => {
      const directory = manifest.includes("/") ? manifest.slice(0, manifest.lastIndexOf("/")) : "";
      const fallback = directory ? directory.split("/").at(-1) : "LayerX protocol";
      return {
        id: "",
        name: parseManifestName(manifest) ?? fallback,
        directory,
        manifest,
        ecosystem: manifest.split("/").at(-1),
        area: areaFor(manifest),
      };
    })
    .sort((left, right) => right.directory.length - left.directory.length || left.manifest.localeCompare(right.manifest));

  const unique = [];
  const seen = new Set();
  for (const manifest of manifests) {
    const key = `${manifest.directory}\0${manifest.ecosystem}`;
    if (seen.has(key)) continue;
    seen.add(key);
    manifest.id = `p:${unique.length}`;
    unique.push(manifest);
  }
  return unique;
}

function packageFor(path, packages) {
  const found = packages.find(({ directory }) => !directory || path === directory || path.startsWith(`${directory}/`));
  return found?.id ?? null;
}

function parseSimpleValue(raw) {
  const value = raw.trim();
  if (value.startsWith("[") || value.startsWith("\"")) {
    try { return JSON.parse(value); } catch { return value.replace(/^"|"$/g, ""); }
  }
  if (/^-?\d+$/.test(value)) return Number(value);
  return value;
}

export function parsePlatformSpec(source) {
  const requirements = [];
  const tasks = [];
  let current = null;
  for (const line of source.split(/\r?\n/)) {
    const section = line.match(/^\[(req|task)\.([^\]]+)\]\s*$/);
    if (section) {
      if (current?.type === "req") requirements.push(current.value);
      if (current?.type === "task") tasks.push(current.value);
      current = { type: section[1], value: { id: section[2] } };
      continue;
    }
    if (!current) continue;
    const assignment = line.match(/^([a-zA-Z0-9_]+)\s*=\s*(.+)$/);
    if (!assignment) continue;
    current.value[assignment[1]] = parseSimpleValue(assignment[2]);
  }
  if (current?.type === "req") requirements.push(current.value);
  if (current?.type === "task") tasks.push(current.value);

  return {
    requirements: requirements.map(({ id, title }) => ({ id, title: title ?? `Requirement ${id}` })),
    tasks: tasks.map((task) => ({
      id: task.id,
      title: task.title ?? `Task ${task.id}`,
      status: task.status ?? "unknown",
      wave: Number.isInteger(task.wave) ? task.wave : null,
      section: task.section ?? null,
      requires: Array.isArray(task.requires) ? task.requires : [],
      reqs: Array.isArray(task.reqs) ? task.reqs : [],
      symbols: Array.isArray(task.symbols) ? task.symbols : [],
      touches: Array.isArray(task.touches) ? task.touches : [],
      verifyCommand: task.verify_cmd ?? null,
      deliverable: Number.isInteger(task.wave),
    })),
  };
}

function aggregateEdges(edges, sourceFor, targetFor) {
  const aggregated = new Map();
  for (const edge of edges) {
    const source = sourceFor(edge);
    const target = targetFor(edge);
    if (!source || !target || source === target) continue;
    const key = `${source}\0${target}`;
    const value = aggregated.get(key) ?? { source, target, calls: 0, imports: 0, total: 0 };
    value.calls += edge.calls ?? 0;
    value.imports += edge.imports ?? 0;
    value.total += edge.total ?? (edge.calls ?? 0) + (edge.imports ?? 0);
    aggregated.set(key, value);
  }
  return [...aggregated.values()].sort((left, right) => right.total - left.total);
}

function main() {
  if (!existsSync(databasePath)) throw new Error("Missing .codegraph/graph.db. Run `cg init` first.");
  if (!existsSync(specPath)) throw new Error("Missing authoritative spec/layerx-platform/spec.kvx.");

  const database = new DatabaseSync(databasePath, { readOnly: true });
  const metadata = Object.fromEntries(database.prepare("SELECT key, value FROM meta").all().map((row) => [row.key, row.value]));
  if (metadata.schema_version !== GRAPH_SCHEMA_VERSION) {
    throw new Error(`Unsupported Codify schema ${metadata.schema_version}; expected ${GRAPH_SCHEMA_VERSION}.`);
  }

  const tracked = parseTrackedFiles();
  const statusByPath = parseStatus();
  const graphFiles = database.prepare("SELECT id, path, lang, size, mtime, hash, lines FROM files ORDER BY path").all();
  const graphFileByPath = new Map(graphFiles.map((file) => [file.path, file]));
  const trackedByPath = new Map(tracked.map((file) => [file.path, file]));
  const paths = [...new Set([...trackedByPath.keys(), ...graphFileByPath.keys()])].sort();
  const packages = discoverPackages(paths);
  const packageById = new Map(packages.map((entry) => [entry.id, entry]));

  const symbolCounts = new Map(database.prepare("SELECT file_id, count(*) count FROM symbols GROUP BY file_id").all().map((row) => [row.file_id, row.count]));
  const referenceCounts = new Map();
  for (const row of database.prepare("SELECT file_id, coalesce(verdict, 'unresolved') verdict, count(*) count FROM refs GROUP BY file_id, verdict").all()) {
    const counts = referenceCounts.get(row.file_id) ?? { total: 0, internal: 0, external: 0, unknown: 0, unresolved: 0 };
    counts.total += row.count;
    if (row.verdict in counts) counts[row.verdict] += row.count;
    else counts.unresolved += row.count;
    referenceCounts.set(row.file_id, counts);
  }
  const churnCounts = new Map(database.prepare("SELECT path, count(*) count FROM git_churn GROUP BY path").all().map((row) => [row.path, row.count]));

  const files = paths.map((path, index) => {
    const graph = graphFileByPath.get(path);
    const gitFile = trackedByPath.get(path);
    const status = statusByPath.get(path) ?? "  ";
    const working = workingMetadata(path, status !== "  ");
    return {
      id: `f:${index}`,
      graphId: graph?.id ?? null,
      path,
      name: path.split("/").at(-1),
      area: areaFor(path),
      package: packageFor(path, packages),
      language: graph?.lang || inferLanguage(path),
      lines: graph?.lines ?? null,
      indexedSize: graph?.size ?? null,
      workingSize: working.workingSize,
      indexedHash: graph?.hash ?? null,
      gitBlob: gitFile?.blob ?? null,
      workingHash: working.workingHash,
      gitMode: gitFile?.mode ?? null,
      gitStatus: status,
      tracked: Boolean(gitFile),
      indexed: Boolean(graph),
      deleted: working.deleted,
      symbols: graph ? (symbolCounts.get(graph.id) ?? 0) : 0,
      references: graph ? (referenceCounts.get(graph.id) ?? { total: 0, internal: 0, external: 0, unknown: 0, unresolved: 0 }) : null,
      churn: churnCounts.get(path) ?? 0,
    };
  });
  const fileByGraphId = new Map(files.filter((file) => file.graphId !== null).map((file) => [file.graphId, file]));
  const fileByPath = new Map(files.map((file) => [file.path, file]));

  for (const entry of packages) {
    const owned = files.filter((file) => file.package === entry.id);
    entry.fileCount = owned.length;
    entry.indexedFileCount = owned.filter((file) => file.indexed).length;
    entry.lines = owned.reduce((total, file) => total + (file.lines ?? 0), 0);
    entry.symbols = owned.reduce((total, file) => total + file.symbols, 0);
  }

  const areaMap = new Map();
  for (const file of files) {
    const area = areaMap.get(file.area) ?? {
      id: `a:${file.area}`,
      name: file.area,
      fileCount: 0,
      indexedFileCount: 0,
      lines: 0,
      symbols: 0,
      packages: 0,
      dirtyFiles: 0,
    };
    area.fileCount += 1;
    area.indexedFileCount += file.indexed ? 1 : 0;
    area.lines += file.lines ?? 0;
    area.symbols += file.symbols;
    area.dirtyFiles += file.gitStatus !== "  " ? 1 : 0;
    areaMap.set(file.area, area);
  }
  for (const entry of packages) {
    const area = areaMap.get(entry.area);
    if (area) area.packages += 1;
  }
  const areas = [...areaMap.values()].sort((left, right) => right.fileCount - left.fileCount);

  const symbols = database.prepare("SELECT id, file_id, name, kind, line, end_line, sig FROM symbols ORDER BY id").all().map((symbol) => ({
    id: `s:${symbol.id}`,
    file: fileByGraphId.get(symbol.file_id)?.id ?? null,
    name: symbol.name,
    kind: symbol.kind,
    line: symbol.line,
    endLine: symbol.end_line,
    signature: symbol.sig,
  }));

  const symbolEdges = database.prepare(
    "SELECT id, file_id, sym_id, target_id, name, line, qual, kind, verdict, conf, argc FROM refs WHERE target_id IS NOT NULL ORDER BY id",
  ).all().map((edge) => ({
    id: `c:${edge.id}`,
    sourceFile: fileByGraphId.get(edge.file_id)?.id ?? null,
    source: edge.sym_id === null ? null : `s:${edge.sym_id}`,
    target: `s:${edge.target_id}`,
    name: edge.name,
    line: edge.line,
    qualifier: edge.qual,
    kind: edge.kind,
    verdict: edge.verdict,
    confidence: edge.conf,
    arguments: edge.argc,
  }));

  const imports = database.prepare(
    "SELECT id, file_id, name, module, line, system, target_file_id, origin FROM imports ORDER BY id",
  ).all().map((entry) => ({
    id: `i:${entry.id}`,
    source: fileByGraphId.get(entry.file_id)?.id ?? null,
    target: entry.target_file_id === null ? null : (fileByGraphId.get(entry.target_file_id)?.id ?? null),
    name: entry.name,
    module: entry.module,
    line: entry.line,
    system: Boolean(entry.system),
    origin: entry.origin,
  }));

  const routes = database.prepare(
    "SELECT id, file_id, framework, method, pattern, handler, line FROM routes ORDER BY framework, pattern, method",
  ).all().map((route) => ({
    id: `r:${route.id}`,
    file: fileByGraphId.get(route.file_id)?.id ?? null,
    framework: route.framework,
    method: route.method,
    pattern: route.pattern,
    handler: route.handler,
    line: route.line,
  }));

  const fileEdgeMap = new Map();
  function addFileEdge(source, target, kind, count = 1) {
    if (!source || !target) return;
    const key = `${source}\0${target}`;
    const edge = fileEdgeMap.get(key) ?? { source, target, calls: 0, imports: 0, total: 0 };
    edge[kind] += count;
    edge.total += count;
    fileEdgeMap.set(key, edge);
  }
  const symbolById = new Map(symbols.map((symbol) => [symbol.id, symbol]));
  for (const edge of symbolEdges) addFileEdge(edge.sourceFile, symbolById.get(edge.target)?.file, "calls");
  for (const entry of imports) if (entry.target) addFileEdge(entry.source, entry.target, "imports");
  const fileEdges = [...fileEdgeMap.values()].sort((left, right) => right.total - left.total);
  const fileById = new Map(files.map((file) => [file.id, file]));

  const packageEdges = aggregateEdges(
    fileEdges,
    (edge) => fileById.get(edge.source)?.package,
    (edge) => fileById.get(edge.target)?.package,
  );
  const areaEdges = aggregateEdges(
    fileEdges,
    (edge) => areaMap.get(fileById.get(edge.source)?.area)?.id,
    (edge) => areaMap.get(fileById.get(edge.target)?.area)?.id,
  );

  const platform = parsePlatformSpec(readFileSync(specPath, "utf8"));
  const taskLinks = [];
  for (const task of platform.tasks) {
    for (const path of task.touches) {
      const exact = fileByPath.get(path);
      if (exact) taskLinks.push({ task: task.id, file: exact.id, touch: path, match: "exact" });
    }
    for (const symbolName of task.symbols) {
      const exactSymbols = symbols.filter((symbol) => symbol.name === symbolName);
      for (const symbol of exactSymbols) taskLinks.push({ task: task.id, symbol: symbol.id, touch: symbolName, match: "symbol" });
    }
  }

  const dirty = files.filter((file) => file.gitStatus !== "  ");
  const referenceVerdicts = Object.fromEntries(
    database.prepare("SELECT coalesce(verdict, 'unresolved') verdict, count(*) count FROM refs GROUP BY verdict ORDER BY count DESC").all()
      .map((row) => [row.verdict, row.count]),
  );
  const languages = Object.values(files.reduce((result, file) => {
    const value = result[file.language] ?? { language: file.language, files: 0, indexedFiles: 0, lines: 0 };
    value.files += 1;
    value.indexedFiles += file.indexed ? 1 : 0;
    value.lines += file.lines ?? 0;
    result[file.language] = value;
    return result;
  }, {})).sort((left, right) => right.files - left.files);

  const graphStat = statSync(databasePath);
  const data = {
    schemaVersion: GENERATED_SCHEMA_VERSION,
    meta: {
      generatedAt: new Date().toISOString(),
      repository: relative(dirname(repositoryRoot), repositoryRoot),
      branch: git(["branch", "--show-current"]).trim() || "detached",
      commit: git(["rev-parse", "HEAD"]).trim(),
      graphDatabaseUpdatedAt: graphStat.mtime.toISOString(),
      codifySchemaVersion: metadata.schema_version,
      codifyIndexMilliseconds: Number(metadata.last_index_ms),
      relationshipPolicy: "Only Codify-resolved internal call targets and resolved imports become code edges.",
      packagePolicy: "Each file is grouped under its nearest ancestor manifest; cross-scope edges are aggregated from resolved file edges.",
      sources: [
        "git ls-files --stage",
        "git status --porcelain=v1",
        ".codegraph/graph.db schema 10",
        "spec/layerx-platform/spec.kvx",
      ],
    },
    coverage: {
      trackedFiles: tracked.length,
      graphFiles: graphFiles.length,
      unionFiles: files.length,
      indexedTrackedFiles: files.filter((file) => file.indexed && file.tracked).length,
      indexedUntrackedFiles: files.filter((file) => file.indexed && !file.tracked).length,
      unindexedTrackedFiles: files.filter((file) => !file.indexed && file.tracked).length,
      symbols: symbols.length,
      references: Number(database.prepare("SELECT count(*) count FROM refs").get().count),
      resolvedInternalReferences: symbolEdges.length,
      imports: imports.length,
      resolvedImports: imports.filter((entry) => entry.target).length,
      routes: routes.length,
      tasks: platform.tasks.filter((task) => task.deliverable).length,
      parentTaskGroups: platform.tasks.filter((task) => !task.deliverable).length,
      dirtyTrackedOrIndexedFiles: dirty.length,
      referenceVerdicts,
    },
    languages,
    areas,
    packages: packages.sort((left, right) => right.fileCount - left.fileCount || left.name.localeCompare(right.name)),
    files,
    symbols,
    imports,
    symbolEdges,
    fileEdges,
    packageEdges,
    areaEdges,
    routes,
    requirements: platform.requirements,
    tasks: platform.tasks,
    taskLinks,
  };

  mkdirSync(dirname(outputPath), { recursive: true });
  writeFileSync(outputPath, JSON.stringify(data));
  const megabytes = statSync(outputPath).size / (1024 * 1024);
  console.log(`Generated ${outputPath}`);
  console.log(`${files.length} files, ${symbols.length} symbols, ${fileEdges.length} file edges, ${symbolEdges.length} resolved symbol edges`);
  console.log(`${megabytes.toFixed(1)} MiB; Git ${data.meta.commit.slice(0, 12)} on ${data.meta.branch}`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) main();
