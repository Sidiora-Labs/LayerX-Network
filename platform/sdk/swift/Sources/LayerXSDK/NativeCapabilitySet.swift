import Crypto
import Foundation

public enum NativeCapability: Equatable, Sendable {
    case storageRead
    case storageWrite
    case emitEvent
    case call(program: Data)
    case transfer402(asset: Data, to: Data, maximumAmount: UInt128Value)
    case programSpend(ownerProgram: Data, seed: Data, sourceAccount: Data, asset: Data, to: Data, maximumAmount: UInt128Value)
    case receiptRead(receiptDigest: Data)
    case balanceView(account: Data, asset: Data, receiptDigest: Data)
    case sharedStorageRead
    case sharedStorageWrite
}

public enum NativeCapabilitySet {
    public static let maximumGrants = 238
    public static let maximumBytes = 65452
    public static let maximumBalanceViews = 32

    private struct Entry {
        let rank: Int
        let key: [Data]
        let encoded: Data
        let maximum: UInt128Value
        let receipt: Data
    }
    private static func require(_ condition: Bool) throws {
        guard condition else { throw NativeProgramCallError.invalid }
    }
    private static func nonzero(_ value: Data) throws -> Data {
        try require(value.count == 32 && value.contains(where: { $0 != 0 }))
        return Data(value)
    }
    public static func deriveProgramAccount(owner: Data, seed: Data) throws -> Data {
        let program = try nonzero(owner)
        try require(seed.count <= 128)
        var hash = SHA256()
        hash.update(data: Data("LayerX/programs/program-account/v1\0".utf8))
        hash.update(data: program); hash.update(data: word(UInt64(seed.count), 4)); hash.update(data: seed)
        return Data(hash.finalize())
    }
    private static func word(_ value: UInt64, _ length: Int) -> Data {
        Data((0..<length).reversed().map { UInt8(truncatingIfNeeded: value >> ($0 * 8)) })
    }
    private static func entry(_ grant: NativeCapability) throws -> Entry {
        let rank: Int
        var encoded: Data
        let key: [Data]
        var maximum = UInt128Value(high: 0, low: 0)
        var receipt = Data()
        switch grant {
        case .storageRead: rank = 0; encoded = Data([1]); key = []
        case .storageWrite: rank = 1; encoded = Data([2]); key = []
        case .emitEvent: rank = 2; encoded = Data([3]); key = []
        case .call(let program): rank = 3; encoded = Data([4]); key = [try nonzero(program)]
        case .transfer402(let asset, let to, let amount):
            rank = 4; encoded = Data([5]); key = [try nonzero(asset), try nonzero(to)]; maximum = amount
        case .programSpend(let owner, let seed, let source, let asset, let to, let amount):
            rank = 5; encoded = Data([9])
            let program = try nonzero(owner)
            try require(seed.count <= 128)
            try require(deriveProgramAccount(owner: program, seed: seed) == source)
            key = [program, Data(seed), Data(source), try nonzero(asset), try nonzero(to)]; maximum = amount
        case .receiptRead(let digest): rank = 6; encoded = Data([6]); key = [try nonzero(digest)]
        case .balanceView(let account, let asset, let digest):
            rank = 7; encoded = Data([10]); key = [try nonzero(account), try nonzero(asset)]; receipt = try nonzero(digest)
        case .sharedStorageRead: rank = 8; encoded = Data([7]); key = []
        case .sharedStorageWrite: rank = 9; encoded = Data([8]); key = []
        }
        for (index, field) in key.enumerated() {
            if rank == 5 && index == 1 { encoded.append(word(UInt64(field.count), 2)) }
            encoded.append(field)
        }
        if rank == 4 || rank == 5 {
            try require(maximum.high != 0 || maximum.low != 0)
            encoded.append(word(maximum.high, 8)); encoded.append(word(maximum.low, 8))
        }
        encoded.append(receipt)
        return Entry(rank: rank, key: key, encoded: encoded, maximum: maximum, receipt: receipt)
    }
    private static func compare(_ left: Entry, _ right: Entry) -> Int {
        if left.rank != right.rank { return left.rank < right.rank ? -1 : 1 }
        for (first, second) in zip(left.key, right.key) {
            if first != second { return first.lexicographicallyPrecedes(second) ? -1 : 1 }
        }
        return 0
    }
    public static func encode(_ grants: [NativeCapability]) throws -> Data {
        try require(grants.count <= maximumGrants)
        let entries = try grants.map(entry).sorted { compare($0, $1) < 0 }
        try require(entries.filter { $0.rank == 7 }.count <= maximumBalanceViews)
        var output = word(UInt64(entries.count), 2)
        var prior: Entry?
        for value in entries {
            if let prior { try require(compare(prior, value) < 0) }
            output.append(value.encoded); prior = value
        }
        try require(output.count <= maximumBytes)
        return output
    }
    public static func decode(_ encoded: Data) throws -> [NativeCapability] {
        try require(encoded.count >= 2 && encoded.count <= maximumBytes)
        var cursor = Cursor(bytes: Data(encoded)); let count = Int(try cursor.word(2))
        try require(count <= maximumGrants)
        var grants: [NativeCapability] = []
        for _ in 0..<count {
            let grant: NativeCapability
            switch try cursor.word(1) {
            case 1: grant = .storageRead
            case 2: grant = .storageWrite
            case 3: grant = .emitEvent
            case 4: grant = .call(program: try cursor.take(32))
            case 5: grant = .transfer402(asset: try cursor.take(32), to: try cursor.take(32), maximumAmount: try cursor.amount())
            case 6: grant = .receiptRead(receiptDigest: try cursor.take(32))
            case 7: grant = .sharedStorageRead
            case 8: grant = .sharedStorageWrite
            case 9:
                let owner = try cursor.take(32); let length = Int(try cursor.word(2)); try require(length <= 128)
                grant = .programSpend(ownerProgram: owner, seed: try cursor.take(length), sourceAccount: try cursor.take(32), asset: try cursor.take(32), to: try cursor.take(32), maximumAmount: try cursor.amount())
            case 10: grant = .balanceView(account: try cursor.take(32), asset: try cursor.take(32), receiptDigest: try cursor.take(32))
            default: throw NativeProgramCallError.invalid
            }
            grants.append(grant)
        }
        try require(cursor.offset == encoded.count && encode(grants) == encoded)
        return grants
    }
    public static func narrow(_ parent: [NativeCapability], to requested: [NativeCapability]) throws -> [NativeCapability] {
        let parents = try decode(encode(parent)).map(entry)
        let children = try decode(encode(requested))
        for child in children {
            let value = try entry(child)
            guard let ancestor = parents.first(where: { compare($0, value) == 0 }) else { throw NativeProgramCallError.invalid }
            try require(value.maximum.high < ancestor.maximum.high || value.maximum.high == ancestor.maximum.high && value.maximum.low <= ancestor.maximum.low)
            try require(value.receipt == ancestor.receipt)
        }
        return children
    }
    private struct Cursor {
        let bytes: Data
        var offset = 0
        mutating func take(_ length: Int) throws -> Data {
            try require(length >= 0 && length <= bytes.count - offset)
            let value = bytes.subdata(in: offset..<(offset + length)); offset += length; return value
        }
        mutating func word(_ length: Int) throws -> UInt64 {
            try take(length).reduce(0) { ($0 << 8) | UInt64($1) }
        }
        mutating func amount() throws -> UInt128Value {
            UInt128Value(high: try word(8), low: try word(8))
        }
    }
}
