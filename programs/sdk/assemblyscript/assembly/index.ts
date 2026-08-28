/**
 * Guest-side bindings for the version-one LayerX programs ABI.
 *
 * A program compiles to WebAssembly and imports the seven `layerx_v1` host
 * functions the runtime freezes. This package binds each of them with types that
 * make the runtime's laws unrepresentable rather than merely discouraged:
 *
 * - money crosses the boundary only as `Amount`, an exact unsigned one hundred
 *   and twenty-eight bit integer with no floating-point constructor, conversion
 *   or overloaded operator, and with every arithmetic operation checked;
 * - identifiers, storage keys, event topics, call inputs and capability sets
 *   check their bounds before the host call, so a value the host would refuse
 *   never leaves the guest;
 * - authority is explicit: a capability set is ordered by the runtime's own
 *   authority key, refuses duplicates, and can only be narrowed on the way into
 *   another program.
 *
 * Nothing here reaches a clock, a socket, a thread, an allocator the runtime
 * cannot reproduce, or a source of entropy, and no declaration in the package
 * names a floating-point type.
 */

export {
  ABI_MODULE,
  ABI_VERSION,
  AMOUNT_BYTES,
  CALL_ENTRY_EXPORT,
  CALL_INPUT_CAPACITY,
  CALL_RESERVE_EXPORT,
  DIGEST_BYTES,
  ENTRYPOINT,
  HOST_FUNCTION_COUNT,
  HostFunction,
  IDENTIFIER_BYTES,
  MAX_CALL_INPUT_BYTES,
  MAX_CAPABILITIES,
  MAX_CAPABILITY_ENCODING_BYTES,
  MAX_CAPABILITY_ENCODING_GRANT_BYTES,
  MAX_CAPABILITY_ENCODING_HEADER_BYTES,
  MAX_CANONICAL_CAPABILITY_SET_BYTES,
  MAX_EVENT_DATA_BYTES,
  MAX_EVENTS_PER_ACTIVITY,
  MAX_EVENT_TOPIC_BYTES,
  MAX_STORAGE_KEY_BYTES,
  MAX_STORAGE_VALUE_BYTES,
  MEMORY_EXPORT,
  RECEIPT_ENCODING_BYTES,
  RESERVATION_REFUSED,
  RUNTIME_VERSION,
  abiManifest,
  hostFunctions
} from "./abi";

export { Amount, AmountResult } from "./amount";

export {
  allocate,
  compare,
  copy,
  equal,
  fromString,
  identifierFromWords,
  isZero,
  pointer,
  readI32BE,
  readU16BE,
  readU32BE,
  readU64BE,
  slice,
  writeU16BE,
  writeU32BE,
  writeU64BE
} from "./bytes";

export { callProgram, callProgramWith } from "./call";

export {
  CAPABILITY_CALL,
  CAPABILITY_EMIT_EVENT,
  CAPABILITY_RECEIPT_READ,
  CAPABILITY_STORAGE_READ,
  CAPABILITY_STORAGE_WRITE,
  CAPABILITY_SHARED_STORAGE_READ,
  CAPABILITY_SHARED_STORAGE_WRITE,
  CAPABILITY_TRANSFER_402,
  Capability,
  CapabilityEncoding,
  CapabilitySet
} from "./capability";

export {
  ERR_BOUNDS,
  ERR_BUFFER_TOO_SMALL,
  ERR_CAPABILITY_BYTES,
  ERR_CAPABILITY_LIMIT,
  ERR_DATA_TOO_LARGE,
  ERR_DENIED,
  ERR_DUPLICATE_CAPABILITY,
  ERR_EMPTY_KEY,
  ERR_EMPTY_TOPIC,
  ERR_EVIDENCE,
  ERR_INPUT_TOO_LARGE,
  ERR_INVALID,
  ERR_KEY_TOO_LARGE,
  ERR_METER,
  ERR_NULL_ARGUMENT,
  ERR_OVERFLOW,
  ERR_RECEIPT_ENCODING,
  ERR_RESERVED_IDENTIFIER,
  ERR_TOPIC_TOO_LARGE,
  ERR_UNDERFLOW,
  ERR_VALUE_TOO_LARGE,
  ERR_ZERO_AMOUNT,
  OK,
  abiStatus,
  isHostRefusal,
  isRefusal,
  statusName
} from "./error";

export {
  acceptCallInput,
  callInputBytes,
  callInputRegion,
  reserveCallInput
} from "./entry";

export { emitEvent } from "./event";

export { AccountId, AssetId, ProgramId, ReceiptDigest } from "./ids";

export { Receipt, ReceiptRead, decodeReceipt, readReceipt } from "./receipt";

export { StoredValue, checkKey, deleteValue, readValue, writeValue } from "./storage";

export { Payment, transfer402 } from "./transfer";
