import Crypto
import Foundation
import XCTest
@testable import LayerXSDK

final class ReceiptFixtureTests: XCTestCase {
    func testNativeAccountAuthorizationVectors() throws {
        let raw = try Data(contentsOf: fixtureURL("../../../../programs/fixtures/pay5/account-authorization-vectors.json"))
        let vectors = try XCTUnwrap(JSONSerialization.jsonObject(with: raw) as? [[String: Any]])
        for vector in vectors {
            let encoded = try hexField(vector, "encoded"), root = try hexField(vector, "root")
            if try XCTUnwrap(vector["accept"] as? Bool) {
                XCTAssertEqual(try ProgramsWireTestSupport.authorizationRoot(encoded), root)
            } else {
                do { XCTAssertNotEqual(try ProgramsWireTestSupport.authorizationRoot(encoded), root) }
                catch { }
            }
        }
    }

    func testSignedTerminalV4Vectors() async throws {
        for name in ["executed-v4", "principal-v4", "mutated-leg-v4", "executed-v3", "account-bound-v4"] {
            let raw = try Data(contentsOf: fixtureURL(name == "account-bound-v4" ? "../../../../programs/fixtures/pay5/receipt-account-bound-v4.json" : "receipt-programs-\(name).json"))
            let vector = try XCTUnwrap(JSONSerialization.jsonObject(with: raw) as? [String: Any])
            let batch = try XCTUnwrap(vector["authorized_batch"] as? [String: Any])
            let authority = AuthorizedReceiptBatch(
                batchID: try hexField(batch, "batch_id_hex"), asset: try hexField(batch, "asset_hex"),
                previousStateRoot: try hexField(batch, "previous_state_root_hex"),
                resultingStateRoot: try hexField(batch, "resulting_state_root_hex"),
                sequencerPublicKey: try hexField(batch, "sequencer_public_key_hex"))
            let verified = try await LocalVerifier.verifyReceipt(try hexField(vector, "canonical_receipt_hex"), authorized: authority, protocolVersion: 3)
            XCTAssertEqual(verified.receiptDigest, try hexField(vector, "receipt_digest_hex"), name)
            var activity = Data("LXP/v1/activity-id\0".utf8)
            activity.append(try hexField(vector, "signed_activity_hex"))
            XCTAssertEqual(Data(SHA256.hash(data: activity)), verified.receipt.activityID, name)
            let receipt = try XCTUnwrap(verified.receipt.programOutcome)
            let terminal = try hexField(vector, "terminal_payload_hex")
            let graph = try hexField(vector, "call_graph_hex")
            let program = try hexField(vector, "program_id_hex")
            var outcome: [String: JSONValue] = ["kind": .string("completed"), "code": .integer(0), "response": .string("")]
            if name == "principal-v4" {
                outcome = ["kind": .string("legacy_completed"), "code": .integer(0),
                    "values": .array([.object(["type": .string("i32"), "value": .integer(0)])])]
            }
            if name == "mutated-leg-v4" {
                XCTAssertThrowsError(try unwrapAppliedTerminal(terminal, receipt: receipt))
                XCTAssertThrowsError(try verifyTerminal(terminal, availableGraph: graph, expectedProgram: program,
                    documentOutcome: outcome, protocolVersion: 3, receipt: receipt))
            } else {
                XCTAssertEqual(try verifyTerminal(terminal, availableGraph: graph, expectedProgram: program,
                    documentOutcome: outcome, protocolVersion: 3, receipt: receipt),
                    name == "executed-v3" ? "recorded_terminal_root_not_locally_reconstructable" : "reconstructed", name)
            }
            if name == "executed-v4" || name == "account-bound-v4" {
                for length in 0..<terminal.count {
                    XCTAssertThrowsError(try verifyTerminal(Data(terminal.prefix(length)), availableGraph: graph, expectedProgram: program,
                        documentOutcome: outcome, protocolVersion: 3, receipt: receipt))
                }
                var trailing = terminal; trailing.append(0)
                XCTAssertThrowsError(try verifyTerminal(trailing, availableGraph: graph, expectedProgram: program,
                    documentOutcome: outcome, protocolVersion: 3, receipt: receipt))
            }
        }
        XCTAssertNoThrow(try verifyAppliedLegs(Data(), expected: Data(repeating: 0, count: 32)))
        XCTAssertThrowsError(try verifyAppliedLegs(Data(), expected: Data(repeating: 1, count: 32)))
    }

    func testNativeLifecycleCFixtures() throws {
    let token = try AccessToken(Data("fixture-bearer".utf8))
    defer { token.destroy() }
    let transport = try AgentHTTPTransport(
      baseURL: URL(string: "http://127.0.0.1:8080")!, accessToken: token)
    for name in [
      "deploy", "upgrade", "wind-down-route", "wind-down-deprecate", "wind-down-tombstone",
      "wind-down-exit",
    ] {
      let raw = try Data(contentsOf: fixtureURL("native-program-\(name)-v3.json"))
      let fixture = try XCTUnwrap(JSONSerialization.jsonObject(with: raw) as? [String: Any])
      XCTAssertEqual(fixture["protocol_version"] as? Int, 3)
      XCTAssertEqual(fixture["module"] as? Int, 9)
      let ordinal = UInt16(try XCTUnwrap(fixture["ordinal"] as? Int))
      let payload = try hexField(fixture, "payload_hex")
      let signed = try hexField(fixture, "signed_activity_hex")
      let value = try decodeNativeProgramLifecycle(ordinal, payload)
      XCTAssertEqual(value.encode(), payload)
      let request = try NativeProgramLifecycleRequest(operation: value, signedActivity: signed)
      XCTAssertEqual(request.activityID, try hexField(fixture, "activity_id_hex"))
      XCTAssertEqual(request.idempotencyKey, try hexField(fixture, "idempotency_key_hex"))
      let operation =
        ordinal == 1 ? "program.deploy" : ordinal == 2 ? "program.upgrade" : "program.wind-down"
      let path =
        ordinal == 1
        ? "/v1/programs/deploy" : ordinal == 2 ? "/v1/programs/upgrade" : "/v1/programs/wind-down"
      let hex: (Data) -> String = { $0.map { String(format: "%02x", $0) }.joined() }
      let body: JSONValue = .object([
        "payload": .string(hex(payload)), "signed_activity": .string(hex(signed)),
      ])
      let http = try transport.programRequest(
        .init(
          operation: operation, request: body,
          idempotencyKey: IdempotencyKey(hex(request.idempotencyKey))))
      XCTAssertEqual(http.url?.path, path)
      XCTAssertEqual(http.httpMethod, "POST")
      XCTAssertEqual(http.httpBody, signed)
      XCTAssertEqual(http.value(forHTTPHeaderField: "Content-Type"), "application/octet-stream")
      XCTAssertEqual(http.value(forHTTPHeaderField: "Idempotency-Key"), hex(request.idempotencyKey))
      XCTAssertEqual(http.value(forHTTPHeaderField: "Authorization"), "Bearer fixture-bearer")
      XCTAssertThrowsError(try transport.programRequest(.init(operation: operation, request: body)))
      for length in 0..<payload.count {
        XCTAssertThrowsError(try decodeNativeProgramLifecycle(ordinal, payload.prefix(length)))
      }
      var trailing = payload
      trailing.append(0)
      XCTAssertThrowsError(try decodeNativeProgramLifecycle(ordinal, trailing))
      var changedPayload = payload
      changedPayload[0] ^= 1
      XCTAssertThrowsError(
        try NativeProgramLifecycleRequest(
          operation: decodeNativeProgramLifecycle(ordinal, changedPayload), signedActivity: signed))
      for offset in [1, 7, 17] {
        var changed = signed
        changed[offset] ^= 1
        XCTAssertThrowsError(
          try NativeProgramLifecycleRequest(operation: value, signedActivity: changed))
      }
      for length in 0..<signed.count {
        XCTAssertThrowsError(
          try NativeProgramLifecycleRequest(operation: value, signedActivity: signed.prefix(length))
        )
      }
      if ordinal == 1 || ordinal == 2 {
        for offset in [35, 68, payload.count - 1] {
          var changed = payload
          changed[offset] ^= 1
          XCTAssertThrowsError(try decodeNativeProgramLifecycle(ordinal, changed))
        }
      }
    }
  }

  private static let programOutcomeV3 = "505247330100000000000100010000000700000001000000000000000b000000000000000c000000000000000d000000000000000e00000001000000000000000f0000000000000000000000000000000000000000000000000000000000000000000000000000000100000000000000020000000000000003000000000000000400000000000000050000000000000006000000000000000700000020000000000000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000010000000201111111111111111111111111111111111111111111111111111111111111111000000202222222222222222222222222222222222222222222222222222222222222222000000200000000000000000000000000000000000000000000000000000000000000000"

    func testNativeSignedBinding() throws {
        let raw = try Data(contentsOf: fixtureURL("native-program-call-v3.json"))
        let fixture = try XCTUnwrap(JSONSerialization.jsonObject(with: raw) as? [String: Any])
        let payload = try hexField(fixture, "payload_hex")
        let native = try NativeProgramCall.decode(payload)
        XCTAssertEqual(try native.encode(), payload)
        let signed = try hexField(fixture, "signed_activity_hex")
        let call = try ProgramCall(nativeCall: native, feeLimit: ProtocolAmount("1000"), signedActivity: signed)
        XCTAssertEqual(try decodeSignedCall(call).activityID, try hexField(fixture, "activity_id_hex"))
        let wrongFee = try ProgramCall(nativeCall: native, feeLimit: ProtocolAmount("999"), signedActivity: signed)
        XCTAssertThrowsError(try decodeSignedCall(wrongFee))
        let changed = NativeProgramCall(programID: native.programID, guestABI: native.guestABI, entrypoint: native.entrypoint, calldata: native.calldata, capabilities: native.capabilities, accessDeclaration: native.accessDeclaration, responseCapacity: (native.responseCapacity + 1) % 1_048_577, resources: native.resources)
        XCTAssertNotEqual(native.responseCapacity, changed.responseCapacity)
        _ = try changed.encode()
        XCTAssertThrowsError(try decodeSignedCall(ProgramCall(nativeCall: changed, feeLimit: ProtocolAmount("1000"), signedActivity: signed)))
        for length in 0..<payload.count { XCTAssertThrowsError(try NativeProgramCall.decode(payload.prefix(length))) }
    }

    func testExplicitProtocolThree() async throws {
        for name in ["receipt-positive-v3.json", "receipt-programs-positive-v3.json"] {
            let fixture = try loadFixture(name)
            do { _ = try await LocalVerifier.verifyReceipt(fixture.canonicalReceipt, authorized: fixture.batch); XCTFail("default accepted protocol 3") } catch {}
            let verified = try await LocalVerifier.verifyReceipt(fixture.canonicalReceipt, authorized: fixture.batch, protocolVersion: 3)
      XCTAssertThrowsError(
        try LocalVerifier.verifyProgramLifecycleReceipt(
          fixture.canonicalReceipt, expectedActivity: verified.receipt.activityID,
          sequencer: fixture.batch.sequencerPublicKey))
            XCTAssertEqual(verified.receipt.protocolVersion, 3)
            XCTAssertEqual(verified.receiptDigest, try hexField(fixture.expected, "receipt_digest_hex"))
            var corrupted = fixture.canonicalReceipt; corrupted[corrupted.count - 1] ^= 1
            do { _ = try await LocalVerifier.verifyReceipt(corrupted, authorized: fixture.batch, protocolVersion: 3); XCTFail("corrupt signature accepted") } catch {}
        }
    }

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
