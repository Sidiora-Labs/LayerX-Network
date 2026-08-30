import Foundation
import XCTest
@testable import LayerXSDK

final class ReceiptFixtureTests: XCTestCase {
    private static let programOutcomeV3 = "505247330100000000000100010000000700000001000000000000000b000000000000000c000000000000000d000000000000000e00000001000000000000000f0000000000000000000000000000000000000000000000000000000000000000000000000000000100000000000000020000000000000003000000000000000400000000000000050000000000000006000000000000000700000020000000000000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000010000000201111111111111111111111111111111111111111111111111111111111111111000000202222222222222222222222222222222222222222222222222222222222222222000000200000000000000000000000000000000000000000000000000000000000000000"

    func testProgramOutcomeV3Vector() throws {
        let hex = Self.programOutcomeV3
        let encoded = Data(stride(from: 0, to: hex.count, by: 2).map {
            UInt8(hex[hex.index(hex.startIndex, offsetBy: $0)..<hex.index(hex.startIndex, offsetBy: $0 + 2)], radix: 16)!
        })
        let outcome = try LocalVerifier.decodeProgramReceiptOutcome(encoded, protocolVersion: 1)
        XCTAssertEqual(outcome.encodingVersion, 3)
        XCTAssertEqual(outcome.abiVersion, 1)
        XCTAssertEqual(outcome.feeUnits, UInt128Value(high: 0, low: 16))
        XCTAssertEqual(outcome.callGraphRoot, Data(repeating: 0x11, count: 32))
        XCTAssertEqual(outcome.terminalPayloadRoot, Data(repeating: 0x22, count: 32))
    }
    private struct Fixture {
        let canonicalReceipt: Data
        let batch: AuthorizedReceiptBatch
        let expected: [String: Any]
        let authorizedBatch: [String: Any]
    }

    private func fixtureURL() -> URL {
        var url = URL(fileURLWithPath: #filePath)
        for _ in 0..<6 { url.deleteLastPathComponent() }
        return url
            .appendingPathComponent("platform/sdk/conformance/fixtures")
            .appendingPathComponent("receipt-positive-v1.json")
    }

    private func hexData(_ value: String) throws -> Data {
        XCTAssertEqual(value.count % 2, 0, "odd hex length")
        var bytes = Data(capacity: value.count / 2)
        var index = value.startIndex
        while index < value.endIndex {
            let next = value.index(index, offsetBy: 2)
            let byte = try XCTUnwrap(UInt8(value[index..<next], radix: 16), "invalid hex byte")
            bytes.append(byte)
            index = next
        }
        return bytes
    }

    private func hexField(_ object: [String: Any], _ key: String) throws -> Data {
        try hexData(try XCTUnwrap(object[key] as? String, "missing \(key)"))
    }

    private func u128Field(_ object: [String: Any], _ key: String) throws -> UInt128Value {
        let text = try XCTUnwrap(object[key] as? String, "missing \(key)")
        return UInt128Value(high: 0, low: try XCTUnwrap(UInt64(text), "non-decimal \(key)"))
    }

    private func loadFixture() throws -> Fixture {
        let raw = try Data(contentsOf: fixtureURL())
        let json = try XCTUnwrap(
            try JSONSerialization.jsonObject(with: raw) as? [String: Any], "fixture is not an object")
        let authorizedBatch = try XCTUnwrap(
            json["authorized_batch"] as? [String: Any], "missing authorized_batch")
        let expected = try XCTUnwrap(json["expected"] as? [String: Any], "missing expected")
        let batch = AuthorizedReceiptBatch(
            batchID: try hexField(authorizedBatch, "batch_id_hex"),
            asset: try hexField(authorizedBatch, "asset_hex"),
            previousStateRoot: try hexField(authorizedBatch, "previous_state_root_hex"),
            resultingStateRoot: try hexField(authorizedBatch, "resulting_state_root_hex"),
            sequencerPublicKey: try hexField(authorizedBatch, "sequencer_public_key_hex"))
        return Fixture(
            canonicalReceipt: try hexField(json, "canonical_receipt_hex"),
            batch: batch,
            expected: expected,
            authorizedBatch: authorizedBatch)
    }

    func testCoreFixtureReceiptVerifiesPositively() async throws {
        let fixture = try loadFixture()
        let expected = fixture.expected
        let verified = try await LocalVerifier.verifyReceipt(
            fixture.canonicalReceipt, authorized: fixture.batch)
        XCTAssertEqual(verified.level, try XCTUnwrap(expected["level"] as? String))
        XCTAssertEqual(verified.canonicalBytes, fixture.canonicalReceipt)
        XCTAssertEqual(verified.receiptDigest, try hexField(expected, "receipt_digest_hex"))
        let receipt = verified.receipt
        XCTAssertEqual(
            Int64(receipt.resultCode),
            try XCTUnwrap(expected["result_code"] as? NSNumber).int64Value)
        XCTAssertEqual(
            UInt64(receipt.protocolVersion),
            try XCTUnwrap(expected["protocol_version"] as? NSNumber).uint64Value)
        XCTAssertEqual(
            UInt64(receipt.operation),
            try XCTUnwrap(expected["operation"] as? NSNumber).uint64Value)
        XCTAssertEqual(
            UInt64(receipt.moduleID),
            try XCTUnwrap(expected["module_id"] as? NSNumber).uint64Value)
        XCTAssertEqual(
            receipt.globalSequence,
            try XCTUnwrap(expected["global_sequence"] as? NSNumber).uint64Value)
        XCTAssertEqual(
            receipt.timestamp,
            try XCTUnwrap(expected["timestamp_ms"] as? NSNumber).uint64Value)
        XCTAssertEqual(receipt.amount, try u128Field(expected, "amount"))
        XCTAssertEqual(receipt.feeCharged, try u128Field(expected, "fee_charged"))
        XCTAssertEqual(receipt.fromBalanceBefore, try u128Field(expected, "from_balance_before"))
        XCTAssertEqual(receipt.fromBalanceAfter, try u128Field(expected, "from_balance_after"))
        XCTAssertEqual(receipt.toBalanceBefore, try u128Field(expected, "to_balance_before"))
        XCTAssertEqual(receipt.toBalanceAfter, try u128Field(expected, "to_balance_after"))
        XCTAssertEqual(receipt.activityID, try hexField(expected, "activity_id_hex"))
        XCTAssertEqual(receipt.from, try hexField(expected, "from_hex"))
        XCTAssertEqual(receipt.to, try hexField(expected, "to_hex"))
        XCTAssertEqual(receipt.batchID, try hexField(fixture.authorizedBatch, "batch_id_hex"))
        XCTAssertEqual(receipt.asset, try hexField(fixture.authorizedBatch, "asset_hex"))
        XCTAssertEqual(
            receipt.previousStateRoot,
            try hexField(fixture.authorizedBatch, "previous_state_root_hex"))
        XCTAssertEqual(
            receipt.resultingStateRoot,
            try hexField(fixture.authorizedBatch, "resulting_state_root_hex"))
    }

    func testCoreFixtureReceiptByteFlipFails() async throws {
        let fixture = try loadFixture()
        var mutated = fixture.canonicalReceipt
        mutated[mutated.count - 1] ^= 0x01
        do {
            _ = try await LocalVerifier.verifyReceipt(mutated, authorized: fixture.batch)
            XCTFail("mutated receipt verified; a flipped signature byte must fail")
        } catch {}
    }
}
