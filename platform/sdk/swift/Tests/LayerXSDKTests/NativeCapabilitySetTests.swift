import Foundation
import XCTest
@testable import LayerXSDK

final class NativeCapabilitySetTests: XCTestCase {
    private func field(_ object: [String: Any], _ name: String) throws -> Data {
        let text = try XCTUnwrap(object[name] as? String)
        XCTAssertEqual(text.utf8.count % 2, 0)
        var result = Data(); var offset = text.startIndex
        while offset < text.endIndex {
            let end = text.index(offset, offsetBy: 2)
            result.append(try XCTUnwrap(UInt8(text[offset..<end], radix: 16))); offset = end
        }
        return result
    }
    private func amount(_ object: [String: Any]) throws -> UInt128Value {
        let text = try XCTUnwrap(object["maximum_amount"] as? String)
        _ = try ProtocolAmount(text)
        var encoded = [UInt8](repeating: 0, count: 16)
        for digit in text.utf8 {
            var carry = Int(digit - 48)
            for index in encoded.indices.reversed() {
                let product = Int(encoded[index]) * 10 + carry
                encoded[index] = UInt8(product & 255); carry = product >> 8
            }
            XCTAssertEqual(carry, 0)
        }
        return .init(high: encoded.prefix(8).reduce(0) { ($0 << 8) | UInt64($1) }, low: encoded.suffix(8).reduce(0) { ($0 << 8) | UInt64($1) })
    }
    private func logicalGrants(_ values: [[String: Any]]) throws -> [NativeCapability] {
        try values.map { value in
            let grant: NativeCapability
            switch try XCTUnwrap(value["kind"] as? String) {
            case "StorageRead": grant = .storageRead
            case "StorageWrite": grant = .storageWrite
            case "EmitEvent": grant = .emitEvent
            case "Call": grant = .call(program: try field(value, "program"))
            case "Transfer402": grant = .transfer402(asset: try field(value, "asset"), to: try field(value, "to"), maximumAmount: try amount(value))
            case "ProgramSpend": grant = .programSpend(ownerProgram: try field(value, "owner_program"), seed: try field(value, "seed"), sourceAccount: try field(value, "source_account"), asset: try field(value, "asset"), to: try field(value, "to"), maximumAmount: try amount(value))
            case "ReceiptRead": grant = .receiptRead(receiptDigest: try field(value, "receipt_digest"))
            case "BalanceView": grant = .balanceView(account: try field(value, "account"), asset: try field(value, "asset"), receiptDigest: try field(value, "receipt_digest"))
            case "SharedStorageRead": grant = .sharedStorageRead
            case "SharedStorageWrite": grant = .sharedStorageWrite
            default: throw NativeProgramCallError.invalid
            }
            XCTAssertEqual(Int(try NativeCapabilitySet.encode([grant])[2]), try XCTUnwrap(value["tag"] as? Int))
            return grant
        }
    }
    private func assertEncoding(_ grants: [NativeCapability], _ expected: Data) throws {
        XCTAssertEqual(try NativeCapabilitySet.encode(grants), expected)
        XCTAssertEqual(try NativeCapabilitySet.encode(NativeCapabilitySet.decode(expected)), expected)
    }
    func testRuntimeFixtureBindsLogicalGrantsAndEscalations() throws {
        var path = URL(fileURLWithPath: #filePath)
        for _ in 0..<6 { path.deleteLastPathComponent() }
        path.appendPathComponent("platform/sdk/conformance/fixtures/native-program-capabilities-v2.json")
        let fixture = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(contentsOf: path)) as? [String: Any])
        let values = try XCTUnwrap(fixture["capabilities"] as? [[String: Any]])
        XCTAssertEqual(try values.map { try XCTUnwrap($0["tag"] as? Int) }, [1, 2, 3, 4, 5, 9, 6, 10, 7, 8])
        let parent = try logicalGrants(values)
        try assertEncoding(parent, field(fixture, "canonical_hex"))
        let requested = try logicalGrants(XCTUnwrap(fixture["narrowed_capabilities"] as? [[String: Any]]))
        try assertEncoding(requested, field(fixture, "narrowed_hex"))
        let narrowed = try NativeCapabilitySet.narrow(parent, to: requested)
        try assertEncoding(narrowed, field(fixture, "narrowed_hex"))
        XCTAssertEqual(fixture["equal_narrowing_accepted"] as? Bool, true)
        try assertEncoding(NativeCapabilitySet.narrow(parent, to: parent), field(fixture, "canonical_hex"))
        let escalations = try XCTUnwrap(fixture["escalation_cases"] as? [[String: Any]])
        XCTAssertEqual(escalations.count, 3)
        for escalation in escalations {
            XCTAssertEqual(escalation["parent"] as? String, "narrowed")
            XCTAssertEqual(escalation["accepted"] as? Bool, false)
            let grants = try logicalGrants(XCTUnwrap(escalation["capabilities"] as? [[String: Any]]))
            try assertEncoding(grants, field(escalation, "canonical_hex"))
            XCTAssertThrowsError(try NativeCapabilitySet.narrow(narrowed, to: grants))
        }
    }

    private func identifier(_ value: UInt8) -> Data { Data(repeating: value, count: 32) }

    func testBoundsAndNarrowingMatchRuntime() throws {
        let owner = identifier(1), asset = identifier(2), to = identifier(3), receipt = identifier(4)
        let seed = Data([0, 255]); let source = try NativeCapabilitySet.deriveProgramAccount(owner: owner, seed: seed)
        let maximum = UInt128Value(high: 1, low: 0)
        let spend = NativeCapability.programSpend(ownerProgram: owner, seed: seed, sourceAccount: source, asset: asset, to: to, maximumAmount: maximum)
        let view = NativeCapability.balanceView(account: source, asset: asset, receiptDigest: receipt)
        let parent: [NativeCapability] = [.sharedStorageWrite, .sharedStorageRead, view, .receiptRead(receiptDigest: receipt), spend,
            .transfer402(asset: asset, to: to, maximumAmount: maximum), .call(program: owner), .emitEvent, .storageWrite, .storageRead]
        let encoded = try NativeCapabilitySet.encode(parent)
        XCTAssertEqual(encoded.prefix(5), Data([0, 10, 1, 2, 3]))
        let reduced = NativeCapability.programSpend(ownerProgram: owner, seed: seed, sourceAccount: source, asset: asset, to: to, maximumAmount: .init(high: 0, low: UInt64.max))
        XCTAssertEqual(try NativeCapabilitySet.narrow(parent, to: [reduced, view]).count, 2)
        let escalated = NativeCapability.programSpend(ownerProgram: owner, seed: seed, sourceAccount: source, asset: asset, to: to, maximumAmount: .init(high: 1, low: 1))
        XCTAssertThrowsError(try NativeCapabilitySet.narrow(parent, to: [escalated]))
        let changedView = NativeCapability.balanceView(account: source, asset: asset, receiptDigest: identifier(5))
        XCTAssertThrowsError(try NativeCapabilitySet.narrow(parent, to: [changedView]))
        XCTAssertThrowsError(try NativeCapabilitySet.encode([view, changedView]))
        XCTAssertThrowsError(try NativeCapabilitySet.narrow([], to: [.storageRead]))
        XCTAssertThrowsError(try NativeCapabilitySet.encode([.transfer402(asset: asset, to: to, maximumAmount: .init(high: 0, low: 0))]))
        XCTAssertThrowsError(try NativeCapabilitySet.encode([.call(program: Data(repeating: 0, count: 32))]))
        XCTAssertThrowsError(try NativeCapabilitySet.encode([.programSpend(ownerProgram: owner, seed: seed, sourceAccount: identifier(9), asset: asset, to: to, maximumAmount: maximum)]))
        XCTAssertThrowsError(try NativeCapabilitySet.encode([.programSpend(ownerProgram: owner, seed: Data(repeating: 0, count: 129), sourceAccount: source, asset: asset, to: to, maximumAmount: maximum)]))
        for length in 0..<encoded.count { XCTAssertThrowsError(try NativeCapabilitySet.decode(encoded.prefix(length))) }
        XCTAssertThrowsError(try NativeCapabilitySet.decode(encoded + Data([0])))
        for malformed: [UInt8] in [[0, 2, 2, 1], [0, 2, 1, 1], [0, 1, 11], [0, 239]] {
            XCTAssertThrowsError(try NativeCapabilitySet.decode(Data(malformed)))
        }
        let views: [NativeCapability] = (1...33).map { .balanceView(account: identifier(UInt8($0)), asset: asset, receiptDigest: receipt) }
        _ = try NativeCapabilitySet.encode(Array(views.prefix(32)))
        XCTAssertThrowsError(try NativeCapabilitySet.encode(views))
        var full: [NativeCapability] = []
        for index in 0...NativeCapabilitySet.maximumGrants {
            var fullSeed = Data(repeating: 0, count: 128); fullSeed[0] = UInt8(index >> 8); fullSeed[1] = UInt8(truncatingIfNeeded: index)
            let account = try NativeCapabilitySet.deriveProgramAccount(owner: owner, seed: fullSeed)
            full.append(.programSpend(ownerProgram: owner, seed: fullSeed, sourceAccount: account, asset: asset, to: to, maximumAmount: maximum))
        }
        XCTAssertEqual(try NativeCapabilitySet.encode(Array(full.prefix(NativeCapabilitySet.maximumGrants))).count, NativeCapabilitySet.maximumBytes)
        XCTAssertThrowsError(try NativeCapabilitySet.encode(full))
    }
}
