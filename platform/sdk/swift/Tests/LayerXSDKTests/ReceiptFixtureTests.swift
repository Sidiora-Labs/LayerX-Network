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

    private func fixtureURL(_ name: String = "receipt-positive-v2.json") -> URL {
        var url = URL(fileURLWithPath: #filePath)
        for _ in 0..<6 { url.deleteLastPathComponent() }
        return url
            .appendingPathComponent("platform/sdk/conformance/fixtures")
            .appendingPathComponent(name)
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

    private func loadFixture(_ name: String = "receipt-positive-v2.json") throws -> Fixture {
        let raw = try Data(contentsOf: fixtureURL(name))
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

    func testProgramsReceiptPreservesOptionalOutcome() async throws {
        let fixture = try loadFixture("receipt-programs-positive-v2.json")
        let verified = try await LocalVerifier.verifyReceipt(
            fixture.canonicalReceipt, authorized: fixture.batch)
        let outcome = try XCTUnwrap(verified.receipt.programOutcome)
        XCTAssertEqual(outcome.encodingVersion, 3)
        XCTAssertEqual(outcome.runtimeVersion, 1)
        XCTAssertEqual(outcome.abiVersion, 1)
        XCTAssertEqual(outcome.occupancyByteBatches, UInt128Value(high: 0, low: 2))
        XCTAssertEqual(outcome.occupancyFeeUnits, UInt128Value(high: 0, low: 7))
        XCTAssertEqual(outcome.occupancyAssetID, fixture.batch.asset)
        XCTAssertNotEqual(outcome.occupancyEvidenceDigest, Data(repeating: 0, count: 32))
        XCTAssertNotEqual(outcome.occupancyTransferRoot, Data(repeating: 0, count: 32))
        XCTAssertEqual(outcome.feeUnits, UInt128Value(high: 0, low: 16))
    }

    func testRefusalVectorsExposeSharedTaxonomy() async throws {
        let raw = try Data(contentsOf: fixtureURL("receipt-refusals-v2.json"))
        let json = try XCTUnwrap(
            try JSONSerialization.jsonObject(with: raw) as? [String: Any])
        let authority = try XCTUnwrap(json["authorized_batch"] as? [String: Any])
        let batch = AuthorizedReceiptBatch(
            batchID: try hexField(authority, "batch_id_hex"),
            asset: try hexField(authority, "asset_hex"),
            previousStateRoot: try hexField(authority, "previous_state_root_hex"),
            resultingStateRoot: try hexField(authority, "resulting_state_root_hex"),
            sequencerPublicKey: try hexField(authority, "sequencer_public_key_hex"))
        let vectors = try XCTUnwrap(json["vectors"] as? [[String: Any]])
        for vector in vectors {
            let name = try XCTUnwrap(vector["name"] as? String)
            let expected = try XCTUnwrap(vector["expected_check"] as? String)
            do {
                _ = try await LocalVerifier.verifyReceipt(
                    try hexField(vector, "canonical_receipt_hex"), authorized: batch)
                XCTFail("\(name) verified")
            } catch let error as PlatformSDKError {
                XCTAssertEqual(error.receiptCheck?.rawValue, expected, name)
            }
        }
    }
}
