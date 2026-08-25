/**
 * Namespaced storage bindings.
 *
 * Keys address the calling program's own program and principal namespace only.
 * No binding here accepts a namespace, so neither an adjacent program nor an
 * adjacent principal can be reached by choosing a key. Bounds are checked before
 * the host call, so a value the runtime would refuse never leaves the guest.
 */

import { MAX_STORAGE_KEY_BYTES, MAX_STORAGE_VALUE_BYTES } from "./abi";
import { pointer } from "./bytes";
import {
  ERR_BUFFER_TOO_SMALL,
  ERR_EMPTY_KEY,
  ERR_KEY_TOO_LARGE,
  ERR_VALUE_TOO_LARGE,
  OK
} from "./error";
import { storageDelete, storageRead, storageWrite } from "./host";

/** The outcome of one namespaced read. */
export class StoredValue {
  status: i32;
  found: bool;
  length: i32;

  constructor(status: i32, found: bool, length: i32) {
    this.status = status;
    this.found = found;
    this.length = length;
  }

  /** Reports whether the host admitted the read. */
  ok(): bool {
    return this.status == OK;
  }
}

/** Refuses a key the version-one storage ABI would reject. */
export function checkKey(key: StaticArray<u8>): i32 {
  if (key.length == 0) return ERR_EMPTY_KEY;
  if (key.length > MAX_STORAGE_KEY_BYTES) return ERR_KEY_TOO_LARGE;
  return OK;
}

/**
 * Reads one value into a caller-owned array, reporting whether the key held a
 * value at all and how many bytes it holds. The host reports absence as zero and
 * presence as the stored length plus one, which this binding decodes so a
 * program never sees the raw convention.
 */
export function readValue(key: StaticArray<u8>, output: StaticArray<u8>): StoredValue {
  const keyStatus = checkKey(key);
  if (keyStatus != OK) return new StoredValue(keyStatus, false, 0);
  if (output.length > MAX_STORAGE_VALUE_BYTES) {
    return new StoredValue(ERR_VALUE_TOO_LARGE, false, 0);
  }
  const outcome = storageRead(pointer(key), key.length, pointer(output), output.length);
  if (outcome < 0) return new StoredValue(outcome, false, 0);
  if (outcome == 0) return new StoredValue(OK, false, 0);
  const length = outcome - 1;
  if (length < 0 || length > output.length) {
    return new StoredValue(ERR_BUFFER_TOO_SMALL, false, 0);
  }
  return new StoredValue(OK, true, length);
}

/** Stages one value in this program's namespace. */
export function writeValue(key: StaticArray<u8>, value: StaticArray<u8>): i32 {
  const keyStatus = checkKey(key);
  if (keyStatus != OK) return keyStatus;
  if (value.length > MAX_STORAGE_VALUE_BYTES) return ERR_VALUE_TOO_LARGE;
  return storageWrite(pointer(key), key.length, pointer(value), value.length);
}

/** Stages the deletion of one key in this program's namespace. */
export function deleteValue(key: StaticArray<u8>): i32 {
  const keyStatus = checkKey(key);
  if (keyStatus != OK) return keyStatus;
  return storageDelete(pointer(key), key.length);
}
