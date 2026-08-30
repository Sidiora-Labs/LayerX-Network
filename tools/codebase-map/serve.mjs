import { createReadStream, existsSync, realpathSync, statSync } from "node:fs";
import { createServer } from "node:http";
import { extname, join, normalize, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = realpathSync(fileURLToPath(new URL(".", import.meta.url))).replace(/[/\\]$/, "");
const port = Number.parseInt(process.env.LAYERX_CODE_MAP_PORT ?? "4177", 10);
const host = process.env.LAYERX_CODE_MAP_HOST ?? "127.0.0.1";
const contentTypes = new Map([
  [".html", "text/html; charset=utf-8"],
  [".js", "text/javascript; charset=utf-8"],
  [".css", "text/css; charset=utf-8"],
  [".json", "application/json; charset=utf-8"],
  [".svg", "image/svg+xml"],
]);

function resolveRequest(requestUrl) {
  try {
    const pathname = decodeURIComponent(new URL(requestUrl, "http://localhost").pathname);
    const relativePath = pathname === "/" ? "index.html" : normalize(pathname).replace(/^[/\\]+/, "");
    const absolutePath = resolve(root, relativePath);
    if (absolutePath !== root && !absolutePath.startsWith(`${root}/`)) return null;
    if (!existsSync(absolutePath)) return absolutePath;
    const realPath = realpathSync(absolutePath);
    return realPath === root || realPath.startsWith(`${root}/`) ? realPath : null;
  } catch {
    return null;
  }
}

const server = createServer((request, response) => {
  if (request.method !== "GET" && request.method !== "HEAD") {
    response.writeHead(405, { "content-type": "text/plain; charset=utf-8", allow: "GET, HEAD" });
    response.end("Method not allowed\n");
    return;
  }
  const path = resolveRequest(request.url ?? "/");
  if (!path || !existsSync(path) || !statSync(path).isFile()) {
    response.writeHead(404, { "content-type": "text/plain; charset=utf-8" });
    response.end("Not found\n");
    return;
  }
  response.writeHead(200, {
    "content-type": contentTypes.get(extname(path)) ?? "application/octet-stream",
    "cache-control": path.endsWith("codebase-map.json") ? "no-store" : "public, max-age=60",
    "x-content-type-options": "nosniff",
    "content-security-policy": "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'none'",
  });
  if (request.method === "HEAD") {
    response.end();
    return;
  }
  const stream = createReadStream(path);
  stream.on("error", () => response.destroy());
  stream.pipe(response);
});

server.listen(port, host, () => {
  const dataPath = join(root, "data/codebase-map.json");
  if (!existsSync(dataPath)) console.warn("No generated map found. Run `npm run generate` first.");
  console.log(`LayerX codebase map: http://${host}:${port}`);
});
