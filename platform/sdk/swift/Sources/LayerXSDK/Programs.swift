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
              programID.contains(where: { $0 != 0 }),
              capabilities.count <= 5,
              zip(capabilities, capabilities.dropFirst()).allSatisfy({ $0.order < $1.order }),
              !signedActivity.isEmpty, signedActivity.count <= 1_048_576 else { throw programInvalid() }
        self.programID = programID; self.calldata = calldata; self.budget = budget
        self.capabilities = capabilities; self.signedActivity = signedActivity
    }
}

public enum ProgramLifecycle: String, Sendable { case active, deprecated, tombstoned }
public enum ProgramSource: Sendable {
    case unpublished
    case verified(sourceDigest: Data, environmentDigest: Data, pipeline: String)
    case mismatch(expectedCodeHash: Data, reproducedArtifactDigest: Data)
}
public struct ProgramDiscovery: Sendable {
    public let programID: Data; public let lifecycle: ProgramLifecycle; public let version: UInt32
    public let codeHash: Data; public let abiVersion: UInt16; public let receiptDigest: Data; public let stateRoot: Data
    public let observedSequence: UInt64; public let observedAt: UInt64; public let validThrough: UInt64
    public let verification = "server-side-receipt-verification-only"
}
public struct ProgramInterface: Sendable {
    public let programID: Data; public let version: UInt32; public let codeHash: Data; public let abiVersion: UInt16
    public let interface: Data; public let interfaceDigest: Data; public let receiptDigest: Data; public let stateRoot: Data
    public let observedSequence: UInt64; public let observedAt: UInt64; public let validThrough: UInt64
    public let source: ProgramSource; public let verification = "server-side-receipt-verification-only"
}
public struct VerifiedProgramExecution: Sendable {
    public let value: JSONValue
    public let receipt: ReceiptVerification
    public let terminalPayload: Data
    public let callGraph: Data
    public var guestABIVersion: UInt16 {
        UInt16(value.objectValue?["guest_abi_version"]?.integerValue ?? 0)
    }
    public var terminalPayloadRoot: Data { receipt.receipt.programOutcome?.terminalPayloadRoot ?? Data() }
    public var callGraphRoot: Data { receipt.receipt.programOutcome?.callGraphRoot ?? Data() }
}
public struct ProgramSimulation: Sendable {
    public let value: JSONValue
    public let execution: VerifiedProgramExecution
}
public struct ProgramSubmission: Sendable {
    public let value: JSONValue
    public let state: String
    public let activityID: Data
    public let idempotencyKey: String
    public let retainedSignedActivity: Data?
    public let execution: VerifiedProgramExecution?
    public var isUnknown: Bool { state == "unknown" }
    fileprivate init(_ value: JSONValue, state: String, activityID: Data, idempotencyKey: String,
                     retainedSignedActivity: Data?, execution: VerifiedProgramExecution?) {
        self.value = value; self.state = state; self.activityID = activityID
        self.idempotencyKey = idempotencyKey; self.retainedSignedActivity = retainedSignedActivity
        self.execution = execution
    }
}

public struct ProgramsClient: Sendable {
    public static let receiptModuleID: UInt16 = 9
    public static let callOperation: UInt8 = 3
    fileprivate static let maximumCallGraphBytes = Data("LayerX/programs/call-graph/v1\0".utf8).count + 32 + 16 + 8 + 64 * 68
    private let client: PlatformClient
    private let sequencerPublicKey: Data
    private let nowMilliseconds: @Sendable () -> UInt64
    private let maximumSimulationAgeMilliseconds: UInt64

    public init(client: PlatformClient, sequencerPublicKey: Data,
                maximumSimulationAgeMilliseconds: UInt64 = 300_000,
                nowMilliseconds: @escaping @Sendable () -> UInt64 = {
                    UInt64(Date().timeIntervalSince1970 * 1_000)
                }) throws {
        guard sequencerPublicKey.count == 32, sequencerPublicKey.contains(where: { $0 != 0 }),
              maximumSimulationAgeMilliseconds > 0 else { throw programInvalid() }
        self.client = client; self.sequencerPublicKey = Data(sequencerPublicKey)
        self.maximumSimulationAgeMilliseconds = maximumSimulationAgeMilliseconds
        self.nowMilliseconds = nowMilliseconds
    }

    public func discover(programID: Data, verificationLevel: String) async throws -> ProgramDiscovery {
        let id = try identifier(programID); let value = try await client.program("program.discover", request:
            .object(["program_id": .string(id), "requested_verification_level": .string(try level(verificationLevel))]),
            pathParameters: ["program_id": id])
        return try verifiedDiscovery(value, programID: id, interface: false, now: nowMilliseconds()).discovery!
    }
    public func interface(programID: Data, verificationLevel: String) async throws -> ProgramInterface {
        let id = try identifier(programID)
        let value = try await client.program("program.interface", request: .object(["program_id": .string(id),
            "requested_verification_level": .string(try level(verificationLevel))]),
            pathParameters: ["program_id": id])
        return try verifiedDiscovery(value, programID: id, interface: true, now: nowMilliseconds()).interface!
    }
    public func simulate(_ call: ProgramCall) async throws -> ProgramSimulation {
        let binding = try decodeSignedCall(call)
        let value = try await client.program("program.simulate", request: encode(call))
        let execution = try await verifiedSimulation(value, expectedProgramID: call.programID,
            binding: binding, pinnedKey: sequencerPublicKey, now: nowMilliseconds(),
            maximumAge: maximumSimulationAgeMilliseconds)
        return .init(value: value, execution: execution)
    }
    public func submit(_ call: ProgramCall, idempotencyKey: IdempotencyKey) async throws -> ProgramSubmission {
        guard hex32(idempotencyKey.rawValue) else { throw programInvalid() }
        let binding = try decodeSignedCall(call)
        guard binding.idempotencyKey == Data(hex: idempotencyKey.rawValue) else { throw programInvalid() }
        let value: JSONValue
        do { value = try await client.program("program.call", request: encode(call), idempotencyKey: idempotencyKey) }
        catch let error as PlatformSDKError where error.code == .unknownOutcome {
            return unknownSubmission(activity: binding.activityID, key: idempotencyKey.rawValue, retained: call.signedActivity)
        }
        do {
            return try await verifiedSubmission(value, expectedProgramID: call.programID,
                expectedActivityID: binding.activityID, expectedIdempotencyKey: idempotencyKey.rawValue,
                retainedSignedActivity: call.signedActivity, pinnedKey: sequencerPublicKey)
        } catch let error as PlatformSDKError where error.code == .decodeFailure || error.code == .verificationFailure {
            return unknownSubmission(activity: binding.activityID, key: idempotencyKey.rawValue, retained: call.signedActivity)
        }
    }
    public func receipt(idempotencyKey: IdempotencyKey, expectedActivityID: Data, verificationLevel: String) async throws -> ProgramSubmission {
        let activity = try identifier(expectedActivityID)
        guard hex32(idempotencyKey.rawValue) else { throw programInvalid() }
        let value = try await client.program("program.receipt", request: .object(["idempotency_key": .string(idempotencyKey.rawValue),
            "expected_activity_id": .string(activity), "requested_verification_level": .string(try level(verificationLevel))]),
            pathParameters: ["idempotency_key": idempotencyKey.rawValue])
        return try await verifiedSubmission(value, expectedProgramID: nil, expectedActivityID: expectedActivityID,
            expectedIdempotencyKey: idempotencyKey.rawValue, retainedSignedActivity: nil, pinnedKey: sequencerPublicKey)
    }
    public func activity(activityID: Data, verificationLevel: String) async throws -> ProgramSubmission {
        let id = try identifier(activityID)
        let value = try await client.program("program.activity", request: .object(["activity_id": .string(id),
            "requested_verification_level": .string(try level(verificationLevel))]), pathParameters: ["activity_id": id])
        return try await verifiedSubmission(value, expectedProgramID: nil, expectedActivityID: activityID,
            expectedIdempotencyKey: nil, retainedSignedActivity: nil, pinnedKey: sequencerPublicKey)
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
              outcome.abiVersion == expectedGuestABIVersion, terminalPayload.count <= 1_048_576,
              !callGraph.isEmpty, callGraph.count <= maximumCallGraphBytes,
              Data(SHA256.hash(data: terminalPayload)) == outcome.terminalPayloadRoot,
              Data(SHA256.hash(data: callGraph)) == outcome.callGraphRoot else { throw programVerification() }
        return verified
    }
}

private struct ActivityBinding {
    let activityID: Data
    let idempotencyKey: Data
    let notBefore: UInt64
    let notAfter: UInt64
}

private struct TerminalUsage {
    let cpu: UInt64; let memory: UInt64; let read: UInt64; let write: UInt64
    let values: UInt32; let outputBytes: UInt64; let fee: UInt128Value
}

private struct TerminalAttachments {
    let inner: Data; let occupancy: Data?; let authorization: Data?; let transferRoot: Data?
}

private struct CapabilityKey {
    let order: Int
    let fields: [Data]
}

private struct ProgramAuthorityBinding {
    let owner: Data; let frame: Data; let source: Data; let asset: Data; let destination: Data; let amount: UInt128Value
}

private struct ProgramFundingBinding {
    let owner: Data; let destination: Data; let asset: Data
}

private struct OccupancyChargeBinding {
    let payer: Data; let amountDue: UInt128Value; let paid: Bool; let arrearsAfter: UInt128Value
}

private struct OccupancySettlementBinding {
    let byteBatches: UInt128Value; let feeUnits: UInt128Value; let charges: [OccupancyChargeBinding]
}

private struct StorageNamespaceBinding {
    let canonical: Data; let wire: Data; let program: Data; let principal: Data?
}

private func verifiedSubmission(_ value: JSONValue, expectedProgramID: Data?, expectedActivityID: Data?,
                                expectedIdempotencyKey: String?, retainedSignedActivity: Data?,
                                pinnedKey: Data) async throws -> ProgramSubmission {
    guard let object = value.objectValue, let state = object["state"]?.stringValue else { throw programVerification() }
    if state == "unknown" {
        let retainedPresent = object["retained_signed_activity"] != nil
        try requireFields(object, retainedPresent
            ? ["state", "activity_id", "idempotency_key", "retained_signed_activity"]
            : ["state", "activity_id", "idempotency_key"])
        let activity = try hexData(object, "activity_id", exactBytes: 32)
        let idempotency = try requiredHex(object, "idempotency_key", exactBytes: 32)
        let retained = retainedPresent ? try hexData(object, "retained_signed_activity", maximumBytes: 1_048_576) : nil
        guard expectedActivityID == nil || activity == expectedActivityID,
              expectedIdempotencyKey == nil || idempotency == expectedIdempotencyKey,
              retainedSignedActivity == nil || retained == retainedSignedActivity else { throw programVerification() }
        return .init(value, state: state, activityID: activity, idempotencyKey: idempotency,
            retainedSignedActivity: retained, execution: nil)
    }
    guard state == "executed" || state == "refused" else { throw programDecode() }
    let execution = try await verifiedExecution(object, state: state, idempotent: true,
        expectedProgramID: expectedProgramID, expectedActivityID: expectedActivityID,
        expectedIdempotencyKey: expectedIdempotencyKey, pinnedKey: pinnedKey)
    let outcomeKind = try objectValue(object, "outcome")["kind"]?.stringValue
    guard state == "refused" && outcomeKind == "refused" || state == "executed"
            && (outcomeKind == "completed" || outcomeKind == "legacy_completed") else { throw programVerification() }
    return .init(value, state: state, activityID: try hexData(object, "activity_id", exactBytes: 32),
        idempotencyKey: try text(object, "idempotency_key"), retainedSignedActivity: nil, execution: execution)
}

private func verifiedExecution(_ object: [String: JSONValue], state: String, idempotent: Bool,
                               expectedProgramID: Data?, expectedActivityID: Data?,
                               expectedIdempotencyKey: String?, pinnedKey: Data) async throws -> VerifiedProgramExecution {
    var fields: Set<String> = ["state", "activity_id", "program_id", "guest_abi_version", "module_version",
        "batch_id", "global_sequence", "result_code", "state_root", "receipt", "receipt_digest",
        "terminal_payload", "call_graph", "authority", "usage", "outcome", "verification"]
    if idempotent { fields.insert("idempotency_key") }
    try requireFields(object, fields)
    let activity = try hexData(object, "activity_id", exactBytes: 32)
    let program = try hexData(object, "program_id", exactBytes: 32)
    guard try text(object, "state") == state,
          expectedProgramID == nil || program == expectedProgramID,
          expectedActivityID == nil || activity == expectedActivityID,
          expectedIdempotencyKey == nil || object["idempotency_key"]?.stringValue == expectedIdempotencyKey,
          let guestABI = object["guest_abi_version"]?.integerValue, guestABI == 1 || guestABI == 2,
          let moduleVersion = object["module_version"]?.integerValue, (1...3).contains(moduleVersion),
          try text(object, "verification") == "receipt-terminal-and-call-graph-verified" else { throw programVerification() }
    let resultCode = try integer32(object, "result_code")
    let globalSequence = try decimalUInt64Field(object, "global_sequence")
    let authorityObject = try objectValue(object, "authority")
    try requireFields(authorityObject, ["batch_id", "asset", "previous_state_root", "resulting_state_root", "sequencer_public_key"])
    let authority = try authorized(object["authority"])
    guard authority.batchID == try hexData(object, "batch_id", exactBytes: 32),
          authority.resultingStateRoot == try hexData(object, "state_root", exactBytes: 32),
          authority.sequencerPublicKey == pinnedKey else { throw programVerification() }
    let usage = try objectValue(object, "usage")
    try requireFields(usage, ["cpu_fuel", "memory_bytes", "storage_read_bytes", "storage_write_bytes",
        "output_values", "output_bytes", "fee_units"])
    let cpu = try decimalUInt64Field(usage, "cpu_fuel")
    let memory = try decimalUInt64Field(usage, "memory_bytes")
    let read = try decimalUInt64Field(usage, "storage_read_bytes")
    let write = try decimalUInt64Field(usage, "storage_write_bytes")
    let outputValues = try uint32Field(usage, "output_values")
    let outputBytes = try decimalUInt64Field(usage, "output_bytes")
    let fee = try decimalUInt128Field(usage, "fee_units")
    let outcomeDocument = try objectValue(object, "outcome")
    let outcomeKind = try validateOutcome(outcomeDocument)
    let receipt = try hexData(object, "receipt", maximumBytes: 1_048_576)
    let terminal = try hexData(object, "terminal_payload", maximumBytes: 1_048_576)
    let graph = try hexData(object, "call_graph", maximumBytes: 1_048_576)
    let verified = try await ProgramsClient.verifyReceipt(receipt, authorized: authority,
        expectedActivityID: activity, expectedGuestABIVersion: UInt16(guestABI),
        terminalPayload: terminal, callGraph: graph)
    guard let receiptOutcome = verified.receipt.programOutcome else { throw programVerification() }
    try verifyTerminal(terminal, availableGraph: graph, expectedProgram: program,
        documentOutcome: outcomeDocument, protocolVersion: verified.receipt.protocolVersion, receipt: receiptOutcome)
    let kindMatches = (outcomeKind == "completed" || outcomeKind == "legacy_completed")
        ? receiptOutcome.terminalKind == 1 && try integer32(outcomeDocument, "code") == receiptOutcome.resultCode
        : outcomeKind == "refused" && (receiptOutcome.terminalKind == 2 || receiptOutcome.terminalKind == 3)
    guard verified.receiptDigest == try hexData(object, "receipt_digest", exactBytes: 32),
          verified.receipt.globalSequence == globalSequence, verified.receipt.resultCode == resultCode,
          receiptOutcome.resultCode == resultCode, verified.receipt.moduleVersion == UInt32(moduleVersion),
          receiptOutcome.cpuFuel == cpu, receiptOutcome.memoryBytes == memory,
          receiptOutcome.storageReadBytes == read, receiptOutcome.storageWriteBytes == write,
          receiptOutcome.outputValues == outputValues, receiptOutcome.outputBytes == outputBytes,
          receiptOutcome.feeUnits == fee, kindMatches else { throw programVerification() }
    return .init(value: .object(object), receipt: verified, terminalPayload: terminal, callGraph: graph)
}

private func verifiedSimulation(_ value: JSONValue, expectedProgramID: Data, binding: ActivityBinding,
                                pinnedKey: Data, now: UInt64, maximumAge: UInt64) async throws -> VerifiedProgramExecution {
    guard let object = value.objectValue else { throw programVerification() }
    try requireFields(object, ["committed", "execution", "simulation_evidence"])
    guard object["committed"] == .boolean(false), let execution = object["execution"]?.objectValue else { throw programVerification() }
    let verified = try await verifiedExecution(execution, state: "simulated", idempotent: false,
        expectedProgramID: expectedProgramID, expectedActivityID: binding.activityID,
        expectedIdempotencyKey: nil, pinnedKey: pinnedKey)
    guard let evidence = object["simulation_evidence"]?.objectValue else { throw programVerification() }
    try requireFields(evidence, ["boundary_id", "activity_id", "previous_state_root", "hypothetical_state_root",
        "observed_sequence", "observed_at", "committed", "public_key", "signature"])
    guard evidence["committed"] == .boolean(false) else { throw programVerification() }
    let boundary = try hexData(evidence, "boundary_id", exactBytes: 32)
    let publicKey = try hexData(evidence, "public_key", exactBytes: 32)
    var boundaryMaterial = Data("LayerX/emulator/simulation-boundary/v1\0".utf8); boundaryMaterial.append(publicKey)
    let activity = try hexData(evidence, "activity_id", exactBytes: 32)
    let previous = try hexData(evidence, "previous_state_root", exactBytes: 32)
    let hypothetical = try hexData(evidence, "hypothetical_state_root", exactBytes: 32)
    let authority = try objectValue(execution, "authority")
    let sequence = try decimalUInt64Field(evidence, "observed_sequence")
    let observedAt = try decimalUInt64Field(evidence, "observed_at")
    guard sequence < UInt64.max, activity == binding.activityID,
          activity == try hexData(execution, "activity_id", exactBytes: 32),
          previous == try hexData(authority, "previous_state_root", exactBytes: 32),
          hypothetical == try hexData(authority, "resulting_state_root", exactBytes: 32),
          hypothetical == try hexData(execution, "state_root", exactBytes: 32),
          publicKey == try hexData(authority, "sequencer_public_key", exactBytes: 32), publicKey == pinnedKey,
          try decimalUInt64Field(execution, "global_sequence") == sequence + 1,
          observedAt >= binding.notBefore, observedAt <= binding.notAfter, observedAt <= now,
          now - observedAt <= maximumAge, Data(SHA256.hash(data: boundaryMaterial)) == boundary else { throw programVerification() }
    var signed = Data("LayerX/agent/program-simulation-evidence/v1\0".utf8)
    signed.append(boundary); signed.append(activity); signed.append(previous); signed.append(hypothetical)
    signed.append(bigEndian(sequence)); signed.append(bigEndian(observedAt)); signed.append(0)
    let digest = Data(SHA256.hash(data: signed))
    let signature = try hexData(evidence, "signature", exactBytes: 64)
    let verifier = try Curve25519.Signing.PublicKey(rawRepresentation: publicKey)
    guard verifier.isValidSignature(signature, for: digest) else { throw programVerification() }
    return verified
}

private func verifiedDiscovery(_ value: JSONValue, programID: String, interface: Bool, now: UInt64)
    throws -> (discovery: ProgramDiscovery?, interface: ProgramInterface?) {
    guard let object = value.objectValue else { throw programVerification() }
    let fields: Set<String> = interface
        ? ["program_id", "version", "code_hash", "abi_version", "interface", "interface_digest",
            "receipt_digest", "state_root", "observed_sequence", "observed_at", "valid_through", "source", "verification"]
        : ["program_id", "lifecycle", "version", "code_hash", "abi_version", "receipt_digest", "state_root",
            "observed_sequence", "observed_at", "valid_through", "verification"]
    try requireFields(object, fields)
    let observedAt = try decimalUInt64Field(object, "observed_at")
    let validThrough = try decimalUInt64Field(object, "valid_through")
    guard object["program_id"]?.stringValue == programID, try uint32Field(object, "version") > 0,
          let abi = object["abi_version"]?.integerValue, abi == 1 || abi == 2,
          hex32(try text(object, "code_hash")), hex32(try text(object, "receipt_digest")),
          hex32(try text(object, "state_root")), validThrough >= observedAt, now <= validThrough,
          object["verification"]?.stringValue == (interface ? "deployment-interface-and-current-head-verified" :
            "registry-receipt-and-current-head-verified") else { throw programVerification() }
    let observedSequence = try decimalUInt64Field(object, "observed_sequence")
    let program = try hexData(object, "program_id", exactBytes: 32)
    let codeHash = try hexData(object, "code_hash", exactBytes: 32)
    let receiptDigest = try hexData(object, "receipt_digest", exactBytes: 32)
    let stateRoot = try hexData(object, "state_root", exactBytes: 32)
    let version = try uint32Field(object, "version")
    let abiVersion = UInt16(abi)
    if interface {
        let bytes = try hexData(object, "interface", maximumBytes: 952)
        let interfaceDigest = try hexData(object, "interface_digest", exactBytes: 32)
        guard !bytes.isEmpty, Data(SHA256.hash(data: bytes)) == interfaceDigest else {
            throw programVerification()
        }
        let source = try typedSource(try objectValue(object, "source"))
        return (nil, ProgramInterface(programID: program, version: version, codeHash: codeHash,
            abiVersion: abiVersion, interface: bytes, interfaceDigest: interfaceDigest,
            receiptDigest: receiptDigest, stateRoot: stateRoot, observedSequence: observedSequence,
            observedAt: observedAt, validThrough: validThrough, source: source))
    } else {
        guard let lifecycle = object["lifecycle"]?.stringValue,
              let typedLifecycle = ProgramLifecycle(rawValue: lifecycle) else { throw programDecode() }
        return (ProgramDiscovery(programID: program, lifecycle: typedLifecycle, version: version,
            codeHash: codeHash, abiVersion: abiVersion, receiptDigest: receiptDigest, stateRoot: stateRoot,
            observedSequence: observedSequence, observedAt: observedAt, validThrough: validThrough), nil)
    }
}

private func typedSource(_ source: [String: JSONValue]) throws -> ProgramSource {
    try validateSource(source)
    switch try text(source, "status") {
    case "unpublished": return .unpublished
    case "verified": return .verified(sourceDigest: try hexData(source, "source_digest", exactBytes: 32),
        environmentDigest: try hexData(source, "environment_digest", exactBytes: 32), pipeline: try text(source, "pipeline"))
    case "mismatch": return .mismatch(expectedCodeHash: try hexData(source, "expected_code_hash", exactBytes: 32),
        reproducedArtifactDigest: try hexData(source, "reproduced_artifact_digest", exactBytes: 32))
    default: throw programDecode()
    }
}

private func validateSource(_ source: [String: JSONValue]) throws {
    switch try text(source, "status") {
    case "unpublished": try requireFields(source, ["status"])
    case "verified":
        try requireFields(source, ["status", "source_digest", "environment_digest", "pipeline"])
        _ = try hexData(source, "source_digest", exactBytes: 32)
        _ = try hexData(source, "environment_digest", exactBytes: 32)
        guard try text(source, "pipeline") == "sha256-source-artifact-reproducible-build-v1" else { throw programDecode() }
    case "mismatch":
        try requireFields(source, ["status", "expected_code_hash", "reproduced_artifact_digest"])
        _ = try hexData(source, "expected_code_hash", exactBytes: 32)
        _ = try hexData(source, "reproduced_artifact_digest", exactBytes: 32)
    default: throw programDecode()
    }
}

private func validateOutcome(_ outcome: [String: JSONValue]) throws -> String {
    let kind = try text(outcome, "kind")
    switch kind {
    case "completed":
        try requireFields(outcome, ["kind", "code", "response"])
        _ = try integer32(outcome, "code"); _ = try hexData(outcome, "response", maximumBytes: 1_048_576, empty: true)
    case "legacy_completed":
        try requireFields(outcome, ["kind", "code", "values"]); _ = try integer32(outcome, "code")
        guard case let .array(values)? = outcome["values"], values.count <= 512 else { throw programDecode() }
        try values.forEach(validateLegacyValue)
    case "refused":
        try requireFields(outcome, ["kind", "failure"]); try validateFailure(try objectValue(outcome, "failure"))
    default: throw programDecode()
    }
    return kind
}

private func validateLegacyValue(_ value: JSONValue) throws {
    guard let object = value.objectValue else { throw programDecode() }
    try requireFields(object, ["type", "value"])
    if try text(object, "type") == "i32" { _ = try integer32(object, "value") }
    else if try text(object, "type") == "i64" { _ = try decimalInt64Field(object, "value") }
    else { throw programDecode() }
}

private func validateFailure(_ failure: [String: JSONValue]) throws {
    switch try text(failure, "kind") {
    case "unknown_program", "reentrancy", "authority", "resource", "response", "fault":
        try requireFields(failure, ["kind"])
    case "depth_exceeded", "fanout_exceeded":
        try requireFields(failure, ["kind", "limit", "attempted"])
        _ = try uint32Field(failure, "limit"); _ = try uint32Field(failure, "attempted")
    case "guest_refused":
        try requireFields(failure, ["kind", "code"]); _ = try integer32(failure, "code")
    default: throw programDecode()
    }
}

private func verifyTerminal(_ encoded: Data, availableGraph: Data, expectedProgram: Data,
                            documentOutcome: [String: JSONValue], protocolVersion: UInt16,
                            receipt: ProgramReceiptOutcome) throws {
    do {
        let attachments = try unwrapTerminal(encoded); let inner = attachments.inner
        let candidate = starts(inner, "LXP/program-execution/v4\0"); var successful = false
        if starts(inner, "LXP/program-execution/v2\0") || starts(inner, "LXP/program-execution/v3\0") {
            let traced = starts(inner, "LXP/program-execution/v3\0")
            let domain = Data((traced ? "LXP/program-execution/v3\0" : "LXP/program-execution/v2\0").utf8)
            var cursor = try TerminalCursor(inner, offset: domain.count)
            let runtime = try cursor.u16(); let abi = try cursor.u16(); let metering = try cursor.u32()
            let countValue = try cursor.u128(); guard countValue.high == 0, countValue.low <= UInt64(Int.max) else { throw programVerification() }
            let count = Int(countValue.low)
            guard count <= cursor.remaining / 5,
                  case let .array(values)? = documentOutcome["values"], values.count == count,
                  try text(documentOutcome, "kind") == "legacy_completed", runtime > 0, metering > 0,
                  runtime == receipt.runtimeVersion, abi == 1, abi == receipt.abiVersion,
                  metering == receipt.meteringScheduleVersion else { throw programVerification() }
            for index in 0..<count {
                guard let value = values[index].objectValue else { throw programVerification() }
                let tag = try cursor.u8()
                if tag == 1 {
                    let decoded = try cursor.i32(); let documented = try integer32(value, "value")
                    guard try text(value, "type") == "i32", decoded == documented else { throw programVerification() }
                } else if tag == 2 {
                    let decoded = try cursor.i64(); let documented = try decimalInt64Field(value, "value")
                    guard try text(value, "type") == "i64", decoded == documented else { throw programVerification() }
                } else { throw programVerification() }
            }
            let usage = TerminalUsage(cpu: try cursor.u64(), memory: try cursor.u64(), read: try cursor.u64(),
                write: try cursor.u64(), values: try cursor.u32(), outputBytes: 0, fee: try cursor.u128())
            if traced { guard try cursor.u8() == 1, try cursor.sized64().count <= 34 + 65_536 * 52 else { throw programVerification() } }
            try cursor.finish(); guard receipt.terminalKind == 1, try integer32(documentOutcome, "code") >= 0 else { throw programVerification() }
            try matchUsage(usage, receipt); successful = true
        } else if candidate {
            var cursor = try TerminalCursor(inner, offset: Data("LXP/program-execution/v4\0".utf8).count)
            let runtime = try cursor.u16(); let feeSchedule = try cursor.u32(); let metering = try cursor.u32()
            let countValue = try cursor.u64(); guard countValue <= UInt64(cursor.remaining / 5) else { throw programVerification() }
            for _ in 0..<Int(countValue) {
                let tag = try cursor.u8(); if tag == 1 { _ = try cursor.i32() } else if tag == 2 { _ = try cursor.i64() }
                else { throw programVerification() }
            }
            let usage = TerminalUsage(cpu: try cursor.u64(), memory: try cursor.u64(), read: try cursor.u64(),
                write: try cursor.u64(), values: try cursor.u32(), outputBytes: try cursor.u64(), fee: try cursor.u128())
            let traceTag = try cursor.u8()
            if traceTag == 1 { guard try cursor.sized64().count <= 34 + 65_536 * 52 else { throw programVerification() } }
            else if traceTag != 0 { throw programVerification() }
            let program = try cursor.take(32); let abi = try cursor.u16(); let outcomeTag = try cursor.u8(); let expectedKind: String
            if outcomeTag == 0 {
                let code = try cursor.i32(); let response = try cursor.sized64()
                guard code >= 0, response.count <= 1_048_576, try text(documentOutcome, "kind") == "completed",
                      code == (try integer32(documentOutcome, "code")),
                      response == (try hexData(documentOutcome, "response", maximumBytes: 1_048_576, empty: true)) else { throw programVerification() }
                expectedKind = "completed"; successful = true
            } else if outcomeTag == 1 {
                try validateAuthenticatedProgramFailure(cursor.sized64()); expectedKind = "guest_refused"
            } else if outcomeTag == 2 {
                try validateCandidateResource(&cursor, usage: usage); expectedKind = "resource"
            } else { throw programVerification() }
            let graph = try cursor.sized64(); try cursor.finish()
            guard graph.count <= ProgramsClient.maximumCallGraphBytes, graph == availableGraph, program == expectedProgram, abi == 2,
                  abi == receipt.abiVersion, runtime > 0, feeSchedule > 0, metering > 0,
                  runtime == receipt.runtimeVersion, feeSchedule == receipt.feeScheduleVersion,
                  metering == receipt.meteringScheduleVersion else { throw programVerification() }
            try matchUsage(usage, receipt)
            if outcomeTag == 0 { guard receipt.terminalKind == 1 else { throw programVerification() } }
            else { try requireRefusal(documentOutcome, expected: expectedKind, code: receipt.resultCode); guard receipt.terminalKind != 1 else { throw programVerification() } }
        } else if starts(inner, "LXP/programs/failure-detail/v1\0") {
            var cursor = try TerminalCursor(inner, offset: Data("LXP/programs/failure-detail/v1\0".utf8).count)
            let family = try cursor.u8(); let payload = try cursor.sized32(); try cursor.finish()
            guard (1...4).contains(family), !payload.isEmpty else { throw programVerification() }
            try validateFailureDetail(family, payload); try requireRefusal(documentOutcome, expected: "guest_refused", code: receipt.resultCode)
            guard receipt.terminalKind == 2 else { throw programVerification() }
        } else if starts(inner, "LXP/programs/resource-detail/v1\0") {
            var cursor = try TerminalCursor(inner, offset: Data("LXP/programs/resource-detail/v1\0".utf8).count)
            try validateLegacyResource(&cursor); try cursor.finish(); try requireRefusal(documentOutcome, expected: "resource", code: receipt.resultCode)
            guard receipt.terminalKind == 3 else { throw programVerification() }
        } else if starts(inner, "LXP/programs/settlement-failure/v1\0") {
            var cursor = try TerminalCursor(inner, offset: Data("LXP/programs/settlement-failure/v1\0".utf8).count)
            guard (1...12).contains(try cursor.u8()) else { throw programVerification() }; try cursor.finish()
            try requireRefusal(documentOutcome, expected: "guest_refused", code: receipt.resultCode)
            guard receipt.terminalKind == 2 else { throw programVerification() }
        } else if starts(inner, "LXP/programs/callback-failure/v1\0") {
            var cursor = try TerminalCursor(inner, offset: Data("LXP/programs/callback-failure/v1\0".utf8).count)
            _ = try cursor.u8(); _ = try cursor.i32(); try cursor.finish()
            try requireRefusal(documentOutcome, expected: "guest_refused", code: receipt.resultCode)
            guard receipt.terminalKind == 2 else { throw programVerification() }
        } else { throw programVerification() }
        try verifyTerminalAttachments(attachments, candidate: candidate, successful: successful,
            protocolVersion: protocolVersion, receipt: receipt)
    } catch { throw programVerification() }
}

private func matchUsage(_ usage: TerminalUsage, _ receipt: ProgramReceiptOutcome) throws {
    guard usage.cpu == receipt.cpuFuel, usage.memory == receipt.memoryBytes,
          usage.read == receipt.storageReadBytes, usage.write == receipt.storageWriteBytes,
          usage.values == receipt.outputValues, usage.outputBytes == receipt.outputBytes,
          usage.fee == receipt.feeUnits else { throw programVerification() }
}

private func requireRefusal(_ outcome: [String: JSONValue], expected: String, code: Int32) throws {
    guard try text(outcome, "kind") == "refused" else { throw programVerification() }
    let failure = try objectValue(outcome, "failure")
    guard try text(failure, "kind") == expected,
          expected != "guest_refused" || try integer32(failure, "code") == code else { throw programVerification() }
}

private func unwrapTerminal(_ encoded: Data) throws -> TerminalAttachments {
    var current = encoded; var authorization: Data?; var transferRoot: Data?; var occupancy: Data?
    let authorityDomain = Data("LXP/program-execution-with-transfer-authority/v2\0".utf8)
    let occupancyDomain = Data("LXP/program-execution-with-occupancy/v1\0".utf8)
    if starts(current, authorityDomain) {
        var cursor = try TerminalCursor(current, offset: authorityDomain.count)
        current = try cursor.sized32(); authorization = try cursor.sized32(); transferRoot = try cursor.take(32); try cursor.finish()
    }
    if starts(current, occupancyDomain) {
        var cursor = try TerminalCursor(current, offset: occupancyDomain.count)
        current = try cursor.sized32(); occupancy = try cursor.sized32(); try cursor.finish()
    }
    guard !starts(current, authorityDomain), !starts(current, occupancyDomain) else { throw programVerification() }
    return .init(inner: current, occupancy: occupancy, authorization: authorization, transferRoot: transferRoot)
}

private func verifyTerminalAttachments(_ attachments: TerminalAttachments, candidate: Bool, successful: Bool,
                                       protocolVersion: UInt16, receipt: ProgramReceiptOutcome) throws {
    guard protocolVersion == 1 || protocolVersion == 2 else { throw programVerification() }
    let zero = UInt128Value(high: 0, low: 0)
    let occupancyRequired = protocolVersion == 2 && successful
    guard occupancyRequired == (attachments.occupancy != nil) else { throw programVerification() }
    if let occupancy = attachments.occupancy {
        if occupancy.isEmpty {
            guard receipt.occupancyEvidenceDigest.allSatisfy({ $0 == 0 }),
                  receipt.occupancyTransferRoot.allSatisfy({ $0 == 0 }),
                  receipt.occupancyByteBatches == zero,
                  receipt.occupancyFeeUnits == zero else { throw programVerification() }
        } else {
            guard Data(SHA256.hash(data: occupancy)) == receipt.occupancyEvidenceDigest else { throw programVerification() }
            let settlement = try decodeOccupancySettlement(occupancy)
            guard settlement.byteBatches == receipt.occupancyByteBatches,
                  settlement.feeUnits == receipt.occupancyFeeUnits,
                  try occupancyTransferRoot(settlement, asset: receipt.occupancyAssetID) == receipt.occupancyTransferRoot else {
                throw programVerification()
            }
        }
    } else {
        guard receipt.occupancyEvidenceDigest.allSatisfy({ $0 == 0 }),
              receipt.occupancyTransferRoot.allSatisfy({ $0 == 0 }),
              receipt.occupancyByteBatches == zero, receipt.occupancyFeeUnits == zero else { throw programVerification() }
    }
    let transferPresent = receipt.transferRoot.contains(where: { $0 != 0 })
    guard (candidate ? (attachments.authorization != nil) == transferPresent : attachments.authorization == nil) else {
        throw programVerification()
    }
    if let authorization = attachments.authorization {
        guard !authorization.isEmpty, attachments.transferRoot == receipt.transferRoot else { throw programVerification() }
        try verifyAuthorizationRoot(authorization, expected: receipt.transferRoot)
    }
}

private func verifyAuthorizationRoot(_ encoded: Data, expected: Data) throws {
    guard try decodeAuthorizationRoot(encoded) == expected else { throw programVerification() }
}

private func decodeAuthorizationRoot(_ encoded: Data) throws -> Data {
    let v1 = Data("LayerX/programs/402LXP/transfer-set/v1\0".utf8)
    let v2 = Data("LayerX/programs/402LXP/transfer-set/v2\0".utf8)
    let candidate = starts(encoded, v2); let domain = candidate ? v2 : v1
    var cursor = try TerminalCursor(encoded, offset: 0)
    guard starts(encoded, domain), try cursor.take(domain.count) == domain else { throw programVerification() }
    try requireNonzero(cursor.take(32)); let principal = try cursor.take(32); try requireNonzero(principal)
    try requireNonzero(cursor.take(32)); _ = try decodeFrame(&cursor)
    let eventLength = try cursor.u32(); try decodeEventEnvelope(cursor.take(Int(eventLength)))
    let callCount = try cursor.u64(); guard callCount <= 64 else { throw programVerification() }
    for _ in 0..<Int(callCount) {
        try requireNonzero(cursor.take(32)); try requireNonzero(cursor.take(32)); try requireNonzero(cursor.take(32))
        _ = try decodeFrame(&cursor); _ = try decodeFrame(&cursor)
        let capabilityLength = try cursor.u32(); try decodeCapabilitySet(cursor.take(Int(capabilityLength)), candidate: candidate)
    }
    let legCount = try cursor.u64(); guard legCount > 0, legCount <= 256 else { throw programVerification() }
    var kernelLegs: [Data] = []; var total = UInt128Value(high: 0, low: 0)
    for _ in 0..<Int(legCount) {
        let frame = try decodeFrame(&cursor); var source = principal
        var authority: ProgramAuthorityBinding?; var funding: ProgramFundingBinding?
        if candidate {
            switch try cursor.u8() {
            case 1:
                source = try cursor.take(32); try requireNonzero(source)
                guard source == principal else { throw programVerification() }
            case 2:
                let authorityLength = try cursor.u32(); authority = try decodeProgramAuthority(cursor.take(Int(authorityLength)))
                source = authority!.source
            case 3:
                source = try cursor.take(32); try requireNonzero(source)
                guard source == principal else { throw programVerification() }
                let fundingLength = try cursor.u32(); funding = try decodeProgramFunding(cursor.take(Int(fundingLength)))
            default: throw programVerification()
            }
        }
        let asset = try cursor.take(32); let destination = try cursor.take(32); let amount = try cursor.u128()
        let program = try cursor.take(32); try requireNonzero(asset); try requireNonzero(destination); try requireNonzero(program)
        guard !isZero(amount) else { throw programVerification() }
        if let authority {
            guard authority.owner == program, authority.frame == frame, authority.asset == asset,
                  authority.destination == destination, authority.amount == amount else { throw programVerification() }
        }
        if let funding {
            guard funding.owner == program, funding.destination == destination, funding.asset == asset else {
                throw programVerification()
            }
        }
        total = try checkedAdd(total, amount)
        kernelLegs.append(concatenated([Data([0]), source, destination, asset, bigEndian128(amount), bigEndian(UInt16(1))]))
    }
    try cursor.finish(); _ = total
    return merkleRoot(kernelLegs)
}

private func decodeProgramAuthority(_ encoded: Data) throws -> ProgramAuthorityBinding {
    let domain = Data("LayerX/programs/402LXP/program-authority/v1\0".utf8)
    var cursor = try TerminalCursor(encoded, offset: 0)
    guard try cursor.take(domain.count) == domain else { throw programVerification() }
    let owner = try cursor.take(32); try requireNonzero(owner)
    let seedLength = try cursor.u16(); guard seedLength <= 128 else { throw programVerification() }
    let seed = try cursor.take(Int(seedLength)); let source = try cursor.take(32); let frame = try decodeFrame(&cursor)
    let asset = try cursor.take(32); let destination = try cursor.take(32); let amount = try cursor.u128(); try cursor.finish()
    try requireNonzero(asset); try requireNonzero(destination)
    guard !isZero(amount), deriveProgramAccount(owner: owner, seed: seed) == source else { throw programVerification() }
    return .init(owner: owner, frame: frame, source: source, asset: asset, destination: destination, amount: amount)
}

private func decodeProgramFunding(_ encoded: Data) throws -> ProgramFundingBinding {
    let domain = Data("LayerX/programs/402LXP/program-funding/v1\0".utf8)
    var cursor = try TerminalCursor(encoded, offset: 0)
    guard try cursor.take(domain.count) == domain else { throw programVerification() }
    let owner = try cursor.take(32); try requireNonzero(owner)
    let seedLength = try cursor.u16(); guard seedLength <= 128 else { throw programVerification() }
    let seed = try cursor.take(Int(seedLength)); let destination = try cursor.take(32); let asset = try cursor.take(32)
    try cursor.finish(); try requireNonzero(destination); try requireNonzero(asset)
    guard deriveProgramAccount(owner: owner, seed: seed) == destination else { throw programVerification() }
    return .init(owner: owner, destination: destination, asset: asset)
}

private func deriveProgramAccount(owner: Data, seed: Data) -> Data {
    digest(Data("LayerX/programs/program-account/v1\0".utf8), owner, bigEndian(UInt32(seed.count)), seed)
}

private func decodeEventEnvelope(_ encoded: Data) throws {
    let domain = Data("LayerX/programs/events/v1\0".utf8)
    var cursor = try TerminalCursor(encoded, offset: 0)
    guard try cursor.take(domain.count) == domain else { throw programVerification() }
    let count = try cursor.u32(); guard count <= 64 else { throw programVerification() }
    for _ in 0..<Int(count) {
        try requireNonzero(cursor.take(32)); try requireNonzero(cursor.take(32)); _ = try decodeFrame(&cursor)
        let topic = try cursor.sized32(); let payload = try cursor.sized32()
        guard topic.count <= 64, payload.count <= 65_536 else { throw programVerification() }
    }
    try cursor.finish()
}

private func decodeFrame(_ cursor: inout TerminalCursor) throws -> Data {
    let path = try cursor.take(8); let depth = try cursor.u8(); let pathBytes = [UInt8](path)
    guard depth <= 8,
          pathBytes.prefix(Int(depth)).allSatisfy({ $0 != 0 }),
          pathBytes.dropFirst(Int(depth)).allSatisfy({ $0 == 0 }) else { throw programVerification() }
    return concatenated([path, Data([depth])])
}

private func decodeCapabilitySet(_ encoded: Data, candidate: Bool) throws {
    guard encoded.count >= 2, encoded.count <= 65_535 else { throw programVerification() }
    var cursor = try TerminalCursor(encoded, offset: 0); let count = try cursor.u16()
    guard count <= 269 else { throw programVerification() }
    var prior: CapabilityKey?; var balanceViews = 0
    for _ in 0..<Int(count) {
        let key: CapabilityKey
        switch try cursor.u8() {
        case 1: key = .init(order: 0, fields: [])
        case 2: key = .init(order: 1, fields: [])
        case 3: key = .init(order: 2, fields: [])
        case 4:
            let program = try cursor.take(32); try requireNonzero(program); key = .init(order: 3, fields: [program])
        case 5:
            let asset = try cursor.take(32); let destination = try cursor.take(32); let maximum = try cursor.u128()
            try requireNonzero(asset); try requireNonzero(destination); guard !isZero(maximum) else { throw programVerification() }
            key = .init(order: 4, fields: [asset, destination])
        case 9 where candidate:
            let owner = try cursor.take(32); try requireNonzero(owner); let seedLength = try cursor.u16()
            guard seedLength <= 128 else { throw programVerification() }
            let seed = try cursor.take(Int(seedLength)); let source = try cursor.take(32); let asset = try cursor.take(32)
            let destination = try cursor.take(32); let maximum = try cursor.u128()
            try requireNonzero(asset); try requireNonzero(destination); guard !isZero(maximum), deriveProgramAccount(owner: owner, seed: seed) == source else {
                throw programVerification()
            }
            key = .init(order: 5, fields: [owner, seed, source, asset, destination])
        case 6:
            let receipt = try cursor.take(32); try requireNonzero(receipt); key = .init(order: 6, fields: [receipt])
        case 10 where candidate:
            let account = try cursor.take(32); let asset = try cursor.take(32); let receipt = try cursor.take(32)
            try requireNonzero(account); try requireNonzero(asset); try requireNonzero(receipt)
            balanceViews += 1; guard balanceViews <= 32 else { throw programVerification() }
            key = .init(order: 7, fields: [account, asset])
        case 7: key = .init(order: 8, fields: [])
        case 8: key = .init(order: 9, fields: [])
        default: throw programVerification()
        }
        if let prior { guard compareCapabilityKeys(prior, key) < 0 else { throw programVerification() } }
        prior = key
    }
    try cursor.finish()
}

private func compareCapabilityKeys(_ left: CapabilityKey, _ right: CapabilityKey) -> Int {
    if left.order != right.order { return left.order - right.order }
    for index in 0..<min(left.fields.count, right.fields.count) {
        let order = compareData(left.fields[index], right.fields[index]); if order != 0 { return order }
    }
    return left.fields.count - right.fields.count
}

private func decodeOccupancySettlement(_ encoded: Data) throws -> OccupancySettlementBinding {
    guard encoded.count <= 65_536 else { throw programVerification() }
    let v1 = Data("LXP/storage-occupancy-settlement/v1\0".utf8)
    let v2 = Data("LXP/storage-occupancy-settlement/v2\0".utf8)
    let v3 = Data("LXP/storage-occupancy-settlement/v3\0".utf8)
    if starts(encoded, v1) || starts(encoded, v2) { return try decodeLegacyOccupancy(encoded, v1: v1, v2: v2) }
    var cursor = try TerminalCursor(encoded, offset: 0)
    guard try cursor.take(v3.count) == v3 else { throw programVerification() }
    let batch = try cursor.u64(); let occupancyPrice = try decodeOccupancySchedule(&cursor, versioned: true)
    let declaredUnits = try cursor.u128(); let declaredFee = try cursor.u128()
    let declaredPaid = try cursor.u128(); let declaredArrears = try cursor.u128()
    let count = try cursor.u32(); guard count <= 256 else { throw programVerification() }
    var byteBatches = UInt128Value(high: 0, low: 0); var feeUnits = byteBatches
    var paidUnits = byteBatches; var arrearsUnits = byteBatches; var priorNamespace: Data?
    var charges: [OccupancyChargeBinding] = []
    for _ in 0..<Int(count) {
        let namespace = try decodeStorageNamespace(&cursor)
        if let priorNamespace { guard compareData(priorNamespace, namespace.canonical) < 0 else { throw programVerification() } }
        priorNamespace = namespace.canonical
        let payer = try cursor.take(32); try requireNonzero(payer)
        if let principal = namespace.principal { guard principal == payer else { throw programVerification() } }
        let rootProgram = try cursor.take(32); try requireNonzero(rootProgram)
        let activity = try cursor.take(32); let fromBatch = try cursor.u64(); let toBatch = try cursor.u64()
        let recordedBytes = try cursor.u64(); let finalBytes = try cursor.u64(); let units = try cursor.u128()
        let price = try cursor.u64(); let accrued = try cursor.u128(); let priorArrears = try cursor.u128()
        let amountDue = try cursor.u128(); let authorizedAdded = try cursor.u128(); let disposition = try cursor.u8()
        guard (1...5).contains(disposition) else { throw programVerification() }
        let arrearsAfter = try cursor.u128(); let maximumBytes = try cursor.u64(); let maximumPrice = try cursor.u64()
        _ = try cursor.u128(); let mandate = try cursor.take(32)
        guard toBatch >= fromBatch else { throw programVerification() }
        let expectedUnits = try checkedMultiply(recordedBytes, toBatch - fromBatch)
        let expectedFee = try checkedMultiply(expectedUnits, price)
        let expectedDue = try checkedAdd(priorArrears, expectedFee); let migration = disposition == 5
        guard toBatch == batch, migration || price == occupancyPrice, units == expectedUnits, accrued == expectedFee,
              amountDue == expectedDue, finalBytes <= maximumBytes,
              migration || (mandate.contains(where: { $0 != 0 }) && activity.contains(where: { $0 != 0 })),
              !migration || (price == 0 && isZero(accrued) && isZero(priorArrears) && isZero(amountDue)
                && isZero(arrearsAfter) && mandate.allSatisfy({ $0 == 0 }) && activity.allSatisfy({ $0 == 0 })
                && rootProgram == namespace.program),
              (disposition == 4) == (price > maximumPrice),
              disposition != 1 || isZero(arrearsAfter), disposition == 1 || arrearsAfter == amountDue else {
            throw programVerification()
        }
        if !isZero(authorizedAdded) {
            let expectedMandate = digest(Data("LXP/storage-occupancy-mandate/v1\0".utf8), payer, rootProgram,
                activity, namespace.wire, bigEndian(maximumBytes), bigEndian(maximumPrice), bigEndian128(authorizedAdded))
            guard mandate == expectedMandate else { throw programVerification() }
        }
        byteBatches = try checkedAdd(byteBatches, units); feeUnits = try checkedAdd(feeUnits, accrued)
        if disposition == 1 { paidUnits = try checkedAdd(paidUnits, amountDue) }
        else { arrearsUnits = try checkedAdd(arrearsUnits, arrearsAfter) }
        charges.append(.init(payer: payer, amountDue: amountDue, paid: disposition == 1, arrearsAfter: arrearsAfter))
    }
    try cursor.finish()
    guard byteBatches == declaredUnits, feeUnits == declaredFee, paidUnits == declaredPaid,
          arrearsUnits == declaredArrears else { throw programVerification() }
    return .init(byteBatches: byteBatches, feeUnits: feeUnits, charges: charges)
}

private func decodeLegacyOccupancy(_ encoded: Data, v1: Data, v2: Data) throws -> OccupancySettlementBinding {
    let versioned = starts(encoded, v2); let domain = versioned ? v2 : v1
    var cursor = try TerminalCursor(encoded, offset: 0)
    guard try cursor.take(domain.count) == domain else { throw programVerification() }
    let batch = try cursor.u64(); let occupancyPrice = try decodeOccupancySchedule(&cursor, versioned: versioned)
    let declaredUnits = try cursor.u128(); let declaredFee = try cursor.u128(); let count = try cursor.u64()
    guard count <= 256 else { throw programVerification() }
    var byteBatches = UInt128Value(high: 0, low: 0); var feeUnits = byteBatches; var charges: [OccupancyChargeBinding] = []
    for _ in 0..<Int(count) {
        _ = try decodeStorageNamespace(&cursor); let payer = try cursor.take(32); try requireNonzero(payer)
        let fromBatch = try cursor.u64(); let toBatch = try cursor.u64(); let recordedBytes = try cursor.u64(); _ = try cursor.u64()
        let units = try cursor.u128(); let price = try cursor.u64(); let accrued = try cursor.u128()
        guard toBatch >= fromBatch else { throw programVerification() }
        let expectedUnits = try checkedMultiply(recordedBytes, toBatch - fromBatch)
        let expectedAccrued = try checkedMultiply(expectedUnits, price)
        guard toBatch == batch, units == expectedUnits, price == occupancyPrice,
              accrued == expectedAccrued else { throw programVerification() }
        byteBatches = try checkedAdd(byteBatches, units); feeUnits = try checkedAdd(feeUnits, accrued)
        charges.append(.init(payer: payer, amountDue: accrued, paid: true, arrearsAfter: .init(high: 0, low: 0)))
    }
    try cursor.finish(); guard byteBatches == declaredUnits, feeUnits == declaredFee else { throw programVerification() }
    return .init(byteBatches: byteBatches, feeUnits: feeUnits, charges: charges)
}

private func decodeOccupancySchedule(_ cursor: inout TerminalCursor, versioned: Bool) throws -> UInt64 {
    let version: UInt32 = versioned ? try cursor.u32() : 1; guard version != 0 else { throw programVerification() }
    var occupancyPrice: UInt64 = 0; for _ in 0..<7 { occupancyPrice = try cursor.u64() }; return occupancyPrice
}

private func decodeStorageNamespace(_ cursor: inout TerminalCursor) throws -> StorageNamespaceBinding {
    let length = try cursor.u8(); guard length == 33 || length == 65 else { throw programVerification() }
    let canonical = try cursor.take(Int(length)); let program = Data(canonical.prefix(32)); try requireNonzero(program)
    let tag = canonical[canonical.index(canonical.startIndex, offsetBy: 32)]; var principal: Data?
    if tag == 0 && length == 65 {
        principal = Data(canonical.dropFirst(33)); try requireNonzero(principal!)
    } else if !(tag == 1 && length == 33) && !(tag == 2 && length == 65) { throw programVerification() }
    return .init(canonical: canonical, wire: concatenated([Data([length]), canonical]), program: program, principal: principal)
}

private func occupancyTransferRoot(_ settlement: OccupancySettlementBinding, asset: Data) throws -> Data {
    guard asset.count == 32 else { throw programVerification() }; try requireNonzero(asset)
    var payers: [Data: (due: UInt128Value, paid: UInt128Value, arrears: UInt128Value)] = [:]
    let zero = UInt128Value(high: 0, low: 0)
    for charge in settlement.charges {
        var entry = payers[charge.payer] ?? (zero, zero, zero)
        entry.due = try checkedAdd(entry.due, charge.amountDue)
        if charge.paid { entry.paid = try checkedAdd(entry.paid, charge.amountDue) }
        entry.arrears = try checkedAdd(entry.arrears, charge.arrearsAfter); payers[charge.payer] = entry
    }
    let treasury = digest(Data("LX:ACCOUNT:v1".utf8), bigEndian(UInt32(11)), Data("system:fees".utf8))
    var legs: [Data] = []
    for payer in payers.keys.sorted(by: { compareData($0, $1) < 0 }) {
        let entry = payers[payer]!
        if !isZero(entry.due) || !isZero(entry.arrears), !isZero(entry.paid) {
            legs.append(concatenated([Data([0]), payer, treasury, asset, bigEndian128(entry.paid), bigEndian(UInt16(23))]))
        }
    }
    return merkleRoot(legs)
}

private func merkleRoot(_ legs: [Data]) -> Data {
    guard !legs.isEmpty else { return Data(repeating: 0, count: 32) }
    var level = legs.map { digest(Data("LXP/v1/merkle-leaf\0".utf8), $0) }
    while level.count > 1 {
        var next: [Data] = []; var index = 0
        while index < level.count {
            let right = index + 1 < level.count ? level[index + 1] : level[index]
            next.append(digest(Data("LXP/v1/merkle-internal\0".utf8), level[index], right)); index += 2
        }
        level = next
    }
    return level[0]
}

private func checkedAdd(_ left: UInt128Value, _ right: UInt128Value) throws -> UInt128Value {
    let (low, carry) = left.low.addingReportingOverflow(right.low)
    let (highValue, overflow) = left.high.addingReportingOverflow(right.high)
    let (high, carryOverflow) = highValue.addingReportingOverflow(carry ? 1 : 0)
    guard !overflow, !carryOverflow else { throw programVerification() }; return .init(high: high, low: low)
}

private func checkedMultiply(_ left: UInt64, _ right: UInt64) throws -> UInt128Value {
    let product = left.multipliedFullWidth(by: right); return .init(high: product.high, low: product.low)
}

private func checkedMultiply(_ left: UInt128Value, _ right: UInt64) throws -> UInt128Value {
    let lowProduct = left.low.multipliedFullWidth(by: right)
    let (highProduct, overflow) = left.high.multipliedReportingOverflow(by: right)
    let (high, carryOverflow) = highProduct.addingReportingOverflow(lowProduct.high)
    guard !overflow, !carryOverflow else { throw programVerification() }; return .init(high: high, low: lowProduct.low)
}

private func isZero(_ value: UInt128Value) -> Bool { value.high == 0 && value.low == 0 }

private func concatenated(_ values: [Data]) -> Data {
    var result = Data(); values.forEach { result.append($0) }; return result
}

private func compareData(_ left: Data, _ right: Data) -> Int {
    let leftBytes = [UInt8](left); let rightBytes = [UInt8](right)
    for index in 0..<min(leftBytes.count, rightBytes.count) {
        if leftBytes[index] != rightBytes[index] { return Int(leftBytes[index]) - Int(rightBytes[index]) }
    }
    return leftBytes.count - rightBytes.count
}

enum ProgramsWireTestSupport {
    static func authorizationRoot(_ encoded: Data) throws -> Data {
        try decodeAuthorizationRoot(encoded)
    }

    static func occupancyBinding(_ encoded: Data, asset: Data) throws -> (UInt128Value, UInt128Value, Data) {
        let settlement = try decodeOccupancySettlement(encoded)
        return (settlement.byteBatches, settlement.feeUnits, try occupancyTransferRoot(settlement, asset: asset))
    }
}

private func validateAuthenticatedProgramFailure(_ encoded: Data) throws {
    var cursor = try TerminalCursor(encoded, offset: 0); let program = try cursor.take(32)
    let failureClass = try cursor.u32(); let reason = try cursor.sized32(); try cursor.finish()
    guard program.contains(where: { $0 != 0 }), [1, 2, 3, 4, 5, 254, 255].contains(failureClass),
          reason.count <= 4_096, ![254, 255].contains(failureClass) || reason.isEmpty else { throw programVerification() }
}

private func validateFailureDetail(_ family: UInt8, _ encoded: Data) throws {
    if family == 1 { try validateAuthenticatedProgramFailure(encoded); return }
    var cursor = try TerminalCursor(encoded, offset: 0); let tag = try cursor.u8()
    if family == 2 { try validateCompositionFailure(&cursor, tag: tag) }
    else if family == 3 { try validateEntrypointFailure(&cursor, tag: tag) }
    else if family == 4 { try validateABIFailure(&cursor, tag: tag) }
    else { throw programVerification() }
    try cursor.finish()
}

private func validateCompositionFailure(_ cursor: inout TerminalCursor, tag: UInt8) throws {
    switch tag {
    case 1, 9, 10, 11, 20, 21, 22: break
    case 2:
        guard (1...2).contains(try cursor.u8()), (1...2).contains(try cursor.u8()) else { throw programVerification() }
    case 3, 4: try requireNonzero(cursor.take(32))
    case 5, 6, 7: _ = try cursor.u32(); _ = try cursor.u32()
    case 8: try requireNonzero(cursor.take(32)); _ = try cursor.u32(); _ = try cursor.u32()
    case 12: _ = try cursor.i32()
    case 13: _ = try cursor.u64(); _ = try cursor.u64()
    case 14: try requireNonzero(cursor.take(32)); _ = try cursor.i32()
    case 15: try validateAuthenticatedProgramFailure(cursor.rest())
    case 16:
        let nested = try cursor.u8(); try validateABIFailure(&cursor, tag: nested)
    case 17: try validateFault(&cursor)
    case 18: try validateMeterFailure(&cursor)
    case 19: try validateResponseFailure(&cursor)
    case 23: _ = try cursor.take(76); _ = try cursor.take(76)
    default: throw programVerification()
    }
}

private func validateEntrypointFailure(_ cursor: inout TerminalCursor, tag: UInt8) throws {
    switch tag {
    case 1: _ = try cursor.u64(); _ = try cursor.u64()
    case 2, 3, 4: break
    case 5, 6: _ = try cursor.i32()
    case 7: try validateFault(&cursor)
    case 8: try validateMeterFailure(&cursor)
    default: throw programVerification()
    }
}

private func validateABIFailure(_ cursor: inout TerminalCursor, tag: UInt8) throws {
    if (1...10).contains(tag) || (13...15).contains(tag) { return }
    if tag == 11 { guard (1...11).contains(try cursor.u8()) else { throw programVerification() } }
    else if tag == 12 { try validateMeterFailure(&cursor) }
    else { throw programVerification() }
}

private func validateMeterFailure(_ cursor: inout TerminalCursor) throws {
    let tag = try cursor.u8()
    if tag == 1 {
        let resource = try cursor.u8(); let limit = try cursor.u64(); let attempted = try cursor.u64()
        guard (1...7).contains(resource), attempted > limit else { throw programVerification() }
    } else if tag == 2 {
        guard (1...7).contains(try cursor.u8()) else { throw programVerification() }
    } else if tag != 3 { throw programVerification() }
}

private func validateFault(_ cursor: inout TerminalCursor) throws {
    let tag = try cursor.u8()
    if tag == 1 || tag == 2 || tag == 16 {
        guard String(data: try cursor.sized32(), encoding: .utf8) != nil else { throw programVerification() }
    } else if (3...13).contains(tag) || tag == 15 { return }
    else if tag == 14 { try validateMeterFailure(&cursor) }
    else { throw programVerification() }
}

private func validateResponseFailure(_ cursor: inout TerminalCursor) throws {
    switch try cursor.u8() {
    case 1, 2: _ = try cursor.u64(); _ = try cursor.u64()
    case 3, 4: break
    case 5: _ = try cursor.i32(); _ = try cursor.i32()
    case 6: try validateMeterFailure(&cursor)
    default: throw programVerification()
    }
}

private func validateCandidateResource(_ cursor: inout TerminalCursor, usage: TerminalUsage) throws {
    let tag = try cursor.u8(); let resource = try cursor.u8(); guard resource <= 6 else { throw programVerification() }
    if tag == 0 {
        let limit = try cursor.u64(); let attempted = try cursor.u64()
        guard attempted > limit, candidateUsage(usage, resource: resource) <= limit else { throw programVerification() }
    } else if tag != 1 { throw programVerification() }
}

private func candidateUsage(_ usage: TerminalUsage, resource: UInt8) -> UInt64 {
    switch resource {
    case 0: return usage.cpu
    case 1: return usage.memory
    case 2: return usage.read
    case 3: return usage.write
    case 4: return UInt64(usage.values)
    case 5: return usage.outputBytes
    default: return 0
    }
}

private func validateLegacyResource(_ cursor: inout TerminalCursor) throws {
    let tag = try cursor.u8(); guard (1...7).contains(try cursor.u8()) else { throw programVerification() }
    if tag == 1 { let limit = try cursor.u64(); guard try cursor.u64() > limit else { throw programVerification() } }
    else if tag != 2 { throw programVerification() }
}

private func requireNonzero(_ value: Data) throws {
    guard value.contains(where: { $0 != 0 }) else { throw programVerification() }
}

private func decodeSignedCall(_ call: ProgramCall) throws -> ActivityBinding {
    do {
        var cursor = BinaryCursor(call.signedActivity)
        guard try cursor.u16() == 1, try cursor.u16() == 0x1001, try cursor.u8() == 12 else { throw programInvalid() }
        try cursor.tag(1); let version = try cursor.u16(); guard version == 1 || version == 2 else { throw programInvalid() }
        try cursor.tag(2); _ = try cursor.u32(); try cursor.tag(3)
        guard try cursor.u32() == ((UInt32(ProgramsClient.receiptModuleID) << 16) | UInt32(ProgramsClient.callOperation)) else {
            throw programInvalid()
        }
        try cursor.tag(4); _ = try cursor.bounded(maximum: 255, empty: true)
        try cursor.tag(5); _ = try cursor.bounded(maximum: 524_288, empty: true)
        try cursor.tag(6); _ = try cursor.u64(); try cursor.tag(7)
        let notBefore = try cursor.u64(); let notAfter = try cursor.u64(); guard notAfter >= notBefore else { throw programInvalid() }
        try cursor.tag(8); let idempotency = try cursor.bounded(maximum: 32, empty: false); guard idempotency.count == 32 else { throw programInvalid() }
        try cursor.tag(9); _ = try cursor.take(16); try cursor.tag(10)
        let payloadHash = try cursor.bounded(maximum: 32, empty: false); guard payloadHash.count == 32 else { throw programInvalid() }
        try cursor.tag(11); let payload = try cursor.bounded(maximum: 524_288, empty: true)
        try cursor.tag(12); _ = try cursor.bounded(maximum: 128, empty: true); try cursor.finish()
        let expected = try canonicalCallPayload(call)
        guard payload == expected, payloadHash == digest(Data("LXP/v1/payload-hash\0".utf8), payload) else { throw programInvalid() }
        return .init(activityID: digest(Data("LXP/v1/activity-id\0".utf8), call.signedActivity),
            idempotencyKey: idempotency, notBefore: notBefore, notAfter: notAfter)
    } catch { throw programInvalid() }
}

private func canonicalCallPayload(_ call: ProgramCall) throws -> Data {
    var value = Data("LayerX/programs/call/v1\0".utf8); value.append(call.programID)
    value.append(bigEndian(call.budget.fuel)); value.append(bigEndian128(try decimalUInt128(call.budget.feeLimit.decimal)))
    value.append(bigEndian(UInt16(call.capabilities.count)))
    for capability in call.capabilities { value.append(UInt8(capability.order)) }
    value.append(bigEndian(UInt32(call.calldata.count))); value.append(call.calldata); return value
}

private func unknownSubmission(activity: Data, key: String, retained: Data) -> ProgramSubmission {
    let value: JSONValue = .object(["state": .string("unknown"), "activity_id": .string(activity.hex),
        "idempotency_key": .string(key), "retained_signed_activity": .string(retained.hex)])
    return .init(value, state: "unknown", activityID: activity, idempotencyKey: key,
        retainedSignedActivity: retained, execution: nil)
}

private func authorized(_ value: JSONValue?) throws -> AuthorizedReceiptBatch {
    guard let object = value?.objectValue else { throw programVerification() }
    return .init(batchID: try hexData(object, "batch_id", exactBytes: 32), asset: try hexData(object, "asset", exactBytes: 32),
        previousStateRoot: try hexData(object, "previous_state_root", exactBytes: 32),
        resultingStateRoot: try hexData(object, "resulting_state_root", exactBytes: 32),
        sequencerPublicKey: try hexData(object, "sequencer_public_key", exactBytes: 32))
}

private func requireFields(_ object: [String: JSONValue], _ fields: Set<String>) throws {
    guard object.count == fields.count, Set(object.keys) == fields else { throw programDecode() }
}

private func objectValue(_ object: [String: JSONValue], _ name: String) throws -> [String: JSONValue] {
    guard let value = object[name]?.objectValue else { throw programDecode() }; return value
}

private func text(_ object: [String: JSONValue], _ name: String) throws -> String {
    guard let value = object[name]?.stringValue, !value.isEmpty else { throw programDecode() }; return value
}

private func integer32(_ object: [String: JSONValue], _ name: String) throws -> Int32 {
    guard let value = object[name]?.integerValue, let exact = Int32(exactly: value) else { throw programDecode() }; return exact
}

private func uint32Field(_ object: [String: JSONValue], _ name: String) throws -> UInt32 {
    guard let value = object[name]?.integerValue, let exact = UInt32(exactly: value) else { throw programDecode() }; return exact
}

private func decimalUInt64Field(_ object: [String: JSONValue], _ name: String) throws -> UInt64 {
    guard let parsed = decimalUInt64(object[name]) else { throw programDecode() }; return parsed
}

private func decimalInt64Field(_ object: [String: JSONValue], _ name: String) throws -> Int64 {
    guard let value = object[name]?.stringValue, value != "-0", !value.isEmpty,
          !(value.count > 1 && value.first == "0"), !value.hasPrefix("-0"), let parsed = Int64(value),
          String(parsed) == value else { throw programDecode() }; return parsed
}

private func decimalUInt128Field(_ object: [String: JSONValue], _ name: String) throws -> UInt128Value {
    guard let value = object[name]?.stringValue else { throw programDecode() }; return try decimalUInt128(value)
}

private func decimalUInt128(_ value: String) throws -> UInt128Value {
    guard !value.isEmpty, value == "0" || value.first != "0",
          value.utf8.allSatisfy({ $0 >= 48 && $0 <= 57 }) else { throw programDecode() }
    var high: UInt64 = 0; var low: UInt64 = 0
    for byte in value.utf8 {
        let lowProduct = low.multipliedFullWidth(by: 10)
        let (highProduct, highOverflow) = high.multipliedReportingOverflow(by: 10)
        let (combinedHigh, carryOverflow) = highProduct.addingReportingOverflow(lowProduct.high)
        let (nextLow, digitOverflow) = lowProduct.low.addingReportingOverflow(UInt64(byte - 48))
        let (nextHigh, digitCarryOverflow) = combinedHigh.addingReportingOverflow(digitOverflow ? 1 : 0)
        guard !highOverflow, !carryOverflow, !digitCarryOverflow else { throw programDecode() }
        high = nextHigh; low = nextLow
    }
    return .init(high: high, low: low)
}

private func requiredHex(_ object: [String: JSONValue], _ name: String, exactBytes: Int? = nil,
                         maximumBytes: Int? = nil, empty: Bool = false) throws -> String {
    guard let value = object[name]?.stringValue, value.utf8.count % 2 == 0,
          empty || !value.isEmpty,
          exactBytes == nil || value.utf8.count == exactBytes! * 2,
          maximumBytes == nil || value.utf8.count <= maximumBytes! * 2,
          value.utf8.allSatisfy({ ($0 >= 48 && $0 <= 57) || ($0 >= 97 && $0 <= 102) }) else { throw programVerification() }
    return value
}

private func hexData(_ object: [String: JSONValue], _ name: String, exactBytes: Int? = nil,
                     maximumBytes: Int? = nil, empty: Bool = false) throws -> Data {
    Data(hex: try requiredHex(object, name, exactBytes: exactBytes, maximumBytes: maximumBytes, empty: empty))
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

private func bigEndian(_ value: UInt32) -> Data {
    var encoded = value.bigEndian; return withUnsafeBytes(of: &encoded) { Data($0) }
}

private func bigEndian(_ value: UInt16) -> Data {
    var encoded = value.bigEndian; return withUnsafeBytes(of: &encoded) { Data($0) }
}

private func bigEndian128(_ value: UInt128Value) -> Data {
    var encoded = Data(); encoded.append(bigEndian(value.high)); encoded.append(bigEndian(value.low)); return encoded
}

private func digest(_ values: Data...) -> Data {
    var hasher = SHA256(); values.forEach { hasher.update(data: $0) }; return Data(hasher.finalize())
}

private func starts(_ value: Data, _ prefix: String) -> Bool { starts(value, Data(prefix.utf8)) }
private func starts(_ value: Data, _ prefix: Data) -> Bool {
    value.count >= prefix.count && value.prefix(prefix.count) == prefix
}

private struct BinaryCursor {
    private let bytes: Data
    private var offset = 0
    init(_ bytes: Data) { self.bytes = bytes }
    mutating func u8() throws -> UInt8 { try take(1)[0] }
    mutating func u16() throws -> UInt16 { UInt16(try unsigned(2)) }
    mutating func u32() throws -> UInt32 { UInt32(try unsigned(4)) }
    mutating func u64() throws -> UInt64 { try unsigned(8) }
    mutating func tag(_ expected: UInt8) throws { guard try u8() == expected else { throw programInvalid() } }
    mutating func bounded(maximum: Int, empty: Bool) throws -> Data {
        let length = try u32(); guard length <= UInt32(maximum), empty || length > 0 else { throw programInvalid() }
        return try take(Int(length))
    }
    mutating func take(_ count: Int) throws -> Data {
        guard count >= 0, offset <= bytes.count - count else { throw programInvalid() }
        let value = bytes.subdata(in: offset..<(offset + count)); offset += count; return value
    }
    mutating func finish() throws { guard offset == bytes.count else { throw programInvalid() } }
    private mutating func unsigned(_ count: Int) throws -> UInt64 {
        try take(count).reduce(0) { ($0 << 8) | UInt64($1) }
    }
}

private struct TerminalCursor {
    private let bytes: Data
    private var offset: Int
    init(_ bytes: Data, offset: Int) throws {
        guard offset >= 0, offset <= bytes.count else { throw programVerification() }
        self.bytes = bytes; self.offset = offset
    }
    mutating func u8() throws -> UInt8 { try take(1)[0] }
    mutating func u16() throws -> UInt16 { UInt16(try unsigned(2)) }
    mutating func u32() throws -> UInt32 { UInt32(try unsigned(4)) }
    mutating func u64() throws -> UInt64 { try unsigned(8) }
    mutating func i32() throws -> Int32 { Int32(bitPattern: try u32()) }
    mutating func i64() throws -> Int64 { Int64(bitPattern: try u64()) }
    mutating func u128() throws -> UInt128Value { .init(high: try u64(), low: try u64()) }
    var remaining: Int { bytes.count - offset }
    mutating func sized32() throws -> Data {
        let length = try u32(); return try take(Int(length))
    }
    mutating func sized64() throws -> Data {
        let length = try u64(); guard length <= UInt64(Int.max) else { throw programVerification() }; return try take(Int(length))
    }
    mutating func take(_ count: Int) throws -> Data {
        guard count >= 0, offset <= bytes.count - count else { throw programVerification() }
        let value = bytes.subdata(in: offset..<(offset + count)); offset += count; return value
    }
    mutating func rest() throws -> Data { try take(bytes.count - offset) }
    mutating func finish() throws { guard offset == bytes.count else { throw programVerification() } }
    private mutating func unsigned(_ count: Int) throws -> UInt64 {
        try take(count).reduce(0) { ($0 << 8) | UInt64($1) }
    }
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
private func programDecode() -> PlatformSDKError { .init(code: .decodeFailure, retry: .never) }
private func programVerification() -> PlatformSDKError { .init(code: .verificationFailure, retry: .never) }
private extension Data {
    init(hex: String) { self.init(stride(from: 0, to: hex.count, by: 2).map { index in let start = hex.index(hex.startIndex, offsetBy: index); return UInt8(hex[start..<hex.index(start, offsetBy: 2)], radix: 16)! }) }
    var hex: String { map { String(format: "%02x", $0) }.joined() }
}
