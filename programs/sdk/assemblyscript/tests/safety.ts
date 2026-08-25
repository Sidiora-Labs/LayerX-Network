import {
  AccountId,
  Amount,
  AssetId,
  Capability,
  CapabilitySet,
  ERR_CAPABILITY_LIMIT,
  ProgramId
} from "../assembly/index";

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
