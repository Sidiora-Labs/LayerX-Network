import { mkdir, chmod, lstat, open } from "node:fs/promises";
import { resolve, join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { MiddlewareError } from "@sidiora/layerx-seller-middleware";

const queues = new Map();

export class FileFulfillmentRepository {
  constructor(directory) {
    this.directory = resolve(directory);
  }

  async fulfill(proposed, release) {
    const before = queues.get(this.directory) ?? Promise.resolve();
    const operation = before.then(() => this.commit(proposed, release));
    const barrier = operation.then(() => undefined, () => undefined);
    queues.set(this.directory, barrier);
    try {
      return await operation;
    } finally {
      if (queues.get(this.directory) === barrier) queues.delete(this.directory);
    }
  }

  async commit(proposed, release) {
    if (!/^[0-9a-f]{64}$/u.test(proposed.idempotencyKey)
      || !/^[0-9a-f]{64}$/u.test(proposed.requestDigest)) {
      throw new MiddlewareError("fulfillment-conflict");
    }
    await mkdir(this.directory, { recursive: true, mode: 0o700 });
    const directory = await lstat(this.directory);
    if (!directory.isDirectory() || directory.isSymbolicLink() || (directory.mode & 0o777) !== 0o700) {
      throw new MiddlewareError("fulfillment-conflict");
    }
    const path = join(this.directory, "fulfillments.sqlite");
    const file = await open(path, "a", 0o600);
    await file.close();
    const metadata = await lstat(path);
    if (!metadata.isFile() || metadata.isSymbolicLink() || metadata.nlink !== 1) {
      throw new MiddlewareError("fulfillment-conflict");
    }
    await chmod(path, 0o600);
    const database = new DatabaseSync(path);
    try {
      database.exec("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA busy_timeout=30000;");
      database.exec("CREATE TABLE IF NOT EXISTS fulfillment (id TEXT PRIMARY KEY, request_digest TEXT NOT NULL, receipt TEXT NOT NULL, resource TEXT NOT NULL)");
      database.exec("BEGIN IMMEDIATE");
      const stored = database.prepare("SELECT request_digest, receipt, resource FROM fulfillment WHERE id = ?").get(proposed.idempotencyKey);
      if (stored !== undefined) {
        if (stored.request_digest !== proposed.requestDigest
          || stored.receipt !== Buffer.from(proposed.canonicalReceipt).toString("base64")) {
          throw new MiddlewareError("fulfillment-conflict");
        }
        database.exec("COMMIT");
        return { ...proposed, resource: JSON.parse(stored.resource) };
      }
      const resource = await release();
      const encoded = JSON.stringify(resource);
      if (encoded === undefined) throw new MiddlewareError("fulfillment-conflict");
      database.prepare("INSERT INTO fulfillment VALUES (?, ?, ?, ?)").run(
        proposed.idempotencyKey, proposed.requestDigest,
        Buffer.from(proposed.canonicalReceipt).toString("base64"), encoded,
      );
      database.exec("COMMIT");
      return { ...proposed, resource };
    } catch (error) {
      if (database.isTransaction) database.exec("ROLLBACK");
      throw error;
    } finally {
      database.close();
    }
  }
}
