/**
 * The paid-counter quickstart.
 *
 * It configures a fee, settles a verified 402LXP receipt against that fee,
 * advances a counter, pays the operator and emits the evidence a consumer
 * renders. Every value that crosses the guest boundary is an integer, and every
 * effect is an explicit capability the invoking activity granted.
 *
 * The storage keys, event topics, payload layouts, export names and export
 * signatures below are the same ones the C quickstart in programs/sdk/c uses, so
 * the two programs are indistinguishable on the wire.
 */

import {
  AccountId,
  Amount,
  AssetId,
  Capability,
  CapabilitySet,
  ERR_INVALID,
  ERR_RESERVED_IDENTIFIER,
  ERR_ZERO_AMOUNT,
  IDENTIFIER_BYTES,
  OK,
  ProgramId,
  ReceiptDigest,
  acceptCallInput,
  callInputRegion,
  callProgramWith,
  copy,
  deleteValue,
  emitEvent,
  equal,
  fromString,
  readReceipt,
  readU64BE,
  readValue,
  reserveCallInput,
  transfer402,
  writeU64BE,
  writeValue
} from "../../../assembly/index";

const QUICKSTART_ERR_NOT_CONFIGURED: i32 = -64;
const QUICKSTART_ERR_STATE: i32 = -65;
const QUICKSTART_ERR_ASSET_MISMATCH: i32 = -66;
const QUICKSTART_ERR_UNDERPAID: i32 = -67;
const QUICKSTART_ERR_RECEIPT_FAILED: i32 = -68;
const QUICKSTART_ERR_COUNTER_OVERFLOW: i32 = -69;

const CONFIGURED_EVENT_BYTES: i32 = 80;
const SETTLED_EVENT_BYTES: i32 = 56;
const NOTED_EVENT_BYTES: i32 = 16;
const FORWARD_INPUT_BYTES: i32 = 8;
const COUNTER_BYTES: i32 = 8;
const AMOUNT_BYTES: i32 = 16;

/**
 * The canonical entrypoint carries a single integer, so the activity picks the
 * operation with a selector. Every other selector is refused rather than
 * silently treated as the first one.
 */
const ENTRY_SELECTOR_COUNT: i64 = 0;
const ENTRY_SELECTOR_RESET: i64 = 1;

function keyFee(): StaticArray<u8> {
  return fromString("layerx.quickstart.fee");
}

function keyAsset(): StaticArray<u8> {
  return fromString("layerx.quickstart.asset");
}

function keyPayee(): StaticArray<u8> {
  return fromString("layerx.quickstart.payee");
}

function keyCount(): StaticArray<u8> {
  return fromString("layerx.quickstart.count");
}

function topicConfigured(): StaticArray<u8> {
  return fromString("layerx.quickstart.configured");
}

function topicSettled(): StaticArray<u8> {
  return fromString("layerx.quickstart.settled");
}

function topicNoted(): StaticArray<u8> {
  return fromString("layerx.quickstart.noted");
}

class StoredField {
  status: i32;
  value: StaticArray<u8>;

  constructor(status: i32, value: StaticArray<u8>) {
    this.status = status;
    this.value = value;
  }
}

function loadExact(key: StaticArray<u8>, expected: i32): StoredField {
  const output = new StaticArray<u8>(expected);
  const stored = readValue(key, output);
  if (!stored.ok()) return new StoredField(stored.status, output);
  if (!stored.found) return new StoredField(QUICKSTART_ERR_NOT_CONFIGURED, output);
  if (stored.length != expected) return new StoredField(QUICKSTART_ERR_STATE, output);
  return new StoredField(OK, output);
}

class CounterRead {
  status: i32;
  value: u64;

  constructor(status: i32, value: u64) {
    this.status = status;
    this.value = value;
  }
}

function loadCounter(): CounterRead {
  const encoded = new StaticArray<u8>(COUNTER_BYTES);
  const stored = readValue(keyCount(), encoded);
  if (!stored.ok()) return new CounterRead(stored.status, 0);
  if (!stored.found) return new CounterRead(OK, 0);
  if (stored.length != COUNTER_BYTES) return new CounterRead(QUICKSTART_ERR_STATE, 0);
  return new CounterRead(OK, readU64BE(encoded, 0));
}

export function configure(
  feeHigh: i64,
  feeLow: i64,
  assetWord0: i64,
  assetWord1: i64,
  assetWord2: i64,
  assetWord3: i64,
  payeeWord0: i64,
  payeeWord1: i64,
  payeeWord2: i64,
  payeeWord3: i64
): i32 {
  const fee = Amount.fromParts(<u64>feeHigh, <u64>feeLow);
  const asset = AssetId.fromWords(<u64>assetWord0, <u64>assetWord1, <u64>assetWord2, <u64>assetWord3);
  const payee = AccountId.fromWords(
    <u64>payeeWord0,
    <u64>payeeWord1,
    <u64>payeeWord2,
    <u64>payeeWord3
  );
  if (fee.isZero()) return ERR_ZERO_AMOUNT;
  if (asset === null || payee === null) return ERR_RESERVED_IDENTIFIER;
  const configuredAsset = changetype<AssetId>(asset);
  const configuredPayee = changetype<AccountId>(payee);
  const configured = new StaticArray<u8>(CONFIGURED_EVENT_BYTES);
  fee.writeBigEndian(configured, 0);
  copy(configured, 16, configuredAsset.bytes, 0, IDENTIFIER_BYTES);
  copy(configured, 48, configuredPayee.bytes, 0, IDENTIFIER_BYTES);
  let status = writeValue(keyFee(), fee.toBigEndian());
  if (status != OK) return status;
  status = writeValue(keyAsset(), configuredAsset.bytes);
  if (status != OK) return status;
  status = writeValue(keyPayee(), configuredPayee.bytes);
  if (status != OK) return status;
  status = writeValue(keyCount(), new StaticArray<u8>(COUNTER_BYTES));
  if (status != OK) return status;
  return emitEvent(topicConfigured(), configured);
}

export function count(): i64 {
  const counter = loadCounter();
  if (counter.status != OK) return <i64>counter.status;
  if (counter.value > <u64>i64.MAX_VALUE) return <i64>QUICKSTART_ERR_COUNTER_OVERFLOW;
  return <i64>counter.value;
}

export function settle(
  digestWord0: i64,
  digestWord1: i64,
  digestWord2: i64,
  digestWord3: i64
): i64 {
  const digest = ReceiptDigest.fromWords(
    <u64>digestWord0,
    <u64>digestWord1,
    <u64>digestWord2,
    <u64>digestWord3
  );
  if (digest === null) return <i64>ERR_RESERVED_IDENTIFIER;
  const storedFee = loadExact(keyFee(), AMOUNT_BYTES);
  if (storedFee.status != OK) return <i64>storedFee.status;
  const storedAsset = loadExact(keyAsset(), IDENTIFIER_BYTES);
  if (storedAsset.status != OK) return <i64>storedAsset.status;
  const storedPayee = loadExact(keyPayee(), IDENTIFIER_BYTES);
  if (storedPayee.status != OK) return <i64>storedPayee.status;
  const fee = Amount.fromBigEndian(storedFee.value, 0);
  const asset = AssetId.fromBytes(storedAsset.value, 0);
  const payee = AccountId.fromBytes(storedPayee.value, 0);
  if (asset === null || payee === null) return <i64>ERR_RESERVED_IDENTIFIER;
  const configuredAsset = changetype<AssetId>(asset);
  const configuredPayee = changetype<AccountId>(payee);
  const evidence = readReceipt(changetype<ReceiptDigest>(digest));
  if (!evidence.ok()) return <i64>evidence.status;
  const receipt = evidence.receipt;
  if (!receipt.settled()) return <i64>QUICKSTART_ERR_RECEIPT_FAILED;
  if (!equal(receipt.asset, 0, configuredAsset.bytes, 0, IDENTIFIER_BYTES)) {
    return <i64>QUICKSTART_ERR_ASSET_MISMATCH;
  }
  if (receipt.amount.compare(fee) < 0) return <i64>QUICKSTART_ERR_UNDERPAID;
  const counter = loadCounter();
  if (counter.status != OK) return <i64>counter.status;
  if (counter.value == u64.MAX_VALUE) return <i64>QUICKSTART_ERR_COUNTER_OVERFLOW;
  const advanced = counter.value + 1;
  const settled = new StaticArray<u8>(SETTLED_EVENT_BYTES);
  writeU64BE(settled, 0, advanced);
  receipt.amount.writeBigEndian(settled, 8);
  copy(settled, 24, changetype<ReceiptDigest>(digest).bytes, 0, IDENTIFIER_BYTES);
  const counterBytes = new StaticArray<u8>(COUNTER_BYTES);
  writeU64BE(counterBytes, 0, advanced);
  let status = writeValue(keyCount(), counterBytes);
  if (status != OK) return <i64>status;
  status = transfer402(configuredAsset, configuredPayee, fee);
  if (status != OK) return <i64>status;
  status = emitEvent(topicSettled(), settled);
  if (status != OK) return <i64>status;
  if (advanced > <u64>i64.MAX_VALUE) return <i64>QUICKSTART_ERR_COUNTER_OVERFLOW;
  return <i64>advanced;
}

export function forward(
  calleeWord0: i64,
  calleeWord1: i64,
  calleeWord2: i64,
  calleeWord3: i64,
  note: i64
): i32 {
  const callee = ProgramId.fromWords(
    <u64>calleeWord0,
    <u64>calleeWord1,
    <u64>calleeWord2,
    <u64>calleeWord3
  );
  if (callee === null) return ERR_RESERVED_IDENTIFIER;
  const narrowed = new CapabilitySet(2);
  let status = narrowed.insert(Capability.storageRead());
  if (status != OK) return status;
  status = narrowed.insert(Capability.emitEvent());
  if (status != OK) return status;
  const input = new StaticArray<u8>(FORWARD_INPUT_BYTES);
  writeU64BE(input, 0, <u64>note);
  return callProgramWith(changetype<ProgramId>(callee), input, narrowed);
}

export function reset(): i32 {
  return deleteValue(keyCount());
}

export function layerx_main(selector: i64): i64 {
  if (selector == ENTRY_SELECTOR_COUNT) return count();
  if (selector == ENTRY_SELECTOR_RESET) return <i64>reset();
  return <i64>ERR_INVALID;
}

export function layerx_reserve(length: i32): i32 {
  return reserveCallInput(length);
}

/**
 * The callee half of forward(). It holds only the storage-read and emit-event
 * grants forward() narrows to, so it reads the counter and files the caller's
 * note as evidence without ever reaching for an authority it was not handed.
 */
export function layerx_call(inputPointer: i32, inputLength: i32): i32 {
  const admitted = acceptCallInput(inputPointer, inputLength);
  if (admitted < 0) return admitted;
  if (admitted != FORWARD_INPUT_BYTES) return ERR_INVALID;
  const counter = loadCounter();
  if (counter.status != OK) return counter.status;
  const noted = new StaticArray<u8>(NOTED_EVENT_BYTES);
  writeU64BE(noted, 0, counter.value);
  copy(noted, 8, callInputRegion(), 0, FORWARD_INPUT_BYTES);
  return emitEvent(topicNoted(), noted);
}
