import Crypto
import Foundation

public struct ProgramBudget: Sendable {
    public let fuel: UInt64
    public let feeLimit: ProtocolAmount
    public init(fuel: UInt64, feeLimit: ProtocolAmount) { self.fuel = fuel; self.feeLimit = feeLimit }
}

public enum ProgramCapability: String, Sendable {
    case storageRead = "storage_read"
    case storageWrite = "storage_write"
    case transfer
    case emitEvent = "emit_event"
    case compose

    fileprivate var order: Int {
        switch self {
        case .storageRead: return 1
        case .storageWrite: return 2
        case .transfer: return 3
        case .emitEvent: return 4
        case .compose: return 5
        }
    }
}

public struct ProgramCall: Sendable {
    public let programID: Data; public let calldata: Data; public let budget: ProgramBudget
    public let capabilities: [ProgramCapability]; public let signedActivity: Data

    public init(programID: Data, calldata: Data, budget: ProgramBudget,
                capabilities: [ProgramCapability], signedActivity: Data) throws {
        guard programID.count == 32, budget.fuel > 0, calldata.count <= 1_048_576,
              capabilities.count <= 5,
              zip(capabilities, capabilities.dropFirst()).allSatisfy({ $0.order < $1.order }),
              !signedActivity.isEmpty, signedActivity.count <= 1_048_576 else { throw programInvalid() }
        self.programID = programID; self.calldata = calldata; self.budget = budget
        self.capabilities = capabilities; self.signedActivity = signedActivity
    }
}

public struct ProgramDiscovery: Sendable { public let value: JSONValue }
public struct ProgramInterface: Sendable { public let value: JSONValue }
public struct ProgramSimulation: Sendable { public let value: JSONValue }
public struct ProgramSubmission: Sendable {
    public let value: JSONValue
    public let state: String
    public var isUnknown: Bool { state == "unknown" }
    fileprivate init(_ value: JSONValue) throws {
        guard let state = value.objectValue?["state"]?.stringValue,
              ["refused", "unknown", "executed"].contains(state) else { throw programVerification() }
        self.value = value; self.state = state
    }
}

public struct ProgramsClient: Sendable {
    public static let receiptModuleID: UInt16 = 9
    public static let callOperation: UInt8 = 3
    private let client: PlatformClient
    public init(client: PlatformClient) { self.client = client }

    public func discover(programID: Data, verificationLevel: String) async throws -> ProgramDiscovery {
        let id = try identifier(programID); let value = try await client.agentProgramDiscover(
            .object(["program_id": .string(id), "requested_verification_level": .string(try level(verificationLevel))]),
            pathParameters: ["program_id": id])
        try verifiedDiscovery(value, programID: id, interface: false)
        return .init(value: value)
    }
    public func interface(programID: Data, verificationLevel: String) async throws -> ProgramInterface {
        let id = try identifier(programID)
        let value = try await client.agentProgramInterface(.object(["program_id": .string(id),
            "requested_verification_level": .string(try level(verificationLevel))]),
            pathParameters: ["program_id": id])
        try verifiedDiscovery(value, programID: id, interface: true)
        return .init(value: value)
    }
    public func simulate(_ call: ProgramCall) async throws -> ProgramSimulation {
        let value = try await client.agentProgramSimulate(encode(call))
        try await verifiedSimulation(value, expectedProgramID: call.programID.hex)
        return .init(value: value)
    }
    public func submit(_ call: ProgramCall, idempotencyKey: IdempotencyKey) async throws -> ProgramSubmission {
        guard hex32(idempotencyKey.rawValue) else { throw programInvalid() }
        let value = try await client.agentProgramCall(encode(call), idempotencyKey: idempotencyKey)
        return try await verifiedSubmission(value, expectedProgramID: call.programID.hex,
            expectedActivityID: nil, expectedIdempotencyKey: idempotencyKey.rawValue,
            retainedSignedActivity: call.signedActivity.hex)
    }
    public func receipt(idempotencyKey: IdempotencyKey, expectedActivityID: Data, verificationLevel: String) async throws -> ProgramSubmission {
        let activity = try identifier(expectedActivityID)
        let value = try await client.agentProgramReceipt(.object(["idempotency_key": .string(idempotencyKey.rawValue),
            "expected_activity_id": .string(activity), "requested_verification_level": .string(try level(verificationLevel))]),
            pathParameters: ["idempotency_key": idempotencyKey.rawValue])
        return try await verifiedSubmission(value, expectedProgramID: nil, expectedActivityID: activity,
            expectedIdempotencyKey: idempotencyKey.rawValue, retainedSignedActivity: nil)
    }
    public func activity(activityID: Data, verificationLevel: String) async throws -> ProgramSubmission {
        let id = try identifier(activityID)
        let value = try await client.agentProgramActivity(.object(["activity_id": .string(id),
            "requested_verification_level": .string(try level(verificationLevel))]), pathParameters: ["activity_id": id])
        return try await verifiedSubmission(value, expectedProgramID: nil, expectedActivityID: id,
            expectedIdempotencyKey: nil, retainedSignedActivity: nil)
    }

    public static func verifyReceipt(_ canonicalReceipt: Data, authorized: AuthorizedReceiptBatch,
                                     expectedActivityID: Data, expectedGuestABIVersion: UInt16,
                                     terminalPayload: Data, callGraph: Data) async throws -> ReceiptVerification {
        guard expectedActivityID.count == 32, expectedGuestABIVersion == 1 || expectedGuestABIVersion == 2 else { throw programInvalid() }
        let verified = try await LocalVerifier.verifyReceiptOutcome(canonicalReceipt, authorized: authorized)
        let receipt = verified.receipt
        let outcome = receipt.programOutcome
        guard receipt.protocolVersion > 0, receipt.moduleID == receiptModuleID, receipt.operation == callOperation,
              (1...3).contains(receipt.moduleVersion),
              receipt.activityID == expectedActivityID, let outcome,
              outcome.abiVersion == expectedGuestABIVersion, !callGraph.isEmpty,
              Data(SHA256.hash(data: terminalPayload)) == outcome.terminalPayloadRoot,
              Data(SHA256.hash(data: callGraph)) == outcome.callGraphRoot else { throw programVerification() }
        return verified
    }
}

private func verifiedSubmission(_ value: JSONValue, expectedProgramID: String?, expectedActivityID: String?,
                                expectedIdempotencyKey: String?, retainedSignedActivity: String?) async throws -> ProgramSubmission {
    guard let object = value.objectValue, let state = object["state"]?.stringValue else { throw programVerification() }
    if state == "unknown" {
        let activity = try requiredHex(object, "activity_id", exactBytes: 32)
        let idempotency = try requiredHex(object, "idempotency_key", exactBytes: 32)
        let retained = try requiredHex(object, "retained_signed_activity", maximumBytes: 1_048_576)
        guard expectedActivityID == nil || activity == expectedActivityID,
              expectedIdempotencyKey == nil || idempotency == expectedIdempotencyKey,
              retainedSignedActivity == nil || retained == retainedSignedActivity else { throw programVerification() }
        return try .init(value)
    }
    guard state == "executed" || state == "refused" else { throw programVerification() }
    try await verifiedExecution(object, state: state, expectedProgramID: expectedProgramID,
        expectedActivityID: expectedActivityID, expectedIdempotencyKey: expectedIdempotencyKey)
    return try .init(value)
}

private func verifiedExecution(_ object: [String: JSONValue], state: String, expectedProgramID: String?,
                               expectedActivityID: String?, expectedIdempotencyKey: String?) async throws {
    let activity = try requiredHex(object, "activity_id", exactBytes: 32)
    let program = try requiredHex(object, "program_id", exactBytes: 32)
    guard expectedProgramID == nil || program == expectedProgramID,
          expectedActivityID == nil || activity == expectedActivityID,
          expectedIdempotencyKey == nil || object["idempotency_key"]?.stringValue == expectedIdempotencyKey,
          let guestABI = object["guest_abi_version"]?.integerValue, guestABI == 1 || guestABI == 2,
          let outcome = object["outcome"]?.objectValue, let outcomeKind = outcome["kind"]?.stringValue,
          (state == "refused") == (outcomeKind == "refused") else { throw programVerification() }
    let receipt = try hexData(object, "receipt", maximumBytes: 1_048_576)
    let terminal = try hexData(object, "terminal_payload", maximumBytes: 1_048_576)
    let graph = try hexData(object, "call_graph", maximumBytes: 1_048_576)
    let authority = try authorized(object["authority"])
    _ = try await ProgramsClient.verifyReceipt(receipt, authorized: authority,
        expectedActivityID: Data(hex: activity), expectedGuestABIVersion: UInt16(guestABI),
        terminalPayload: terminal, callGraph: graph)
}

private func verifiedSimulation(_ value: JSONValue, expectedProgramID: String) async throws {
    guard let object = value.objectValue, object["committed"] == .boolean(false),
          let execution = object["execution"]?.objectValue, execution["state"]?.stringValue == "simulated" else {
        throw programVerification()
    }
    try await verifiedExecution(execution, state: "simulated", expectedProgramID: expectedProgramID,
        expectedActivityID: nil, expectedIdempotencyKey: nil)
    guard let evidence = object["simulation_evidence"]?.objectValue, evidence["committed"] == .boolean(false) else {
        throw programVerification()
    }
    let boundary = try hexData(evidence, "boundary_id", exactBytes: 32)
    let publicKey = try hexData(evidence, "public_key", exactBytes: 32)
    var boundaryMaterial = Data("LayerX/emulator/simulation-boundary/v1\0".utf8); boundaryMaterial.append(publicKey)
    guard Data(SHA256.hash(data: boundaryMaterial)) == boundary else { throw programVerification() }
    let activity = try requiredHex(evidence, "activity_id", exactBytes: 32)
    let previous = try hexData(evidence, "previous_state_root", exactBytes: 32)
    let hypothetical = try requiredHex(evidence, "hypothetical_state_root", exactBytes: 32)
    guard activity == execution["activity_id"]?.stringValue, hypothetical == execution["state_root"]?.stringValue,
          let sequence = decimalUInt64(evidence["observed_sequence"]),
          let observedAt = decimalUInt64(evidence["observed_at"]) else { throw programVerification() }
    var signed = Data("LayerX/agent/program-simulation-evidence/v1\0".utf8)
    signed.append(boundary); signed.append(Data(hex: activity)); signed.append(previous); signed.append(Data(hex: hypothetical))
    signed.append(bigEndian(sequence)); signed.append(bigEndian(observedAt)); signed.append(0)
    let digest = Data(SHA256.hash(data: signed))
    let signature = try hexData(evidence, "signature", exactBytes: 64)
    let verifier = try Curve25519.Signing.PublicKey(rawRepresentation: publicKey)
    guard verifier.isValidSignature(signature, for: digest) else { throw programVerification() }
}

private func verifiedDiscovery(_ value: JSONValue, programID: String, interface: Bool) throws {
    guard let object = value.objectValue, object["program_id"]?.stringValue == programID,
          object["verification"]?.stringValue == (interface ? "deployment-interface-and-current-head-verified" : "registry-receipt-and-current-head-verified"),
          decimalUInt64(object["observed_sequence"]) != nil, decimalUInt64(object["observed_at"]) != nil,
          decimalUInt64(object["valid_through"]) != nil else { throw programVerification() }
}

private func authorized(_ value: JSONValue?) throws -> AuthorizedReceiptBatch {
    guard let object = value?.objectValue else { throw programVerification() }
    return .init(batchID: try hexData(object, "batch_id", exactBytes: 32), asset: try hexData(object, "asset", exactBytes: 32),
        previousStateRoot: try hexData(object, "previous_state_root", exactBytes: 32),
        resultingStateRoot: try hexData(object, "resulting_state_root", exactBytes: 32),
        sequencerPublicKey: try hexData(object, "sequencer_public_key", exactBytes: 32))
}

private func requiredHex(_ object: [String: JSONValue], _ name: String, exactBytes: Int? = nil,
                         maximumBytes: Int? = nil) throws -> String {
    guard let value = object[name]?.stringValue, value.utf8.count % 2 == 0,
          exactBytes == nil || value.utf8.count == exactBytes! * 2,
          maximumBytes == nil || value.utf8.count <= maximumBytes! * 2,
          value.utf8.allSatisfy({ ($0 >= 48 && $0 <= 57) || ($0 >= 97 && $0 <= 102) }) else { throw programVerification() }
    return value
}

private func hexData(_ object: [String: JSONValue], _ name: String, exactBytes: Int? = nil,
                     maximumBytes: Int? = nil) throws -> Data {
    Data(hex: try requiredHex(object, name, exactBytes: exactBytes, maximumBytes: maximumBytes))
}

private func decimalUInt64(_ value: JSONValue?) -> UInt64? {
    guard let text = value?.stringValue, !text.isEmpty, text.utf8.allSatisfy({ $0 >= 48 && $0 <= 57 }),
          text == "0" || text.first != "0" else { return nil }
    return UInt64(text)
}

private func bigEndian(_ value: UInt64) -> Data {
    var encoded = value.bigEndian
    return withUnsafeBytes(of: &encoded) { Data($0) }
}

private func encode(_ call: ProgramCall) -> JSONValue {
    .object(["program_id": .string(call.programID.hex), "calldata": .string(call.calldata.hex),
        "budget": .object(["fuel": .string(String(call.budget.fuel)), "fee_limit": .string(call.budget.feeLimit.decimal)]),
        "capabilities": .array(call.capabilities.map { .string($0.rawValue) }),
        "signed_activity": .string(call.signedActivity.hex)])
}
private func identifier(_ value: Data) throws -> String { guard value.count == 32 else { throw programInvalid() }; return value.hex }
private func level(_ value: String) throws -> String { guard value == "sequencer-signed" else { throw programInvalid() }; return value }
private func hex32(_ value: String) -> Bool { value.utf8.count == 64 && value.utf8.allSatisfy { ($0 >= 48 && $0 <= 57) || ($0 >= 97 && $0 <= 102) } }
private func programInvalid() -> PlatformSDKError { .init(code: .invalidArgument, retry: .never) }
private func programVerification() -> PlatformSDKError { .init(code: .verificationFailure, retry: .never) }
private extension Data {
    init(hex: String) { self.init(stride(from: 0, to: hex.count, by: 2).map { index in let start = hex.index(hex.startIndex, offsetBy: index); return UInt8(hex[start..<hex.index(start, offsetBy: 2)], radix: 16)! }) }
    var hex: String { map { String(format: "%02x", $0) }.joined() }
}
