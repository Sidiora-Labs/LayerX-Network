import {
  AccountId,
  Amount,
  AssetId,
  Capability,
  CapabilitySet,
  callProgram,
  ERR_CAPABILITY_BYTES,
  ERR_CAPABILITY_LIMIT,
  MAX_CAPABILITIES,
  MAX_CAPABILITY_ENCODING_BYTES,
  MAX_CANONICAL_CAPABILITY_SET_BYTES,
  MAX_EVENTS_PER_ACTIVITY,
  ProgramId
} from "../assembly/index";

const MIXED_FIXTURE_HEX = "000303041111111111111111111111111111111111111111111111111111111111111111052222222222222222222222222222222222222222222222222222222222222222333333333333333333333333333333333333333333333333333333333333333300000000000000000000000000000007";

function hexDigit(byte: u8): i32 {
  if (byte >= 48 && byte <= 57) return <i32>byte - 48;
  if (byte >= 97 && byte <= 102) return <i32>byte - 87;
  return -1;
}

function mixedFixtureBytes(): StaticArray<u8> {
  const decoded = new StaticArray<u8>(MIXED_FIXTURE_HEX.length / 2);
  for (let index = 0; index < decoded.length; index++) {
    const high = hexDigit(<u8>MIXED_FIXTURE_HEX.charCodeAt(index * 2));
    const low = hexDigit(<u8>MIXED_FIXTURE_HEX.charCodeAt(index * 2 + 1));
    if (high < 0 || low < 0) return new StaticArray<u8>(0);
    decoded[index] = <u8>((high << 4) | low);
  }
  return decoded;
}

export function identifierFactoriesRejectMalformedAndProtectStorage(): bool {
  if (ProgramId.fromBytes(new StaticArray<u8>(31), 0) !== null) return false;
  if (ProgramId.fromBytes(new StaticArray<u8>(32), 0) !== null) return false;
  const source = new StaticArray<u8>(32);
  source[31] = 1;
  const identifier = ProgramId.fromBytes(source, 0);
  if (identifier === null) return false;
  const first = changetype<ProgramId>(identifier).bytes;
  first[31] = 0;
  return changetype<ProgramId>(identifier).bytes[31] == 1;
}

export function boundedCapabilitySetReportsStatusExplicitly(): bool {
  const set = new CapabilitySet(-1);
  const account = AccountId.fromWords(<u64>0, <u64>0, <u64>0, <u64>1);
  const asset = AssetId.fromWords(<u64>0, <u64>0, <u64>0, <u64>1);
  if (account === null || asset === null) return false;
  const grant = Capability.transfer402(
    changetype<AssetId>(asset),
    changetype<AccountId>(account),
    Amount.fromParts(0, 1)
  );
  if (set.insert(grant) != ERR_CAPABILITY_LIMIT) {
    return false;
  }
  const encoded = set.toBytes();
  return set.status == ERR_CAPABILITY_LIMIT &&
    encoded.status == ERR_CAPABILITY_LIMIT &&
    !encoded.ok() &&
    encoded.bytes.length == 0;
}

export function capabilityAuthorityCannotBeMutatedAfterConstruction(): bool {
  const account = AccountId.fromWords(<u64>0, <u64>0, <u64>0, <u64>1);
  const asset = AssetId.fromWords(<u64>0, <u64>0, <u64>0, <u64>2);
  if (account === null || asset === null) return false;
  const grant = Capability.transfer402(
    changetype<AssetId>(asset),
    changetype<AccountId>(account),
    Amount.fromParts(0, 7)
  );
  const exposed = grant.identifier;
  exposed[31] = 0;
  return grant.identifier[31] == 2 && grant.maximumAmount.equals(Amount.fromParts(0, 7));
}

export function capabilityParityFixtureMatchesRustAndC(): bool {
  if (MAX_CAPABILITY_ENCODING_BYTES != 65535 || MAX_CAPABILITIES != 238 ||
      MAX_CANONICAL_CAPABILITY_SET_BYTES != 65452 ||
      MAX_EVENTS_PER_ACTIVITY != 64) return false;
  const program = ProgramId.fromWords(<u64>0x1111111111111111, <u64>0x1111111111111111,
    <u64>0x1111111111111111, <u64>0x1111111111111111);
  const asset = AssetId.fromWords(<u64>0x2222222222222222, <u64>0x2222222222222222,
    <u64>0x2222222222222222, <u64>0x2222222222222222);
  const account = AccountId.fromWords(<u64>0x3333333333333333, <u64>0x3333333333333333,
    <u64>0x3333333333333333, <u64>0x3333333333333333);
  if (program === null || asset === null || account === null) return false;
  const set = new CapabilitySet(3);
  if (set.insert(Capability.emitEvent()) != 0 ||
      set.insert(Capability.call(changetype<ProgramId>(program))) != 0 ||
      set.insert(Capability.transfer402(changetype<AssetId>(asset),
        changetype<AccountId>(account), Amount.fromParts(0, 7))) != 0) return false;
  const encoded = set.toBytes();
  const expected = mixedFixtureBytes();
  if (!encoded.ok() || encoded.bytes.length != expected.length) return false;
  for (let index = 0; index < expected.length; index++) {
    if (encoded.bytes[index] != expected[index]) return false;
  }
  return callProgram(changetype<ProgramId>(program), new StaticArray<u8>(0),
    new StaticArray<u8>(65536)) == ERR_CAPABILITY_BYTES;
}
