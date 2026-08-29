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
              !signedActivity.isEmpty else { throw programInvalid() }
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
        let id = try identifier(programID); return .init(value: try await client.agentProgramDiscover(
            .object(["program_id": .string(id), "requested_verification_level": .string(try level(verificationLevel))]),
            pathParameters: ["program_id": id]))
    }
    public func interface(programID: Data, verificationLevel: String) async throws -> ProgramInterface {
        let id = try identifier(programID)
        return .init(value: try await client.agentProgramInterface(.object(["program_id": .string(id),
            "requested_verification_level": .string(try level(verificationLevel))]),
            pathParameters: ["program_id": id]))
    }
    public func simulate(_ call: ProgramCall) async throws -> ProgramSimulation {
        .init(value: try await client.agentProgramSimulate(encode(call)))
    }
    public func submit(_ call: ProgramCall, idempotencyKey: IdempotencyKey) async throws -> ProgramSubmission {
        try .init(await client.agentProgramCall(encode(call), idempotencyKey: idempotencyKey))
    }
    public func receipt(idempotencyKey: IdempotencyKey, expectedActivityID: Data, verificationLevel: String) async throws -> ProgramSubmission {
        let activity = try identifier(expectedActivityID)
        return try .init(await client.agentProgramReceipt(.object(["idempotency_key": .string(idempotencyKey.rawValue),
            "expected_activity_id": .string(activity), "requested_verification_level": .string(try level(verificationLevel))]),
            pathParameters: ["idempotency_key": idempotencyKey.rawValue]))
    }
    public func activity(activityID: Data, verificationLevel: String) async throws -> ProgramSubmission {
        let id = try identifier(activityID)
        return try .init(await client.agentProgramActivity(.object(["activity_id": .string(id),
            "requested_verification_level": .string(try level(verificationLevel))]), pathParameters: ["activity_id": id]))
    }

    public static func verifyReceipt(_ canonicalReceipt: Data, authorized: AuthorizedReceiptBatch,
                                     expectedActivityID: Data) async throws -> ReceiptVerification {
        guard expectedActivityID.count == 32 else { throw programInvalid() }
        let verified = try await LocalVerifier.verifyReceiptOutcome(canonicalReceipt, authorized: authorized)
        let receipt = verified.receipt
        guard receipt.protocolVersion > 0, receipt.moduleID == receiptModuleID, receipt.operation == callOperation,
              (1...3).contains(receipt.moduleVersion),
              receipt.activityID == expectedActivityID else { throw programVerification() }
        return verified
    }
}

private func encode(_ call: ProgramCall) -> JSONValue {
    .object(["program_id": .string(call.programID.hex), "calldata": .string(call.calldata.hex),
        "budget": .object(["fuel": .string(String(call.budget.fuel)), "fee_limit": .string(call.budget.feeLimit.decimal)]),
        "capabilities": .array(call.capabilities.map { .string($0.rawValue) }),
        "signed_activity": .string(call.signedActivity.hex)])
}
private func identifier(_ value: Data) throws -> String { guard value.count == 32 else { throw programInvalid() }; return value.hex }
private func level(_ value: String) throws -> String { guard !value.isEmpty, value.utf8.count <= 64 else { throw programInvalid() }; return value }
private func programInvalid() -> PlatformSDKError { .init(code: .invalidArgument, retry: .never) }
private func programVerification() -> PlatformSDKError { .init(code: .verificationFailure, retry: .never) }
private extension Data { var hex: String { map { String(format: "%02x", $0) }.joined() } }
