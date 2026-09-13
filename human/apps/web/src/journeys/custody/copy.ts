import { human_copy_catalog } from "../../../copy/runtime.ts";

export function custodyCopyKey(key: string, fallbackKey: string): string {
  if (human_copy_catalog().has(key)) {
    return key;
  }
  if (!human_copy_catalog().has(fallbackKey)) {
    throw new Error(`Unknown fallback copy key: ${fallbackKey}`);
  }
  return fallbackKey;
}
