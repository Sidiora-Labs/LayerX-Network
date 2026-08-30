import Foundation
import XCTest
@testable import LayerXSDK

final class ProgramsContractTests: XCTestCase {
    private final class ProgramTransport: PlatformTransport, @unchecked Sendable {
        let response: JSONValue
        init(_ response: JSONValue) { self.response = response }
        func send(_ call: TransportCall) async throws -> JSONValue {
            throw PlatformSDKError(code: .unavailableCapability, retry: .never)
        }
        func sendProgram(_ call: ProgramTransportCall) async throws -> JSONValue { response }
    }

    func testProgramsClientRequiresIndependentNonzeroSequencerPin() throws {
        let client = PlatformClient(transport: ProgramTransport(.emptyObject))
        XCTAssertThrowsError(try ProgramsClient(client: client, sequencerPublicKey: Data(repeating: 0, count: 32)))
        XCTAssertNoThrow(try ProgramsClient(client: client, sequencerPublicKey: Data(repeating: 1, count: 32)))
    }

    func testPendingReceiptMayOmitRetainedBytesButMustBindExpectedActivity() async throws {
        let key = String(repeating: "a", count: 64)
        let activity = Data(repeating: 0x11, count: 32)
        let value: JSONValue = .object([
            "state": .string("unknown"), "activity_id": .string(activity.hexString),
            "idempotency_key": .string(key),
        ])
        let programs = try ProgramsClient(client: PlatformClient(transport: ProgramTransport(value)),
            sequencerPublicKey: Data(repeating: 1, count: 32))
        let pending = try await programs.receipt(idempotencyKey: IdempotencyKey(key), expectedActivityID: activity,
            verificationLevel: "sequencer-signed")
        XCTAssertTrue(pending.isUnknown)
        XCTAssertNil(pending.retainedSignedActivity)
        do {
            _ = try await programs.receipt(idempotencyKey: IdempotencyKey(key),
                expectedActivityID: Data(repeating: 0x12, count: 32), verificationLevel: "sequencer-signed")
            XCTFail("mismatched activity selector was accepted")
        } catch let error as PlatformSDKError {
            XCTAssertEqual(error.code, .verificationFailure)
        }
    }

    func testOperationValueVerificationStatusMatrixIsExact() {
        let achieved: JSONValue = .object(["state": .string("Achieved"), "level": .string("SequencerSigned")])
        let discovery: JSONValue = .object(["state": .string("Unverified"), "level": .string("SequencerSigned"),
            "reason": .string("server_side_receipt_verification_only")])
        let pending: JSONValue = .object(["state": .string("Unverified"), "level": .string("SequencerSigned"),
            "reason": .string("receipt_pending")])
        let unknown: JSONValue = .object(["state": .string("unknown")])
        XCTAssertTrue(AgentHTTPTransport.validVerification("program.discover", value: .emptyObject, status: discovery))
        XCTAssertFalse(AgentHTTPTransport.validVerification("program.discover", value: .emptyObject, status: achieved))
        XCTAssertTrue(AgentHTTPTransport.validVerification("program.receipt", value: unknown, status: pending))
        XCTAssertFalse(AgentHTTPTransport.validVerification("program.receipt", value: unknown, status: achieved))
        XCTAssertTrue(AgentHTTPTransport.validVerification("program.simulate", value: .emptyObject, status: achieved))
        XCTAssertFalse(AgentHTTPTransport.validVerification("program.simulate", value: .emptyObject, status: discovery))
    }

    func testTransferSetV1AndV2ProduceTheSameCanonicalKernelRoot() throws {
        let v1 = transferAuthorization(version: 1)
        let v2 = transferAuthorization(version: 2)
        let rootV1 = try ProgramsWireTestSupport.authorizationRoot(v1)
        let rootV2 = try ProgramsWireTestSupport.authorizationRoot(v2)
        XCTAssertEqual(rootV1, rootV2)
        XCTAssertTrue(rootV1.contains(where: { $0 != 0 }))
        var mutated = v2; mutated[mutated.count - 33] ^= 1
        XCTAssertNotEqual(try ProgramsWireTestSupport.authorizationRoot(mutated), rootV2)
    }

    func testOccupancyV1V2V3AndAggregateBindings() throws {
        let asset = Data(repeating: 0x66, count: 32)
        for version in 1...3 {
            let binding = try ProgramsWireTestSupport.occupancyBinding(emptyOccupancy(version: version), asset: asset)
            XCTAssertEqual(binding.0, UInt128Value(high: 0, low: 0))
            XCTAssertEqual(binding.1, UInt128Value(high: 0, low: 0))
            XCTAssertEqual(binding.2, Data(repeating: 0, count: 32))
        }
        let evidence = chargedOccupancy()
        let binding = try ProgramsWireTestSupport.occupancyBinding(evidence, asset: asset)
        XCTAssertEqual(binding.0, UInt128Value(high: 0, low: 3))
        XCTAssertEqual(binding.1, UInt128Value(high: 0, low: 6))
        XCTAssertTrue(binding.2.contains(where: { $0 != 0 }))
        var mutated = evidence
        let declaredFeeLowByte = Data("LXP/storage-occupancy-settlement/v3\0".utf8).count + 8 + 4 + 7 * 8 + 16 + 15
        mutated[declaredFeeLowByte] ^= 1
        XCTAssertThrowsError(try ProgramsWireTestSupport.occupancyBinding(mutated, asset: asset))
    }

    private func transferAuthorization(version: Int) -> Data {
        let program = Data(repeating: 1, count: 32); let principal = Data(repeating: 2, count: 32)
        let asset = Data(repeating: 4, count: 32); let destination = Data(repeating: 5, count: 32)
        var encoded = Data("LayerX/programs/402LXP/transfer-set/v\(version)\0".utf8)
        encoded.append(program); encoded.append(principal); encoded.append(Data(repeating: 3, count: 32))
        encoded.append(Data(repeating: 0, count: 9))
        var events = Data("LayerX/programs/events/v1\0".utf8); events.append(be32(0))
        encoded.append(be32(UInt32(events.count))); encoded.append(events); encoded.append(be64(0)); encoded.append(be64(1))
        encoded.append(Data(repeating: 0, count: 9))
        if version == 2 { encoded.append(1); encoded.append(principal) }
        encoded.append(asset); encoded.append(destination); encoded.append(be128(7)); encoded.append(program)
        return encoded
    }

    private func emptyOccupancy(version: Int) -> Data {
        var encoded = Data("LXP/storage-occupancy-settlement/v\(version)\0".utf8); encoded.append(be64(1))
        if version > 1 { encoded.append(be32(1)) }
        for value in 1...7 { encoded.append(be64(UInt64(value))) }
        if version == 3 {
            encoded.append(Data(repeating: 0, count: 16 * 4)); encoded.append(be32(0))
        } else {
            encoded.append(Data(repeating: 0, count: 16 * 2)); encoded.append(be64(0))
        }
        return encoded
    }

    private func chargedOccupancy() -> Data {
        let program = Data(repeating: 0x11, count: 32); let payer = Data(repeating: 0x77, count: 32)
        var encoded = Data("LXP/storage-occupancy-settlement/v3\0".utf8); encoded.append(be64(2)); encoded.append(be32(1))
        for value: UInt64 in [0, 0, 0, 0, 0, 0, 2] { encoded.append(be64(value)) }
        encoded.append(be128(3)); encoded.append(be128(6)); encoded.append(be128(6)); encoded.append(be128(0)); encoded.append(be32(1))
        encoded.append(65); encoded.append(program); encoded.append(0); encoded.append(payer)
        encoded.append(payer); encoded.append(program); encoded.append(Data(repeating: 0x88, count: 32))
        encoded.append(be64(1)); encoded.append(be64(2)); encoded.append(be64(3)); encoded.append(be64(3))
        encoded.append(be128(3)); encoded.append(be64(2)); encoded.append(be128(6)); encoded.append(be128(0))
        encoded.append(be128(6)); encoded.append(be128(0)); encoded.append(1); encoded.append(be128(0))
        encoded.append(be64(3)); encoded.append(be64(2)); encoded.append(be128(0)); encoded.append(Data(repeating: 0x99, count: 32))
        return encoded
    }

    private func be32(_ value: UInt32) -> Data { word(value.bigEndian) }
    private func be64(_ value: UInt64) -> Data { word(value.bigEndian) }
    private func be128(_ value: UInt64) -> Data { Data(repeating: 0, count: 8) + be64(value) }
    private func word<T>(_ value: T) -> Data { var copy = value; return withUnsafeBytes(of: &copy) { Data($0) } }
}

private extension Data {
    var hexString: String { map { String(format: "%02x", $0) }.joined() }
}
