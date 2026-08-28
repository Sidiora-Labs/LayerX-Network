/**
 * Frozen version-one ABI vocabulary shared with the programs runtime.
 *
 * Every constant here mirrors `layerx-programs-runtime` exactly, and the
 * manifest bytes are byte-for-byte the ones the Rust and C SDKs publish. The
 * determinism lint compares the produced module against this surface, so a
 * program can never be compiled against a stale manifest.
 */

import { copy, fromString } from "./bytes";

/** Host module every program imports. */
export const ABI_MODULE: string = "layerx_v1";
/** Canonical export name the runtime invokes on a program. */
export const ENTRYPOINT: string = "layerx_main";
/** Export a composable program provides as its program-to-program call entry point. */
export const CALL_ENTRY_EXPORT: string = "layerx_call";
/** Export a composable program provides to reserve a bounded input region. */
export const CALL_RESERVE_EXPORT: string = "layerx_reserve";
/** Export through which the host reads and writes guest linear memory. */
export const MEMORY_EXPORT: string = "memory";

/** ABI version these bindings speak. */
export const ABI_VERSION: i32 = 1;
/** Runtime version these bindings were frozen against. */
export const RUNTIME_VERSION: i32 = 1;
/** Width of every protocol identifier. */
export const IDENTIFIER_BYTES: i32 = 32;
/** Width of the exact protocol monetary integer. */
export const AMOUNT_BYTES: i32 = 16;
/** Width of a receipt digest. */
export const DIGEST_BYTES: i32 = 32;
/** Exact length of the encoded receipt view returned by `receipt_read`. */
export const RECEIPT_ENCODING_BYTES: i32 = 116;
/** Maximum key length admitted by the version-one storage ABI. */
export const MAX_STORAGE_KEY_BYTES: i32 = 256;
/** Maximum value length admitted by the version-one storage ABI. */
export const MAX_STORAGE_VALUE_BYTES: i32 = 1048576;
/** Maximum event topic length admitted by the version-one ABI. */
export const MAX_EVENT_TOPIC_BYTES: i32 = 64;
/** Maximum event payload length admitted by the version-one ABI. */
export const MAX_EVENT_DATA_BYTES: i32 = 65536;
/** Runtime-enforced aggregate event count across one activity call graph. */
export const MAX_EVENTS_PER_ACTIVITY: i32 = 64;
/** Maximum call input length admitted by the version-one ABI. */
export const MAX_CALL_INPUT_BYTES: i32 = 1048576;
/** Maximum number of grants in one capability set. */
export const MAX_CAPABILITY_ENCODING_HEADER_BYTES: i32 = 2;
export const MAX_CAPABILITY_ENCODING_GRANT_BYTES: i32 = 275;
/** Maximum encoded capability-list length the host will read. */
export const MAX_CAPABILITY_ENCODING_BYTES: i32 = 65535;
export const MAX_CAPABILITIES: i32 =
  (MAX_CAPABILITY_ENCODING_BYTES - MAX_CAPABILITY_ENCODING_HEADER_BYTES) /
  MAX_CAPABILITY_ENCODING_GRANT_BYTES;
export const MAX_CANONICAL_CAPABILITY_SET_BYTES: i32 =
  MAX_CAPABILITY_ENCODING_HEADER_BYTES +
  MAX_CAPABILITIES * MAX_CAPABILITY_ENCODING_GRANT_BYTES;
/** Declared capacity of the call-input reservation every SDK program owns. */
export const CALL_INPUT_CAPACITY: i32 = 8192;
/** Value `layerx_reserve` returns when it refuses a reservation. */
export const RESERVATION_REFUSED: i32 = -1;
/** Number of host functions in the frozen version-one surface. */
export const HOST_FUNCTION_COUNT: i32 = 7;

/** One entry of the frozen host-function surface. */
export class HostFunction {
  name: string;
  signature: string;

  constructor(name: string, signature: string) {
    this.name = name;
    this.signature = signature;
  }
}

/** The seven host functions a version-one program may import. */
export function hostFunctions(): HostFunction[] {
  const functions = new Array<HostFunction>();
  functions.push(new HostFunction("storage_read", "(i32,i32,i32,i32)->i32"));
  functions.push(new HostFunction("storage_write", "(i32,i32,i32,i32)->i32"));
  functions.push(new HostFunction("storage_delete", "(i32,i32)->i32"));
  functions.push(new HostFunction("event_emit", "(i32,i32,i32,i32)->i32"));
  functions.push(new HostFunction("program_call", "(i32,i32,i32,i32,i32,i32)->i32"));
  functions.push(new HostFunction("transfer_402", "(i64,i64,i32,i32,i32,i32)->i32"));
  functions.push(new HostFunction("receipt_read", "(i32,i32,i32,i32)->i32"));
  return functions;
}

/**
 * The frozen manifest as the runtime publishes it: the module name and each
 * host-function signature, every record terminated by a zero byte.
 */
export function abiManifest(): StaticArray<u8> {
  const records = new Array<StaticArray<u8>>();
  records.push(fromString(ABI_MODULE));
  const functions = hostFunctions();
  for (let index = 0; index < functions.length; index++) {
    const entry = functions[index];
    records.push(fromString(entry.name + entry.signature));
  }
  let total = 0;
  for (let index = 0; index < records.length; index++) {
    total += records[index].length + 1;
  }
  const manifest = new StaticArray<u8>(total);
  let cursor = 0;
  for (let index = 0; index < records.length; index++) {
    const record = records[index];
    copy(manifest, cursor, record, 0, record.length);
    cursor += record.length;
    manifest[cursor] = 0;
    cursor += 1;
  }
  return manifest;
}
