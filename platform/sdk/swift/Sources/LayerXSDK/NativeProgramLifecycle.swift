import Crypto
import Foundation

func nativeTransportHex(_ value: String?) throws -> Data {
  guard let value, !value.isEmpty, value.count <= 2_097_152, value.count % 2 == 0,
    value.utf8.allSatisfy({ (48...57).contains($0) || (97...102).contains($0) })
  else { throw NativeProgramCallError.invalid }
  var bytes = Data()
  var offset = value.startIndex
  while offset < value.endIndex {
    let next = value.index(offset, offsetBy: 2)
    guard let byte = UInt8(value[offset..<next], radix: 16) else {
      throw NativeProgramCallError.invalid
    }
    bytes.append(byte)
    offset = next
  }
  return bytes
}

public protocol NativeProgramLifecycle: Sendable {
  var ordinal: UInt16 { get }
  func encode() -> Data
}

public struct NativeProgramDeploy: NativeProgramLifecycle {
  private let payload: Data
  public var ordinal: UInt16 { 1 }
  public func encode() -> Data { payload }
  public init(
    programID: Data, guestABI: UInt16, policy: UInt8, authority: Data, newHash: Data,
    programInterface: Data? = nil, wasm: Data
  ) throws {
    try lifecycleCode(programID, guestABI, newHash, wasm)
    guard authority.count == 32, policy <= 1,
      (policy == 0) == !authority.contains(where: { $0 != 0 }),
      programInterface == nil || (!programInterface!.isEmpty && programInterface!.count <= 952)
    else { throw NativeProgramCallError.invalid }
    var bytes = programID
    bytes.append(lifecycleWord(UInt64(guestABI), 2))
    bytes.append(contentsOf: [policy, 0])
    bytes.append(authority)
    bytes.append(newHash)
    bytes.append(lifecycleWord(UInt64(wasm.count), 4))
    if let programInterface {
      bytes.append(lifecycleWord(UInt64(programInterface.count), 4))
      bytes.append(programInterface)
    }
    bytes.append(wasm)
    payload = bytes
  }
  public static func decode(_ payload: Data) throws -> NativeProgramDeploy {
    var cursor = LifecycleCursor(payload)
    let program = try cursor.take(32)
    let abi = try cursor.word(2)
    let policy = try cursor.word(1)
    guard try cursor.word(1) == 0 else { throw NativeProgramCallError.invalid }
    let authority = try cursor.take(32)
    let hash = try cursor.take(32)
    let wasmLength = try cursor.word(4)
    var programInterface: Data?
    if wasmLength != UInt64(cursor.remaining) {
      let length = try cursor.word(4)
      guard length > 0, length <= 952, length + wasmLength == UInt64(cursor.remaining) else {
        throw NativeProgramCallError.invalid
      }
      programInterface = try cursor.take(Int(length))
    }
    let value = try NativeProgramDeploy(
      programID: program, guestABI: UInt16(abi), policy: UInt8(policy), authority: authority,
      newHash: hash, programInterface: programInterface, wasm: cursor.take(cursor.remaining))
    guard value.encode() == payload else { throw NativeProgramCallError.invalid }
    return value
  }
}

public struct NativeProgramUpgrade: NativeProgramLifecycle {
  private let payload: Data
  public var ordinal: UInt16 { 2 }
  public func encode() -> Data { payload }
  public init(
    programID: Data, guestABI: UInt16, oldHash: Data, newHash: Data, migrationHook: Data = Data(),
    clearInterface: Bool = false, programInterface: Data? = nil, wasm: Data
  ) throws {
    try lifecycleCode(programID, guestABI, newHash, wasm)
    guard oldHash.count == 32, migrationHook.count <= 65535,
      !clearInterface || programInterface != nil,
      programInterface == nil
        || (programInterface!.count <= 952 && (!programInterface!.isEmpty || clearInterface))
    else { throw NativeProgramCallError.invalid }
    var bytes = programID
    bytes.append(lifecycleWord(UInt64(guestABI), 2))
    bytes.append(contentsOf: [(migrationHook.isEmpty ? 0 : 1) | (clearInterface ? 2 : 0), 0])
    bytes.append(oldHash)
    bytes.append(newHash)
    bytes.append(lifecycleWord(UInt64(migrationHook.count), 2))
    bytes.append(lifecycleWord(UInt64(wasm.count), 4))
    if let programInterface { bytes.append(lifecycleWord(UInt64(programInterface.count), 4)) }
    bytes.append(migrationHook)
    if let programInterface { bytes.append(programInterface) }
    bytes.append(wasm)
    payload = bytes
  }
  public static func decode(_ payload: Data) throws -> NativeProgramUpgrade {
    var cursor = LifecycleCursor(payload)
    let program = try cursor.take(32)
    let abi = try cursor.word(2)
    let flags = try cursor.word(1)
    guard flags & 0xfc == 0, try cursor.word(1) == 0 else { throw NativeProgramCallError.invalid }
    let oldHash = try cursor.take(32)
    let newHash = try cursor.take(32)
    let hook = try cursor.word(2)
    let wasm = try cursor.word(4)
    let clear = flags & 2 != 0
    guard (flags & 1 == 0) == (hook == 0) else { throw NativeProgramCallError.invalid }
    var interfaceLength: Int?
    if clear || hook + wasm != UInt64(cursor.remaining) {
      let length = try cursor.word(4)
      guard length <= 952, length > 0 || clear, hook + length + wasm == UInt64(cursor.remaining)
      else { throw NativeProgramCallError.invalid }
      interfaceLength = Int(length)
    }
    let migration = try cursor.take(Int(hook))
    var programInterface: Data?
    if let interfaceLength { programInterface = try cursor.take(interfaceLength) }
    let value = try NativeProgramUpgrade(
      programID: program, guestABI: UInt16(abi), oldHash: oldHash, newHash: newHash,
      migrationHook: migration,
      clearInterface: clear, programInterface: programInterface, wasm: cursor.take(cursor.remaining)
    )
    guard value.encode() == payload else { throw NativeProgramCallError.invalid }
    return value
  }
}

public struct NativeProgramWindDown: NativeProgramLifecycle {
  private let payload: Data
  public var ordinal: UInt16 { 7 }
  public var operation: UInt8 { payload[32] }
  public func encode() -> Data { payload }
  public static func route(
    programID: Data, account: Data, asset: Data, destination: Data, seed: Data
  ) throws -> NativeProgramWindDown {
    guard programID.count == 32, account.count == 32, asset.count == 32, destination.count == 32,
      seed.count <= 128
    else { throw NativeProgramCallError.invalid }
    var bytes = programID
    bytes.append(1)
    bytes.append(account)
    bytes.append(asset)
    bytes.append(destination)
    bytes.append(lifecycleWord(UInt64(seed.count), 2))
    bytes.append(seed)
    return try decode(bytes)
  }
  public static func deprecate(programID: Data, exitProgram: Data, deadlineBatch: UInt64) throws
    -> NativeProgramWindDown
  {
    guard programID.count == 32, exitProgram.count == 32 else {
      throw NativeProgramCallError.invalid
    }
    var bytes = programID
    bytes.append(2)
    bytes.append(exitProgram)
    bytes.append(lifecycleWord(deadlineBatch, 8))
    return try decode(bytes)
  }
  public static func tombstone(programID: Data) throws -> NativeProgramWindDown {
    guard programID.count == 32 else { throw NativeProgramCallError.invalid }
    var bytes = programID
    bytes.append(3)
    return try decode(bytes)
  }
  public static func exit(programID: Data, account: Data) throws -> NativeProgramWindDown {
    guard programID.count == 32, account.count == 32 else { throw NativeProgramCallError.invalid }
    var bytes = programID
    bytes.append(4)
    bytes.append(account)
    return try decode(bytes)
  }
  public static func decode(_ payload: Data) throws -> NativeProgramWindDown {
    var cursor = LifecycleCursor(payload)
    guard try cursor.take(32).contains(where: { $0 != 0 }) else {
      throw NativeProgramCallError.invalid
    }
    switch try cursor.word(1) {
    case 1:
      _ = try cursor.take(96)
      let seed = try cursor.word(2)
      guard seed <= 128, UInt64(cursor.remaining) == seed else {
        throw NativeProgramCallError.invalid
      }
    case 2: guard cursor.remaining == 40 else { throw NativeProgramCallError.invalid }
    case 3: guard cursor.remaining == 0 else { throw NativeProgramCallError.invalid }
    case 4: guard cursor.remaining == 32 else { throw NativeProgramCallError.invalid }
    default: throw NativeProgramCallError.invalid
    }
    return NativeProgramWindDown(payload: Data(payload))
  }
}

public struct NativeProgramLifecycleRequest: Sendable {
  public let ordinal: UInt16
  public let payload: Data
  public let signedActivity: Data
  public let activityID: Data
  public let idempotencyKey: Data
  public init(operation: any NativeProgramLifecycle, signedActivity: Data) throws {
    ordinal = operation.ordinal
    payload = try decodeNativeProgramLifecycle(ordinal, operation.encode()).encode()
    self.signedActivity = signedActivity
    idempotencyKey = try bindNativeProgramActivity(ordinal, payload, signedActivity)
    activityID = lifecycleHash(Data("LXP/v1/activity-id\0".utf8), signedActivity)
  }
}

func decodeNativeProgramLifecycle(_ ordinal: UInt16, _ payload: Data) throws
  -> any NativeProgramLifecycle
{
  switch ordinal {
  case 1: return try NativeProgramDeploy.decode(payload)
  case 2: return try NativeProgramUpgrade.decode(payload)
  case 7: return try NativeProgramWindDown.decode(payload)
  default: throw NativeProgramCallError.invalid
  }
}

func bindNativeProgramActivity(_ ordinal: UInt16, _ expected: Data, _ signed: Data) throws -> Data {
  guard !signed.isEmpty, signed.count <= 1_048_576 else { throw NativeProgramCallError.invalid }
  var cursor = LifecycleCursor(signed)
  guard try cursor.word(2) == 3, try cursor.word(2) == 0x1001, try cursor.word(1) == 12,
    try cursor.word(1) == 1, try cursor.word(2) == 3, try cursor.word(1) == 2
  else { throw NativeProgramCallError.invalid }
  _ = try cursor.word(4)
  guard try cursor.word(1) == 3, try cursor.word(4) == (0x0009_0000 | UInt64(ordinal)),
    try cursor.word(1) == 4
  else { throw NativeProgramCallError.invalid }
  _ = try cursor.bounded(255)
  try cursor.tag(5)
  _ = try cursor.bounded(524288)
  try cursor.tag(6)
  _ = try cursor.word(8)
  try cursor.tag(7)
  let before = try cursor.word(8)
  let after = try cursor.word(8)
  guard after >= before else { throw NativeProgramCallError.invalid }
  try cursor.tag(8)
  let key = try cursor.bounded(32)
  guard key.count == 32 else { throw NativeProgramCallError.invalid }
  try cursor.tag(9)
  _ = try cursor.take(16)
  try cursor.tag(10)
  let hash = try cursor.bounded(32)
  try cursor.tag(11)
  let payload = try cursor.bounded(524288)
  try cursor.tag(12)
  _ = try cursor.bounded(128)
  guard cursor.remaining == 0, payload == expected,
    hash == lifecycleHash(Data("LXP/v1/payload-hash\0".utf8), payload)
  else { throw NativeProgramCallError.invalid }
  return key
}

private func lifecycleCode(_ program: Data, _ abi: UInt16, _ hash: Data, _ wasm: Data) throws {
  guard program.count == 32, program.contains(where: { $0 != 0 }), (1...3).contains(abi),
    hash.count == 32, wasm.count >= 8, wasm.count <= 1_048_576,
    wasm.prefix(8) == Data([0, 97, 115, 109, 1, 0, 0, 0]), hash == lifecycleHash(wasm)
  else { throw NativeProgramCallError.invalid }
}
private func lifecycleHash(_ parts: Data...) -> Data {
  var hash = SHA256()
  for part in parts { hash.update(data: part) }
  return Data(hash.finalize())
}
private func lifecycleWord(_ value: UInt64, _ length: Int) -> Data {
  Data((0..<length).reversed().map { UInt8(truncatingIfNeeded: value >> ($0 * 8)) })
}
private struct LifecycleCursor {
  private let bytes: Data
  private var offset = 0
  var remaining: Int { bytes.count - offset }
  init(_ bytes: Data) { self.bytes = Data(bytes) }
  mutating func take(_ length: Int) throws -> Data {
    guard length >= 0, length <= remaining else { throw NativeProgramCallError.invalid }
    let value = bytes.subdata(in: offset..<(offset + length))
    offset += length
    return value
  }
  mutating func word(_ length: Int) throws -> UInt64 {
    try take(length).reduce(0) { ($0 << 8) | UInt64($1) }
  }
  mutating func tag(_ value: UInt64) throws {
    guard try word(1) == value else { throw NativeProgramCallError.invalid }
  }
  mutating func bounded(_ maximum: Int) throws -> Data {
    let length = try word(4)
    guard length <= UInt64(maximum) else { throw NativeProgramCallError.invalid }
    return try take(Int(length))
  }
}
