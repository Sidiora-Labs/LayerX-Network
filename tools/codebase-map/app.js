const VIEW_DEFINITIONS = [
  { id: "systems", icon: "◫", label: "Systems", kicker: "Architecture", title: "System relationships" },
  { id: "packages", icon: "◇", label: "Packages", kicker: "Manifest scopes", title: "Manifest-scope relationships" },
  { id: "files", icon: "▤", label: "Files", kicker: "Source tree", title: "File relationships" },
  { id: "symbols", icon: "ƒ", label: "Symbols", kicker: "Code graph", title: "Resolved symbol calls" },
  { id: "imports", icon: "⇢", label: "Imports", kicker: "Dependencies", title: "Import relationships" },
  { id: "routes", icon: "↗", label: "Routes", kicker: "Entry points", title: "Route to handler map" },
  { id: "tasks", icon: "✓", label: "Delivery", kicker: "Platform spec", title: "Task dependency graph" },
];

const COLORS = {
  area: "#42d8d0",
  package: "#668cff",
  file: "#8aa5c5",
  symbol: "#a779ff",
  module: "#efb45e",
  framework: "#6ed9a1",
  route: "#65c7f2",
  task: "#8695a8",
  done: "#6ed9a1",
  implemented: "#668cff",
  in_progress: "#efb45e",
  pending: "#5e6b7b",
};

const number = new Intl.NumberFormat("en", { notation: "compact", maximumFractionDigits: 1 });
const exactNumber = new Intl.NumberFormat("en");
const date = new Intl.DateTimeFormat("en", { dateStyle: "medium", timeStyle: "short" });

const elements = Object.fromEntries([
  "app", "loading", "snapshot-label", "dirty-indicator", "branch-label", "commit-label", "metric-strip",
  "view-nav", "search", "search-results", "inventory", "inventory-title", "inventory-count", "inventory-note",
  "breadcrumbs", "view-kicker", "view-title", "render-note", "back-button", "fit-button", "fit-toolbar-button",
  "help-button", "help-dialog", "graph", "canvas-shell", "graph-empty", "legend", "inspector",
  "inspector-empty", "inspector-content",
].map((id) => [id, document.getElementById(id)]));

let data;
let index;
const state = {
  view: "systems",
  scope: {},
  selected: null,
  query: "",
  history: [],
};

function formatBytes(value) {
  if (value === null || value === undefined) return "—";
  const units = ["B", "KB", "MB", "GB"];
  let amount = value;
  let unit = 0;
  while (amount >= 1024 && unit < units.length - 1) { amount /= 1024; unit += 1; }
  return `${amount < 10 && unit > 0 ? amount.toFixed(1) : Math.round(amount)} ${units[unit]}`;
}

function element(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined && text !== null) node.textContent = String(text);
  return node;
}

function button(className, text, handler) {
  const node = element("button", className, text);
  node.type = "button";
  node.addEventListener("click", handler);
  return node;
}

function truncate(value, maximum = 72) {
  if (!value) return "";
  return value.length > maximum ? `${value.slice(0, maximum - 1)}…` : value;
}

function buildIndex() {
  const byId = (items) => new Map(items.map((item) => [item.id, item]));
  const files = byId(data.files);
  const symbols = byId(data.symbols);
  const packages = byId(data.packages);
  const areas = byId(data.areas);
  const routes = byId(data.routes);
  const tasks = new Map(data.tasks.map((task) => [task.id, task]));
  const requirements = new Map(data.requirements.map((requirement) => [requirement.id, requirement]));
  const symbolsByFile = new Map();
  const routesByFile = new Map();
  const importsByFile = new Map();
  const taskLinksByFile = new Map();
  const taskLinksBySymbol = new Map();
  const fileEdgesOut = new Map();
  const fileEdgesIn = new Map();
  const symbolEdgesOut = new Map();
  const symbolEdgesIn = new Map();

  function add(map, key, value) {
    if (!key) return;
    const list = map.get(key) ?? [];
    list.push(value);
    map.set(key, list);
  }
  for (const symbol of data.symbols) add(symbolsByFile, symbol.file, symbol);
  for (const route of data.routes) add(routesByFile, route.file, route);
  for (const entry of data.imports) add(importsByFile, entry.source, entry);
  for (const link of data.taskLinks) {
    add(taskLinksByFile, link.file, link);
    add(taskLinksBySymbol, link.symbol, link);
  }
  for (const edge of data.fileEdges) {
    add(fileEdgesOut, edge.source, edge);
    add(fileEdgesIn, edge.target, edge);
  }
  for (const edge of data.symbolEdges) {
    add(symbolEdgesOut, edge.source, edge);
    add(symbolEdgesIn, edge.target, edge);
  }

  return {
    files, symbols, packages, areas, routes, tasks, requirements,
    symbolsByFile, routesByFile, importsByFile, taskLinksByFile, taskLinksBySymbol,
    fileEdgesOut, fileEdgesIn, symbolEdgesOut, symbolEdgesIn,
  };
}

function initializeHeader() {
  const dirty = data.coverage.dirtyTrackedOrIndexedFiles;
  elements["snapshot-label"].textContent = `Generated ${date.format(new Date(data.meta.generatedAt))}`;
  elements["branch-label"].textContent = data.meta.branch;
  elements["commit-label"].textContent = data.meta.commit.slice(0, 12);
  elements["dirty-indicator"].classList.toggle("dirty", dirty > 0);
  elements["dirty-indicator"].title = dirty ? `${dirty} tracked or indexed files differ from Git` : "Clean Git snapshot";

  const metrics = [
    [data.coverage.unionFiles, "Files visible", `${exactNumber.format(data.coverage.trackedFiles)} Git tracked`],
    [data.coverage.symbols, "Symbols", `${exactNumber.format(data.coverage.graphFiles)} files indexed`],
    [data.coverage.resolvedInternalReferences, "Resolved calls", `${number.format(data.coverage.references)} references observed`],
    [data.coverage.imports, "Imports", `${exactNumber.format(data.coverage.resolvedImports)} resolved to files`],
    [data.coverage.routes, "Routes", "Framework-detected entry points"],
    [data.coverage.tasks, "Delivery tasks", `${data.coverage.parentTaskGroups} parent groups`],
  ];
  elements["metric-strip"].replaceChildren(...metrics.map(([value, label, note]) => {
    const metric = element("div", "metric");
    metric.append(element("strong", "", exactNumber.format(value)), element("span", "", label), element("em", "", note));
    return metric;
  }));
}

function initializeNavigation() {
  elements["view-nav"].replaceChildren(...VIEW_DEFINITIONS.map((view) => {
    const control = button("view-button", "", () => navigate(view.id, {}, null));
    control.dataset.view = view.id;
    control.title = view.label;
    control.setAttribute("aria-label", view.label);
    control.append(element("span", "", view.icon), element("span", "", view.label));
    return control;
  }));
}

function saveHistory() {
  state.history.push({ view: state.view, scope: structuredClone(state.scope), selected: state.selected ? { ...state.selected } : null });
  if (state.history.length > 40) state.history.shift();
}

function navigate(view, scope = {}, selected = null, remember = true) {
  if (remember) saveHistory();
  state.view = view;
  state.scope = scope;
  state.selected = selected;
  render();
}

function goBack() {
  const previous = state.history.pop();
  if (!previous) return;
  state.view = previous.view;
  state.scope = previous.scope;
  state.selected = previous.selected;
  render();
}

function nodeForArea(area) {
  return { id: area.id, entityType: "area", entityId: area.id, label: area.name, value: area.fileCount, color: COLORS.area };
}

function nodeForPackage(entry) {
  return { id: entry.id, entityType: "package", entityId: entry.id, label: entry.name, value: entry.fileCount, color: COLORS.package };
}

function nodeForFile(file) {
  return {
    id: file.id, entityType: "file", entityId: file.id, label: file.name,
    secondary: file.path, value: Math.max(1, file.symbols), color: file.indexed ? COLORS.file : "#536172",
    ring: file.gitStatus !== "  " ? COLORS.in_progress : null,
  };
}

function nodeForSymbol(symbol) {
  return { id: symbol.id, entityType: "symbol", entityId: symbol.id, label: symbol.name, secondary: symbol.kind, value: 1, color: COLORS.symbol };
}

function nodeForTask(task) {
  return {
    id: `t:${task.id}`, entityType: "task", entityId: task.id, label: task.id,
    secondary: task.title, value: 1, color: COLORS[task.status] ?? COLORS.task,
  };
}

function rankAndLimit(items, score, selectedId, limit) {
  if (items.length <= limit) return items;
  const ranked = [...items].sort((left, right) => score(right) - score(left));
  const limited = ranked.slice(0, limit);
  if (selectedId && !limited.some((item) => item.id === selectedId)) {
    const selected = items.find((item) => item.id === selectedId);
    if (selected) limited[limited.length - 1] = selected;
  }
  return limited;
}

function graphSystems() {
  return {
    nodes: data.areas.map(nodeForArea),
    edges: data.areaEdges.map((edge) => ({ ...edge, kind: edge.imports && edge.calls ? "mixed" : edge.imports ? "import" : "call" })),
    total: data.areas.length,
  };
}

function graphPackages() {
  const areaName = state.scope.area ? index.areas.get(state.scope.area)?.name : null;
  let entries = data.packages.filter((entry) => entry.fileCount > 0 && (!areaName || entry.area === areaName));
  const degree = new Map();
  for (const edge of data.packageEdges) {
    degree.set(edge.source, (degree.get(edge.source) ?? 0) + edge.total);
    degree.set(edge.target, (degree.get(edge.target) ?? 0) + edge.total);
  }
  const total = entries.length;
  entries = rankAndLimit(entries, (entry) => (degree.get(entry.id) ?? 0) + entry.fileCount, state.selected?.id, 190);
  const ids = new Set(entries.map((entry) => entry.id));
  return {
    nodes: entries.map(nodeForPackage),
    edges: data.packageEdges.filter((edge) => ids.has(edge.source) && ids.has(edge.target)).map((edge) => ({ ...edge, kind: "mixed" })),
    total,
  };
}

function filesInScope() {
  if (state.scope.package) return data.files.filter((file) => file.package === state.scope.package);
  if (state.scope.area) {
    const area = index.areas.get(state.scope.area)?.name;
    return data.files.filter((file) => file.area === area);
  }
  return data.files;
}

function graphFiles() {
  let entries = filesInScope();
  const degree = new Map();
  for (const edge of data.fileEdges) {
    degree.set(edge.source, (degree.get(edge.source) ?? 0) + edge.total);
    degree.set(edge.target, (degree.get(edge.target) ?? 0) + edge.total);
  }
  if (state.selected?.type === "file") {
    const selected = state.selected.id;
    const neighbors = new Set([selected]);
    for (const edge of index.fileEdgesOut.get(selected) ?? []) neighbors.add(edge.target);
    for (const edge of index.fileEdgesIn.get(selected) ?? []) neighbors.add(edge.source);
    const scopedNeighbors = entries.filter((file) => neighbors.has(file.id));
    if (scopedNeighbors.length > 1) entries = scopedNeighbors;
  }
  const total = entries.length;
  entries = rankAndLimit(entries, (file) => (degree.get(file.id) ?? 0) + file.symbols, state.selected?.id, 220);
  const ids = new Set(entries.map((file) => file.id));
  return {
    nodes: entries.map(nodeForFile),
    edges: data.fileEdges.filter((edge) => ids.has(edge.source) && ids.has(edge.target)).map((edge) => ({ ...edge, kind: edge.imports && edge.calls ? "mixed" : edge.imports ? "import" : "call" })),
    total,
  };
}

function graphSymbols() {
  let entries = [];
  if (state.selected?.type === "symbol") {
    const selected = state.selected.id;
    const ids = new Set([selected]);
    for (const edge of index.symbolEdgesOut.get(selected) ?? []) ids.add(edge.target);
    for (const edge of index.symbolEdgesIn.get(selected) ?? []) if (edge.source) ids.add(edge.source);
    entries = [...ids].map((id) => index.symbols.get(id)).filter(Boolean);
  } else if (state.scope.file) {
    entries = index.symbolsByFile.get(state.scope.file) ?? [];
    const ids = new Set(entries.map((symbol) => symbol.id));
    for (const symbol of entries) {
      for (const edge of index.symbolEdgesOut.get(symbol.id) ?? []) ids.add(edge.target);
      for (const edge of index.symbolEdgesIn.get(symbol.id) ?? []) if (edge.source) ids.add(edge.source);
    }
    entries = [...ids].map((id) => index.symbols.get(id)).filter(Boolean);
  } else {
    const degree = new Map();
    for (const edge of data.symbolEdges) {
      if (edge.source) degree.set(edge.source, (degree.get(edge.source) ?? 0) + 1);
      degree.set(edge.target, (degree.get(edge.target) ?? 0) + 1);
    }
    entries = [...data.symbols].sort((left, right) => (degree.get(right.id) ?? 0) - (degree.get(left.id) ?? 0)).slice(0, 180);
  }
  const total = entries.length;
  entries = entries.slice(0, 240);
  const ids = new Set(entries.map((symbol) => symbol.id));
  return {
    nodes: entries.map(nodeForSymbol),
    edges: data.symbolEdges.filter((edge) => edge.source && ids.has(edge.source) && ids.has(edge.target)).map((edge) => ({ source: edge.source, target: edge.target, total: 1, kind: "call", raw: edge })),
    total,
  };
}

function graphImports() {
  if (state.scope.file) {
    const source = index.files.get(state.scope.file);
    const entries = index.importsByFile.get(state.scope.file) ?? [];
    const nodes = [nodeForFile(source)];
    const seen = new Set([source.id]);
    const edges = [];
    for (const entry of entries) {
      const id = entry.target ?? `m:${entry.module}`;
      if (!seen.has(id)) {
        nodes.push(entry.target ? nodeForFile(index.files.get(entry.target)) : {
          id, entityType: "module", entityId: entry.module, label: entry.module, value: 1, color: COLORS.module,
        });
        seen.add(id);
      }
      edges.push({ source: source.id, target: id, total: 1, imports: 1, kind: "import", raw: entry });
    }
    return { nodes, edges, total: entries.length };
  }

  const areaName = state.scope.area ? index.areas.get(state.scope.area)?.name : null;
  const aggregated = new Map();
  for (const entry of data.imports) {
    if (entry.target) continue;
    const sourceFile = index.files.get(entry.source);
    if (!sourceFile || (areaName && sourceFile.area !== areaName)) continue;
    const source = index.areas.get(`a:${sourceFile.area}`)?.id;
    const target = `m:${entry.module}`;
    const key = `${source}\0${target}`;
    const edge = aggregated.get(key) ?? { source, target, total: 0, imports: 0, kind: "import" };
    edge.total += 1;
    edge.imports += 1;
    aggregated.set(key, edge);
  }
  const edges = [...aggregated.values()].sort((left, right) => right.total - left.total).slice(0, 240);
  const areaIds = new Set(edges.map((edge) => edge.source));
  const moduleCounts = new Map();
  for (const edge of edges) moduleCounts.set(edge.target, (moduleCounts.get(edge.target) ?? 0) + edge.total);
  const nodes = [
    ...[...areaIds].map((id) => nodeForArea(index.areas.get(id))),
    ...[...moduleCounts].map(([id, count]) => ({ id, entityType: "module", entityId: id.slice(2), label: id.slice(2), value: count, color: COLORS.module })),
  ];
  return { nodes, edges, total: aggregated.size };
}

function graphRoutes() {
  const routes = state.scope.framework ? data.routes.filter((route) => route.framework === state.scope.framework) : data.routes;
  const frameworks = [...new Set(routes.map((route) => route.framework))];
  const nodes = frameworks.map((framework) => ({
    id: `fw:${framework}`, entityType: "framework", entityId: framework, label: framework,
    value: routes.filter((route) => route.framework === framework).length, color: COLORS.framework,
  }));
  const edges = [];
  for (const route of routes) {
    nodes.push({
      id: route.id, entityType: "route", entityId: route.id,
      label: `${route.method ?? "*"} ${route.pattern}`, secondary: route.framework, value: 1, color: COLORS.route,
    });
    edges.push({ source: `fw:${route.framework}`, target: route.id, total: 1, kind: "route" });
  }
  return { nodes, edges, total: routes.length };
}

function graphTasks() {
  const tasks = data.tasks.filter((task) => task.deliverable && (!state.scope.wave || task.wave === state.scope.wave));
  const ids = new Set(tasks.map((task) => task.id));
  const wavePositions = new Map();
  const waves = [...new Set(tasks.map((task) => task.wave))].sort((left, right) => left - right);
  const waveCenter = (waves.length - 1) / 2;
  const taskNodes = tasks.map((task) => {
    const peers = tasks.filter((candidate) => candidate.wave === task.wave);
    const position = wavePositions.get(task.wave) ?? 0;
    wavePositions.set(task.wave, position + 1);
    return {
      ...nodeForTask(task),
      initialX: (waves.indexOf(task.wave) - waveCenter) * 130,
      initialY: (position - (peers.length - 1) / 2) * 34,
    };
  });
  const edges = tasks.flatMap((task) => task.requires.filter((required) => ids.has(required)).map((required) => ({
    source: `t:${required}`, target: `t:${task.id}`, total: 1, kind: "requires",
  })));
  return { nodes: taskNodes, edges, total: tasks.length };
}

function buildGraph() {
  if (state.view === "systems") return graphSystems();
  if (state.view === "packages") return graphPackages();
  if (state.view === "files") return graphFiles();
  if (state.view === "symbols") return graphSymbols();
  if (state.view === "imports") return graphImports();
  if (state.view === "routes") return graphRoutes();
  return graphTasks();
}

class CanvasGraph {
  constructor(canvas) {
    this.canvas = canvas;
    this.context = canvas.getContext("2d");
    this.nodes = [];
    this.edges = [];
    this.camera = { x: 0, y: 0, scale: 1 };
    this.pointer = null;
    this.hovered = null;
    this.dragged = null;
    this.frame = null;
    this.steps = 0;
    this.resizeObserver = new ResizeObserver(() => this.resize());
    this.resizeObserver.observe(canvas.parentElement);
    this.attachEvents();
  }

  resize() {
    const bounds = this.canvas.getBoundingClientRect();
    const ratio = window.devicePixelRatio || 1;
    this.canvas.width = Math.max(1, Math.round(bounds.width * ratio));
    this.canvas.height = Math.max(1, Math.round(bounds.height * ratio));
    this.draw();
  }

  seed(id) {
    let hash = 2166136261;
    for (let index = 0; index < id.length; index += 1) hash = Math.imul(hash ^ id.charCodeAt(index), 16777619);
    return (hash >>> 0) / 4294967295;
  }

  setGraph(nodes, edges, selectedId) {
    const prior = new Map(this.nodes.map((node) => [node.id, node]));
    this.nodes = nodes.map((node, index) => {
      const previous = prior.get(node.id);
      const angle = this.seed(node.id) * Math.PI * 2;
      const radius = 60 + Math.sqrt(index + 1) * 28;
      return {
        ...node,
        x: previous?.x ?? node.initialX ?? Math.cos(angle) * radius,
        y: previous?.y ?? node.initialY ?? Math.sin(angle) * radius,
        vx: 0,
        vy: 0,
        radius: Math.max(5, Math.min(17, 6 + Math.log2((node.value ?? 1) + 1) * 1.7)),
        selected: node.id === selectedId,
      };
    });
    this.nodeById = new Map(this.nodes.map((node) => [node.id, node]));
    this.edges = edges.filter((edge) => this.nodeById.has(edge.source) && this.nodeById.has(edge.target));
    this.steps = this.nodes.length > 1 ? 110 : 0;
    this.start();
    requestAnimationFrame(() => this.fit());
  }

  start() {
    if (this.frame) return;
    const tick = () => {
      this.frame = null;
      if (this.steps > 0) {
        this.simulate();
        this.steps -= 1;
        this.start();
      }
      this.draw();
    };
    this.frame = requestAnimationFrame(tick);
  }

  simulate() {
    const count = this.nodes.length;
    const repulsion = count > 170 ? 750 : count > 70 ? 1100 : 1650;
    for (let left = 0; left < count; left += 1) {
      const a = this.nodes[left];
      for (let right = left + 1; right < count; right += 1) {
        const b = this.nodes[right];
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        const distanceSquared = Math.max(36, dx * dx + dy * dy);
        const force = repulsion / distanceSquared;
        const distance = Math.sqrt(distanceSquared);
        dx /= distance;
        dy /= distance;
        a.vx -= dx * force;
        a.vy -= dy * force;
        b.vx += dx * force;
        b.vy += dy * force;
      }
    }
    for (const edge of this.edges) {
      const source = this.nodeById.get(edge.source);
      const target = this.nodeById.get(edge.target);
      const dx = target.x - source.x;
      const dy = target.y - source.y;
      const distance = Math.max(1, Math.hypot(dx, dy));
      const desired = 65 + Math.min(55, Math.log2((edge.total ?? 1) + 1) * 10);
      const force = (distance - desired) * 0.0035;
      source.vx += (dx / distance) * force;
      source.vy += (dy / distance) * force;
      target.vx -= (dx / distance) * force;
      target.vy -= (dy / distance) * force;
    }
    for (const node of this.nodes) {
      if (node === this.dragged) continue;
      node.vx += -node.x * 0.0012;
      node.vy += -node.y * 0.0012;
      node.vx *= 0.82;
      node.vy *= 0.82;
      node.x += node.vx;
      node.y += node.vy;
    }
  }

  screen(node) {
    const bounds = this.canvas.getBoundingClientRect();
    return {
      x: bounds.width / 2 + this.camera.x + node.x * this.camera.scale,
      y: bounds.height / 2 + this.camera.y + node.y * this.camera.scale,
    };
  }

  world(x, y) {
    const bounds = this.canvas.getBoundingClientRect();
    return {
      x: (x - bounds.width / 2 - this.camera.x) / this.camera.scale,
      y: (y - bounds.height / 2 - this.camera.y) / this.camera.scale,
    };
  }

  draw() {
    const context = this.context;
    const ratio = window.devicePixelRatio || 1;
    const bounds = this.canvas.getBoundingClientRect();
    context.setTransform(ratio, 0, 0, ratio, 0, 0);
    context.clearRect(0, 0, bounds.width, bounds.height);
    this.drawGrid(context, bounds);

    for (const edge of this.edges) {
      const sourceNode = this.nodeById.get(edge.source);
      const targetNode = this.nodeById.get(edge.target);
      const source = this.screen(sourceNode);
      const target = this.screen(targetNode);
      const highlighted = sourceNode.selected || targetNode.selected || sourceNode === this.hovered || targetNode === this.hovered;
      context.beginPath();
      context.moveTo(source.x, source.y);
      context.lineTo(target.x, target.y);
      context.strokeStyle = highlighted ? "rgba(102, 183, 255, 0.54)" : "rgba(123, 151, 180, 0.11)";
      context.lineWidth = highlighted ? 1.2 : Math.min(1.4, 0.45 + Math.log2((edge.total ?? 1) + 1) * 0.15);
      context.stroke();
      if (highlighted && Math.hypot(target.x - source.x, target.y - source.y) > 34) this.drawArrow(context, source, target, targetNode.radius * this.camera.scale + 4);
    }

    const labelThreshold = this.nodes.length < 45 ? 0 : this.nodes.length < 120 ? 0.75 : 1.15;
    for (const node of this.nodes) {
      const point = this.screen(node);
      const radius = Math.max(3.5, node.radius * Math.min(1.35, this.camera.scale));
      if (point.x < -50 || point.y < -50 || point.x > bounds.width + 50 || point.y > bounds.height + 50) continue;
      if (node.selected || node === this.hovered) {
        context.beginPath();
        context.arc(point.x, point.y, radius + 7, 0, Math.PI * 2);
        context.fillStyle = `${node.color}1f`;
        context.fill();
        context.strokeStyle = `${node.color}99`;
        context.lineWidth = 1;
        context.stroke();
      }
      context.beginPath();
      context.arc(point.x, point.y, radius, 0, Math.PI * 2);
      context.fillStyle = node.color;
      context.shadowColor = node.color;
      context.shadowBlur = node.selected ? 15 : 4;
      context.fill();
      context.shadowBlur = 0;
      if (node.ring) {
        context.strokeStyle = node.ring;
        context.lineWidth = 1.5;
        context.stroke();
      }
      if (this.camera.scale >= labelThreshold || node.selected || node === this.hovered) {
        context.font = `${node.selected ? 600 : 500} 10px Inter, system-ui, sans-serif`;
        context.textAlign = "center";
        context.textBaseline = "top";
        context.fillStyle = node.selected || node === this.hovered ? "#f2f6fa" : "rgba(202, 216, 231, 0.76)";
        context.fillText(truncate(node.label, 30), point.x, point.y + radius + 6);
      }
    }
  }

  drawGrid(context, bounds) {
    const gap = 42 * this.camera.scale;
    if (gap < 15) return;
    const offsetX = (bounds.width / 2 + this.camera.x) % gap;
    const offsetY = (bounds.height / 2 + this.camera.y) % gap;
    context.beginPath();
    for (let x = offsetX; x < bounds.width; x += gap) { context.moveTo(x, 0); context.lineTo(x, bounds.height); }
    for (let y = offsetY; y < bounds.height; y += gap) { context.moveTo(0, y); context.lineTo(bounds.width, y); }
    context.strokeStyle = "rgba(154, 180, 207, 0.025)";
    context.lineWidth = 1;
    context.stroke();
  }

  drawArrow(context, source, target, offset) {
    const angle = Math.atan2(target.y - source.y, target.x - source.x);
    const x = target.x - Math.cos(angle) * offset;
    const y = target.y - Math.sin(angle) * offset;
    context.beginPath();
    context.moveTo(x, y);
    context.lineTo(x - Math.cos(angle - 0.55) * 5, y - Math.sin(angle - 0.55) * 5);
    context.lineTo(x - Math.cos(angle + 0.55) * 5, y - Math.sin(angle + 0.55) * 5);
    context.closePath();
    context.fillStyle = "rgba(102, 183, 255, 0.64)";
    context.fill();
  }

  hitTest(x, y) {
    for (let index = this.nodes.length - 1; index >= 0; index -= 1) {
      const node = this.nodes[index];
      const point = this.screen(node);
      if (Math.hypot(x - point.x, y - point.y) <= Math.max(8, node.radius * this.camera.scale + 4)) return node;
    }
    return null;
  }

  attachEvents() {
    this.canvas.addEventListener("pointerdown", (event) => {
      this.canvas.setPointerCapture(event.pointerId);
      const bounds = this.canvas.getBoundingClientRect();
      const x = event.clientX - bounds.left;
      const y = event.clientY - bounds.top;
      const node = this.hitTest(x, y);
      this.pointer = { x, y, cameraX: this.camera.x, cameraY: this.camera.y, moved: false };
      this.dragged = node;
      if (node) {
        const world = this.world(x, y);
        this.pointer.offsetX = node.x - world.x;
        this.pointer.offsetY = node.y - world.y;
      }
    });
    this.canvas.addEventListener("pointermove", (event) => {
      const bounds = this.canvas.getBoundingClientRect();
      const x = event.clientX - bounds.left;
      const y = event.clientY - bounds.top;
      if (this.pointer) {
        const dx = x - this.pointer.x;
        const dy = y - this.pointer.y;
        if (Math.hypot(dx, dy) > 3) this.pointer.moved = true;
        if (this.dragged) {
          const world = this.world(x, y);
          this.dragged.x = world.x + this.pointer.offsetX;
          this.dragged.y = world.y + this.pointer.offsetY;
          this.dragged.vx = 0;
          this.dragged.vy = 0;
          this.steps = Math.max(this.steps, 24);
          this.start();
        } else {
          this.camera.x = this.pointer.cameraX + dx;
          this.camera.y = this.pointer.cameraY + dy;
          this.draw();
        }
      } else {
        this.hovered = this.hitTest(x, y);
        this.canvas.classList.toggle("node-hover", Boolean(this.hovered));
        this.draw();
      }
    });
    this.canvas.addEventListener("pointerup", () => {
      if (this.dragged && !this.pointer?.moved) selectNode(this.dragged);
      this.pointer = null;
      this.dragged = null;
    });
    this.canvas.addEventListener("pointerleave", () => {
      if (!this.pointer) { this.hovered = null; this.draw(); }
    });
    this.canvas.addEventListener("dblclick", (event) => {
      const bounds = this.canvas.getBoundingClientRect();
      const node = this.hitTest(event.clientX - bounds.left, event.clientY - bounds.top);
      if (node) drillNode(node);
    });
    this.canvas.addEventListener("wheel", (event) => {
      event.preventDefault();
      const bounds = this.canvas.getBoundingClientRect();
      const x = event.clientX - bounds.left;
      const y = event.clientY - bounds.top;
      const before = this.world(x, y);
      this.camera.scale = Math.max(0.12, Math.min(5, this.camera.scale * Math.exp(-event.deltaY * 0.0012)));
      const after = this.world(x, y);
      this.camera.x += (after.x - before.x) * this.camera.scale;
      this.camera.y += (after.y - before.y) * this.camera.scale;
      this.draw();
    }, { passive: false });
  }

  fit() {
    if (!this.nodes.length) return;
    const bounds = this.canvas.getBoundingClientRect();
    const xs = this.nodes.map((node) => node.x);
    const ys = this.nodes.map((node) => node.y);
    const minX = Math.min(...xs) - 35;
    const maxX = Math.max(...xs) + 35;
    const minY = Math.min(...ys) - 35;
    const maxY = Math.max(...ys) + 35;
    this.camera.scale = Math.max(0.12, Math.min(2, Math.min(bounds.width / Math.max(100, maxX - minX), bounds.height / Math.max(100, maxY - minY)) * 0.84));
    this.camera.x = -((minX + maxX) / 2) * this.camera.scale;
    this.camera.y = -((minY + maxY) / 2) * this.camera.scale;
    this.draw();
  }
}

const graph = new CanvasGraph(elements.graph);

function selectNode(node) {
  state.selected = { type: node.entityType, id: node.entityId };
  render(false);
}

function drillNode(node) {
  if (node.entityType === "area") navigate("packages", { area: node.entityId }, { type: "area", id: node.entityId });
  else if (node.entityType === "package") {
    const entry = index.packages.get(node.entityId);
    navigate("files", { area: `a:${entry.area}`, package: entry.id }, { type: "package", id: entry.id });
  } else if (node.entityType === "file") navigate("symbols", { file: node.entityId }, { type: "file", id: node.entityId });
  else if (node.entityType === "framework") navigate("routes", { framework: node.entityId }, { type: "framework", id: node.entityId });
  else if (node.entityType === "route") {
    const route = index.routes.get(node.entityId);
    if (route?.file) navigate("files", {}, { type: "file", id: route.file });
  } else selectNode(node);
}

function inventoryForView() {
  if (state.view === "systems") return data.areas.map((item) => ({ type: "area", id: item.id, title: item.name, subtitle: `${exactNumber.format(item.indexedFileCount)} indexed`, value: `${number.format(item.fileCount)} files`, icon: "◫" }));
  if (state.view === "packages") {
    const area = state.scope.area ? index.areas.get(state.scope.area)?.name : null;
    return data.packages.filter((item) => item.fileCount && (!area || item.area === area)).map((item) => ({ type: "package", id: item.id, title: item.name, subtitle: item.directory || "repository root", value: `${number.format(item.fileCount)} files`, icon: "◇" }));
  }
  if (state.view === "files") return filesInScope().map((item) => ({ type: "file", id: item.id, title: item.name, subtitle: item.path, value: item.indexed ? `${number.format(item.symbols)} sym` : "tree only", icon: item.gitStatus !== "  " ? "●" : "▤" }));
  if (state.view === "symbols") {
    const items = state.scope.file ? (index.symbolsByFile.get(state.scope.file) ?? []) : data.symbols;
    return items.map((item) => ({ type: "symbol", id: item.id, title: item.name, subtitle: `${item.kind ?? "symbol"} · ${index.files.get(item.file)?.path ?? "unknown"}:${item.line ?? "?"}`, value: "ƒ", icon: "ƒ" }));
  }
  if (state.view === "imports") {
    if (state.scope.file) return (index.importsByFile.get(state.scope.file) ?? []).map((item) => ({ type: item.target ? "file" : "module", id: item.target ?? item.module, title: item.module, subtitle: item.name ?? "import", value: item.line ? `L${item.line}` : "", icon: "⇢" }));
    const counts = new Map();
    for (const entry of data.imports) if (!entry.target) counts.set(entry.module, (counts.get(entry.module) ?? 0) + 1);
    return [...counts].sort((left, right) => right[1] - left[1]).map(([module, count]) => ({ type: "module", id: module, title: module, subtitle: "external or unresolved module", value: exactNumber.format(count), icon: "⇢" }));
  }
  if (state.view === "routes") return data.routes.filter((item) => !state.scope.framework || item.framework === state.scope.framework).map((item) => ({ type: "route", id: item.id, title: `${item.method ?? "*"} ${item.pattern}`, subtitle: `${item.framework} · ${index.files.get(item.file)?.path ?? "unknown"}`, value: item.line ? `L${item.line}` : "", icon: "↗" }));
  return data.tasks.filter((item) => item.deliverable && (!state.scope.wave || item.wave === state.scope.wave)).map((item) => ({ type: "task", id: item.id, title: `${item.id} · ${item.title}`, subtitle: `Wave ${item.wave} · ${item.status}`, value: item.requires.length ? `${item.requires.length} dep` : "root", icon: "✓" }));
}

function selectedMatches(item) {
  return state.selected?.type === item.type && state.selected?.id === item.id;
}

function chooseInventory(item) {
  state.selected = { type: item.type, id: item.id };
  if (item.type === "file" && state.view === "imports") state.scope = { file: item.id };
  render();
}

function renderInventory() {
  const definition = VIEW_DEFINITIONS.find((view) => view.id === state.view);
  const items = inventoryForView();
  const maximum = 450;
  elements["inventory-title"].textContent = definition.label;
  elements["inventory-count"].textContent = exactNumber.format(items.length);
  elements.inventory.replaceChildren(...items.slice(0, maximum).map((item) => {
    const control = button(`inventory-item${selectedMatches(item) ? " selected" : ""}`, "", () => chooseInventory(item));
    const icon = element("span", "item-icon", item.icon);
    const copy = element("span");
    copy.append(element("strong", "", item.title), element("small", "", item.subtitle));
    control.append(icon, copy, element("span", "item-value", item.value));
    control.setAttribute("role", "listitem");
    return control;
  }));
  elements["inventory-note"].textContent = items.length > maximum
    ? `Showing ${maximum} of ${exactNumber.format(items.length)} · search reaches all`
    : "Select once to inspect · double-click a graph node to drill in";
}

function renderBreadcrumbs() {
  const crumbs = [{ label: "All", handler: () => navigate(state.view, {}, null) }];
  if (state.scope.area) {
    const area = index.areas.get(state.scope.area);
    crumbs.push({ label: area?.name ?? state.scope.area, handler: () => navigate(state.view, { area: state.scope.area }, { type: "area", id: state.scope.area }) });
  }
  if (state.scope.package) {
    const entry = index.packages.get(state.scope.package);
    crumbs.push({ label: entry?.name ?? state.scope.package, handler: () => navigate(state.view, { ...state.scope }, { type: "package", id: state.scope.package }) });
  }
  if (state.scope.file) {
    const file = index.files.get(state.scope.file);
    crumbs.push({ label: file?.name ?? state.scope.file, handler: () => { state.selected = { type: "file", id: state.scope.file }; render(false); } });
  }
  if (state.scope.framework) crumbs.push({ label: state.scope.framework, handler: () => {} });
  if (state.scope.wave) crumbs.push({ label: `Wave ${state.scope.wave}`, handler: () => {} });
  elements.breadcrumbs.replaceChildren(...crumbs.map((crumb) => button("crumb", crumb.label, crumb.handler)));
}

function searchEverything(query) {
  const needle = query.trim().toLowerCase();
  if (needle.length < 2) return [];
  const matches = [];
  function consider(type, id, title, subtitle, keywords = "") {
    const haystack = `${title} ${subtitle} ${keywords}`.toLowerCase();
    const position = haystack.indexOf(needle);
    if (position === -1) return;
    let score = position === 0 ? 100 : 30;
    if (title.toLowerCase() === needle) score += 200;
    if (title.toLowerCase().includes(needle)) score += 50;
    matches.push({ type, id, title, subtitle, score });
  }
  for (const area of data.areas) consider("area", area.id, area.name, `${area.fileCount} files`);
  for (const entry of data.packages) consider("package", entry.id, entry.name, entry.directory, entry.manifest);
  for (const file of data.files) consider("file", file.id, file.name, file.path, file.language);
  for (const symbol of data.symbols) consider("symbol", symbol.id, symbol.name, `${symbol.kind ?? "symbol"} · ${index.files.get(symbol.file)?.path ?? ""}`, symbol.signature ?? "");
  for (const route of data.routes) consider("route", route.id, `${route.method ?? "*"} ${route.pattern}`, `${route.framework} · ${index.files.get(route.file)?.path ?? ""}`);
  for (const task of data.tasks) if (task.deliverable) consider("task", task.id, `${task.id} · ${task.title}`, `Wave ${task.wave} · ${task.status}`, `${task.symbols.join(" ")} ${task.touches.join(" ")}`);
  return matches.sort((left, right) => right.score - left.score || left.title.localeCompare(right.title)).slice(0, 120);
}

function openSearchResult(result) {
  state.query = "";
  elements.search.value = "";
  if (result.type === "area") navigate("packages", { area: result.id }, { type: "area", id: result.id });
  else if (result.type === "package") {
    const entry = index.packages.get(result.id);
    navigate("files", { area: `a:${entry.area}`, package: entry.id }, { type: "package", id: entry.id });
  } else if (result.type === "file") {
    const file = index.files.get(result.id);
    navigate("files", file.package ? { area: `a:${file.area}`, package: file.package } : { area: `a:${file.area}` }, { type: "file", id: file.id });
  } else if (result.type === "symbol") {
    const symbol = index.symbols.get(result.id);
    navigate("symbols", { file: symbol.file }, { type: "symbol", id: symbol.id });
  } else if (result.type === "route") navigate("routes", {}, { type: "route", id: result.id });
  else if (result.type === "task") navigate("tasks", {}, { type: "task", id: result.id });
}

function renderSearch() {
  const results = searchEverything(state.query);
  elements["search-results"].hidden = state.query.trim().length < 2;
  if (elements["search-results"].hidden) return;
  if (!results.length) {
    elements["search-results"].replaceChildren(element("div", "inspector-empty", "No exact source-backed matches"));
    return;
  }
  elements["search-results"].replaceChildren(...results.map((result) => {
    const control = button("inventory-item", "", () => openSearchResult(result));
    const icon = { area: "◫", package: "◇", file: "▤", symbol: "ƒ", route: "↗", task: "✓" }[result.type];
    const copy = element("span");
    copy.append(element("strong", "", result.title), element("small", "", result.subtitle));
    control.append(element("span", "item-icon", icon), copy, element("span", "item-value", result.type));
    return control;
  }));
}

function facts(values) {
  const grid = element("div", "fact-grid");
  for (const [label, value] of values) {
    const fact = element("div", "fact");
    fact.append(element("span", "", label), element("strong", "", value ?? "—"));
    grid.append(fact);
  }
  return grid;
}

function section(title, content) {
  const wrapper = element("section", "inspector-section");
  wrapper.append(element("h3", "", title), content);
  return wrapper;
}

function relationList(items, mapper) {
  const list = element("div", "relation-list");
  if (!items.length) {
    list.append(element("span", "inspector-path", "None recorded in this source graph."));
    return list;
  }
  for (const item of items.slice(0, 14)) {
    const mapped = mapper(item);
    const control = button("relation", "", mapped.handler ?? (() => {}));
    control.append(element("strong", "", mapped.title), element("span", "", mapped.value ?? ""));
    list.append(control);
  }
  if (items.length > 14) list.append(element("span", "inspector-path", `+ ${items.length - 14} more in the selected graph scope`));
  return list;
}

function inspectorHeader(type, title, path, actions = []) {
  const header = element("header", "inspector-header");
  header.append(element("span", "inspector-type", type), element("h2", "", title));
  if (path) header.append(element("p", "inspector-path", path));
  if (actions.length) {
    const controls = element("div", "inspector-actions");
    for (const action of actions) controls.append(button("text-button", action.label, action.handler));
    header.append(controls);
  }
  return header;
}

function inspectArea(id) {
  const area = index.areas.get(id);
  const packages = data.packages.filter((entry) => entry.area === area.name && entry.fileCount);
  return [
    inspectorHeader("System area", area.name, `${area.fileCount} files in the Git/Codify union`, [
      { label: "Open packages", handler: () => navigate("packages", { area: area.id }, { type: "area", id: area.id }) },
      { label: "Open files", handler: () => navigate("files", { area: area.id }, { type: "area", id: area.id }) },
    ]),
    section("Coverage", facts([
      ["Files", exactNumber.format(area.fileCount)], ["Indexed", exactNumber.format(area.indexedFileCount)],
      ["Symbols", exactNumber.format(area.symbols)], ["Indexed lines", exactNumber.format(area.lines)],
      ["Packages", exactNumber.format(area.packages)], ["Dirty files", exactNumber.format(area.dirtyFiles)],
    ])),
    section("Packages", relationList(packages, (entry) => ({ title: entry.name, value: `${entry.fileCount} files`, handler: () => navigate("files", { area: area.id, package: entry.id }, { type: "package", id: entry.id }) }))),
  ];
}

function inspectPackage(id) {
  const entry = index.packages.get(id);
  const files = data.files.filter((file) => file.package === id).sort((left, right) => right.symbols - left.symbols);
  return [
    inspectorHeader("Nearest manifest scope", entry.name, entry.directory || "repository root", [
      { label: "Open files", handler: () => navigate("files", { area: `a:${entry.area}`, package: entry.id }, { type: "package", id: entry.id }) },
    ]),
    section("Manifest evidence", facts([
      ["Manifest", entry.manifest], ["Ecosystem", entry.ecosystem], ["Files", exactNumber.format(entry.fileCount)],
      ["Indexed", exactNumber.format(entry.indexedFileCount)], ["Symbols", exactNumber.format(entry.symbols)], ["Lines", exactNumber.format(entry.lines)],
    ])),
    section("Highest signal files", relationList(files, (file) => ({ title: file.name, value: `${file.symbols} sym`, handler: () => navigate("files", { area: `a:${file.area}`, package: id }, { type: "file", id: file.id }) }))),
  ];
}

function inspectFile(id) {
  const file = index.files.get(id);
  const outgoing = index.fileEdgesOut.get(id) ?? [];
  const incoming = index.fileEdgesIn.get(id) ?? [];
  const imports = index.importsByFile.get(id) ?? [];
  const routes = index.routesByFile.get(id) ?? [];
  const links = index.taskLinksByFile.get(id) ?? [];
  const tasks = links.map((link) => index.tasks.get(link.task)).filter(Boolean);
  return [
    inspectorHeader("File", file.name, file.path, [
      { label: "Symbols", handler: () => navigate("symbols", { file: id }, { type: "file", id }) },
      { label: "Imports", handler: () => navigate("imports", { file: id }, { type: "file", id }) },
      { label: "Copy path", handler: () => navigator.clipboard?.writeText(file.path) },
    ]),
    section("Source evidence", facts([
      ["Language", file.language], ["Git state", file.gitStatus.trim() || "clean"],
      ["Indexed", file.indexed ? "yes" : "no"], ["Tracked", file.tracked ? "yes" : "no"],
      ["Lines", file.lines === null ? "not indexed" : exactNumber.format(file.lines)], ["Working size", formatBytes(file.workingSize)],
      ["Symbols", exactNumber.format(file.symbols)], ["Git churn", exactNumber.format(file.churn)],
    ])),
    section("Resolved relationships", facts([
      ["Outgoing files", exactNumber.format(outgoing.length)], ["Incoming files", exactNumber.format(incoming.length)],
      ["Imports", exactNumber.format(imports.length)], ["Routes", exactNumber.format(routes.length)],
      ["References", file.references ? exactNumber.format(file.references.total) : "not indexed"], ["Internal refs", file.references ? exactNumber.format(file.references.internal) : "not indexed"],
    ])),
    section("Calls and imports out", relationList(outgoing.sort((left, right) => right.total - left.total), (edge) => {
      const target = index.files.get(edge.target);
      return { title: target?.path ?? edge.target, value: `${edge.total} edges`, handler: () => navigate("files", {}, { type: "file", id: edge.target }) };
    })),
    ...(tasks.length ? [section("Delivery links", relationList(tasks, (task) => ({ title: `${task.id} · ${task.title}`, value: task.status, handler: () => navigate("tasks", {}, { type: "task", id: task.id }) })))] : []),
    ...(routes.length ? [section("Detected routes", relationList(routes, (route) => ({ title: `${route.method ?? "*"} ${route.pattern}`, value: route.framework, handler: () => navigate("routes", {}, { type: "route", id: route.id }) })))] : []),
  ];
}

function inspectSymbol(id) {
  const symbol = index.symbols.get(id);
  const file = index.files.get(symbol.file);
  const outgoing = index.symbolEdgesOut.get(id) ?? [];
  const incoming = index.symbolEdgesIn.get(id) ?? [];
  const signature = element("pre", "signature", symbol.signature || "No signature captured by the index.");
  return [
    inspectorHeader("Symbol", symbol.name, `${file?.path ?? "unknown"}:${symbol.line ?? "?"}`, [
      { label: "Open file", handler: () => navigate("files", {}, { type: "file", id: symbol.file }) },
      { label: "Call neighborhood", handler: () => navigate("symbols", { file: symbol.file }, { type: "symbol", id }) },
    ]),
    section("Definition", facts([
      ["Kind", symbol.kind ?? "unknown"], ["Start line", symbol.line ?? "—"],
      ["End line", symbol.endLine ?? "—"], ["File indexed", file?.indexed ? "yes" : "no"],
      ["Outgoing calls", exactNumber.format(outgoing.length)], ["Incoming calls", exactNumber.format(incoming.length)],
    ])),
    section("Indexed signature", signature),
    section("Calls out", relationList(outgoing, (edge) => {
      const target = index.symbols.get(edge.target);
      return { title: target?.name ?? edge.name, value: `L${edge.line ?? "?"}`, handler: () => navigate("symbols", { file: target?.file }, { type: "symbol", id: edge.target }) };
    })),
    section("Called by", relationList(incoming, (edge) => {
      const source = edge.source ? index.symbols.get(edge.source) : null;
      return { title: source?.name ?? index.files.get(edge.sourceFile)?.name ?? "file-level reference", value: edge.confidence ?? "resolved", handler: () => source && navigate("symbols", { file: source.file }, { type: "symbol", id: source.id }) };
    })),
  ];
}

function inspectTask(id) {
  const task = index.tasks.get(id);
  const requirements = task.reqs.map((requirement) => index.requirements.get(requirement.split(".")[0])).filter(Boolean);
  const dependent = data.tasks.filter((candidate) => candidate.requires.includes(id));
  const status = element("span", `status-chip ${task.status}`, task.status.replaceAll("_", " "));
  return [
    inspectorHeader("Delivery task", `${task.id} · ${task.title}`, `Wave ${task.wave ?? "group"}`, [
      ...(task.touches.length ? [{ label: "Find touched files", handler: () => { elements.search.value = task.touches[0]; state.query = task.touches[0]; renderSearch(); elements.search.focus(); } }] : []),
    ]),
    section("Current state", status),
    section("Qualification contract", facts([
      ["Wave", task.wave ?? "parent"], ["Prerequisites", task.requires.length],
      ["Requirement clauses", task.reqs.length], ["Declared symbols", task.symbols.length],
      ["Touch patterns", task.touches.length], ["Verify command", task.verifyCommand ?? "none"],
    ])),
    section("Requires", relationList(task.requires.map((required) => index.tasks.get(required)).filter(Boolean), (required) => ({ title: `${required.id} · ${required.title}`, value: required.status, handler: () => navigate("tasks", {}, { type: "task", id: required.id }) }))),
    section("Unlocks", relationList(dependent, (candidate) => ({ title: `${candidate.id} · ${candidate.title}`, value: candidate.status, handler: () => navigate("tasks", {}, { type: "task", id: candidate.id }) }))),
    ...(requirements.length ? [section("Requirements", relationList(requirements, (requirement) => ({ title: `${requirement.id} · ${requirement.title}`, value: "spec" })))] : []),
    ...(task.touches.length ? [section("Declared paths", relationList(task.touches, (path) => ({ title: path, value: "touch", handler: () => { elements.search.value = path; state.query = path; renderSearch(); } })))] : []),
  ];
}

function inspectRoute(id) {
  const route = index.routes.get(id);
  const file = index.files.get(route.file);
  return [
    inspectorHeader("Detected route", `${route.method ?? "*"} ${route.pattern}`, `${file?.path ?? "unknown"}:${route.line ?? "?"}`, [
      ...(file ? [{ label: "Open file", handler: () => navigate("files", {}, { type: "file", id: file.id }) }] : []),
    ]),
    section("Route evidence", facts([
      ["Framework", route.framework], ["Method", route.method ?? "*"],
      ["Pattern", route.pattern], ["Handler", route.handler ?? "not resolved"],
      ["Line", route.line ?? "—"], ["File indexed", file?.indexed ? "yes" : "no"],
    ])),
  ];
}

function inspectModule(name) {
  const imports = data.imports.filter((entry) => !entry.target && entry.module === name);
  const sources = [...new Set(imports.map((entry) => entry.source))].map((id) => index.files.get(id)).filter(Boolean);
  return [
    inspectorHeader("External or unresolved module", name, `${imports.length} import records`, []),
    section("Resolution boundary", facts([
      ["Import records", exactNumber.format(imports.length)], ["Source files", exactNumber.format(sources.length)],
      ["System imports", exactNumber.format(imports.filter((entry) => entry.system).length)], ["Internal target", "not resolved"],
    ])),
    section("Imported from", relationList(sources, (file) => ({ title: file.path, value: file.language, handler: () => navigate("imports", { file: file.id }, { type: "file", id: file.id }) }))),
  ];
}

function renderInspector() {
  const selection = state.selected;
  elements["inspector-empty"].hidden = Boolean(selection);
  elements["inspector-content"].hidden = !selection;
  elements.inspector.classList.toggle("open", Boolean(selection));
  if (!selection) {
    elements["inspector-content"].replaceChildren();
    return;
  }
  let content = [];
  if (selection.type === "area") content = inspectArea(selection.id);
  else if (selection.type === "package") content = inspectPackage(selection.id);
  else if (selection.type === "file") content = inspectFile(selection.id);
  else if (selection.type === "symbol") content = inspectSymbol(selection.id);
  else if (selection.type === "task") content = inspectTask(selection.id);
  else if (selection.type === "route") content = inspectRoute(selection.id);
  else if (selection.type === "module") content = inspectModule(selection.id);
  else if (selection.type === "framework") {
    const routes = data.routes.filter((route) => route.framework === selection.id);
    content = [inspectorHeader("Framework", selection.id, `${routes.length} detected routes`, [
      { label: "Open routes", handler: () => navigate("routes", { framework: selection.id }, { type: "framework", id: selection.id }) },
    ]), section("Coverage", facts([["Routes", routes.length], ["Files", new Set(routes.map((route) => route.file)).size]]))];
  }
  elements["inspector-content"].replaceChildren(...content);
}

function renderLegend() {
  const legends = state.view === "tasks"
    ? [["done", "done"], ["implemented", "implemented"], ["progress", "in progress"], ["pending", "pending"]]
    : state.view === "imports"
      ? [["area", "source area"], ["file", "source file"], ["module", "external / unresolved module"]]
      : [["resolved", "resolver-confirmed relationship"], ["weight", "node size = relationship or file weight"]];
  elements.legend.replaceChildren(...legends.map(([colorClass, label]) => {
    const item = element("span", "legend-item");
    const dot = element("span", `legend-dot legend-${colorClass}`);
    item.append(dot, element("span", "", label));
    return item;
  }));
}

function render(updateGraph = true) {
  const definition = VIEW_DEFINITIONS.find((view) => view.id === state.view);
  for (const control of elements["view-nav"].children) control.classList.toggle("active", control.dataset.view === state.view);
  elements["view-kicker"].textContent = definition.kicker;
  elements["view-title"].textContent = definition.title;
  elements["back-button"].disabled = state.history.length === 0;
  renderBreadcrumbs();
  renderInventory();
  renderSearch();
  renderInspector();
  renderLegend();
  if (updateGraph) {
    const result = buildGraph();
    const selectedGraphId = state.selected?.type === "task" ? `t:${state.selected.id}` :
      state.selected?.type === "module" ? `m:${state.selected.id}` : state.selected?.id;
    graph.setGraph(result.nodes, result.edges, selectedGraphId);
    elements["render-note"].textContent = `showing ${exactNumber.format(result.nodes.length)} nodes · ${exactNumber.format(result.edges.length)} edges${result.total > result.nodes.length ? ` · ${exactNumber.format(result.total)} in scope` : ""}`;
    elements["graph-empty"].hidden = result.nodes.length !== 0;
  } else {
    const selectedGraphId = state.selected?.type === "task" ? `t:${state.selected.id}` :
      state.selected?.type === "module" ? `m:${state.selected.id}` : state.selected?.id;
    for (const node of graph.nodes) node.selected = node.id === selectedGraphId;
    graph.draw();
  }
}

function attachGlobalEvents() {
  elements.search.addEventListener("input", () => {
    state.query = elements.search.value;
    renderSearch();
  });
  elements["back-button"].addEventListener("click", goBack);
  elements["fit-button"].addEventListener("click", () => graph.fit());
  elements["fit-toolbar-button"].addEventListener("click", () => graph.fit());
  elements["help-button"].addEventListener("click", () => elements["help-dialog"].showModal());
  document.addEventListener("keydown", (event) => {
    if (event.key === "/" && document.activeElement !== elements.search) {
      event.preventDefault();
      elements.search.focus();
    } else if (event.key === "Escape") {
      if (state.query) {
        state.query = "";
        elements.search.value = "";
        renderSearch();
      } else if (state.selected) {
        state.selected = null;
        render(false);
      }
    } else if ((event.key === "f" || event.key === "F") && document.activeElement !== elements.search) graph.fit();
    else if (event.key === "ArrowLeft" && document.activeElement !== elements.search && state.history.length) goBack();
  });
}

async function initialize() {
  const response = await fetch("data/codebase-map.json", { cache: "no-store" });
  if (!response.ok) throw new Error(`Map data returned HTTP ${response.status}. Run \`npm run generate\`.`);
  data = await response.json();
  if (data.schemaVersion !== 1) throw new Error(`Unsupported map schema ${data.schemaVersion}.`);
  index = buildIndex();
  initializeHeader();
  initializeNavigation();
  attachGlobalEvents();
  elements.loading.remove();
  elements.app.hidden = false;
  graph.resize();
  render();
}

export const initialization = initialize();

initialization.catch((error) => {
  elements.loading.replaceChildren();
  const wrapper = element("div", "inspector-empty");
  wrapper.append(element("h2", "", "The codebase map could not load"), element("p", "", error.message));
  elements.loading.append(wrapper);
  console.error(error);
});
