import type { AuthorizedReceiptBatch } from "@sidiora/layerx-sdk";
import type {
  AuthorizedBatchResolver,
  FulfillmentRepository,
  StoredFulfillment,
  WebhookClaimResult,
  WebhookDeliveryClaim,
  WebhookDeliveryStore,
} from "@sidiora/layerx-seller-middleware";

/** Resolves every receipt to a single fixed authorised batch, as a real relay would after fetching it. */
export class FixedBatchResolver implements AuthorizedBatchResolver {
  public constructor(private readonly batch: AuthorizedReceiptBatch) {}

  public async resolve(): Promise<AuthorizedReceiptBatch> {
    return this.batch;
  }
}

/**
 * An in-memory, first-writer-wins fulfilment ledger. The release callback runs
 * exactly once per idempotency key; a replay returns the stored resource and a
 * digest mismatch is a conflict. This is the real durability contract the
 * seller middleware relies on, backed by a Map instead of a database.
 */
export class InMemoryFulfillmentRepository<T> implements FulfillmentRepository<T> {
  readonly #records = new Map<string, StoredFulfillment<T>>();

  public releaseCount = 0;

  public async fulfill(
    proposed: Omit<StoredFulfillment<T>, "resource">,
    release: () => Promise<T>,
  ): Promise<StoredFulfillment<T>> {
    const existing = this.#records.get(proposed.idempotencyKey);
    if (existing !== undefined) {
      if (existing.requestDigest !== proposed.requestDigest) {
        throw new Error("fulfillment-conflict");
      }
      return existing;
    }
    const resource = await release();
    this.releaseCount += 1;
    const stored: StoredFulfillment<T> = { ...proposed, resource };
    this.#records.set(proposed.idempotencyKey, stored);
    return stored;
  }
}

/** In-memory webhook delivery ledger enforcing single-flight, replay and conflict semantics. */
export class InMemoryDeliveryStore implements WebhookDeliveryStore {
  readonly #records = new Map<string, { digest: string; done: boolean }>();

  public async claim(value: WebhookDeliveryClaim): Promise<WebhookClaimResult> {
    const existing = this.#records.get(value.deliveryId);
    if (existing === undefined) {
      this.#records.set(value.deliveryId, { digest: value.payloadDigest, done: false });
      return "claimed";
    }
    if (existing.digest !== value.payloadDigest) {
      return "conflict";
    }
    return existing.done ? "completed" : "processing";
  }

  public async complete(deliveryId: string, payloadDigest: string): Promise<void> {
    const record = this.#records.get(deliveryId);
    if (record !== undefined && record.digest === payloadDigest) {
      record.done = true;
    }
  }

  public async release(deliveryId: string, payloadDigest: string): Promise<void> {
    const record = this.#records.get(deliveryId);
    if (record !== undefined && record.digest === payloadDigest && !record.done) {
      this.#records.delete(deliveryId);
    }
  }
}

export interface CheckResult {
  readonly name: string;
  readonly ok: boolean;
  readonly detail?: string;
}

/** A minimal assertion harness that records outcomes rather than throwing eagerly, so a full run reports every failure. */
export class Suite {
  readonly #results: CheckResult[] = [];

  public async check(name: string, body: () => Promise<void> | void): Promise<void> {
    try {
      await body();
      this.#results.push({ name, ok: true });
    } catch (error) {
      this.#results.push({ name, ok: false, detail: error instanceof Error ? error.message : String(error) });
    }
  }

  public results(): readonly CheckResult[] {
    return this.#results;
  }
}

export async function expectThrows(
  body: () => Promise<unknown> | unknown,
  predicate: (error: unknown) => boolean,
  description: string,
): Promise<void> {
  try {
    await body();
  } catch (error) {
    if (!predicate(error)) {
      throw new Error(`${description}: threw the wrong error: ${error instanceof Error ? error.message : String(error)}`);
    }
    return;
  }
  throw new Error(`${description}: expected a rejection but the call resolved`);
}

export function assert(condition: boolean, message: string): void {
  if (!condition) {
    throw new Error(message);
  }
}
