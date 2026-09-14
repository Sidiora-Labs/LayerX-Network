import { runtimeMessages } from "./messages.generated.ts";

export interface RuntimeCopyEntry {
  readonly key: string;
  readonly message: string;
}

const catalog = new Map<string, RuntimeCopyEntry>(runtimeMessages.map(([key, message]) => [
  key,
  Object.freeze({ key, message }),
]));

export function human_copy_catalog(): ReadonlyMap<string, RuntimeCopyEntry> {
  return catalog;
}

export function copyEntry(key: string): RuntimeCopyEntry {
  const entry = catalog.get(key);
  if (entry === undefined) throw new Error(`Unknown copy key: ${key}`);
  return entry;
}
