import { readFile, lstat } from "node:fs/promises";
import https from "node:https";
import { X509Certificate } from "node:crypto";
import { fileURLToPath } from "node:url";

import next from "next";

async function protectedBytes(filename) {
  const metadata = await lstat(filename);
  if (!metadata.isFile() || metadata.isSymbolicLink() || metadata.nlink !== 1
    || metadata.uid !== process.getuid() || (metadata.mode & 0o777) !== 0o600
    || metadata.size === 0 || metadata.size > 65_536) {
    throw new Error("Production TLS material ownership, type or bounds refused");
  }
  return readFile(filename);
}

const configuration = JSON.parse(await protectedBytes(process.env.HUMAN_E2E_TLS_CONFIG));
if (Object.keys(configuration).sort().join(",") !== "certificate,key,origin,service,version"
  || configuration.version !== 1) throw new Error("Production TLS configuration refused");
const origin = new URL(configuration.origin);
if (origin.protocol !== "https:" || origin.port !== "" || origin.pathname !== "/"
  || origin.username !== "" || origin.password !== "" || origin.search !== "" || origin.hash !== ""
  || origin.origin !== process.env.LAYERX_HUMAN_WEB_ORIGIN
  || configuration.service !== process.env.LAYERX_HUMAN_SERVICE_URL) {
  throw new Error("Production TLS origin or backend binding refused");
}
const certificate = await protectedBytes(configuration.certificate);
if (new X509Certificate(certificate).checkHost(origin.hostname) !== origin.hostname) {
  throw new Error("Production TLS certificate does not bind the application host");
}
const application = next({
  dev: false,
  dir: fileURLToPath(new URL("../", import.meta.url)),
  hostname: origin.hostname,
  port: 443,
});
await application.prepare();
const handler = application.getRequestHandler();
const server = https.createServer({
  key: await protectedBytes(configuration.key),
  cert: certificate,
  minVersion: "TLSv1.2",
}, (request, response) => {
  if (request.headers.host !== origin.host) {
    response.writeHead(421).end();
    return;
  }
  handler(request, response).catch(() => {
    if (!response.headersSent) response.writeHead(500);
    response.end();
  });
});
server.on("error", (error) => {
  console.error(error.message);
  process.exitCode = 1;
  application.close().catch(() => { process.exitCode = 1; });
});
for (const signal of ["SIGINT", "SIGTERM"]) {
  process.once(signal, () => {
    server.close(() => {
      application.close().catch(() => { process.exitCode = 1; });
    });
    server.closeIdleConnections();
  });
}
server.listen(443, "127.0.0.1", () => { console.log("Ready in production HTTPS mode"); });
