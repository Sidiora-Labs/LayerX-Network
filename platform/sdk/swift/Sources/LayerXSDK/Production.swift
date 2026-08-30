import Foundation

public enum PlatformPlane: String, Sendable { case agent, human }
public enum SDKHTTPMethod: String, Sendable { case get = "GET", post = "POST", put = "PUT", patch = "PATCH", delete = "DELETE" }
public enum RetryClass: String, Sendable { case never, safe, after, unknownOutcome = "unknown-outcome" }

public enum SDKErrorCode: String, Sendable {
    case invalidArgument = "invalid-argument"
    case idempotencyRequired = "idempotency-required"
    case transportFailure = "transport-failure"
    case deadline
    case protocolIncompatibility = "protocol-incompatibility"
    case unavailableCapability = "unavailable-capability"
    case coreRejection = "core-rejection"
    case verificationFailure = "verification-failure"
    case policyRefusal = "policy-refusal"
    case capabilityRefusal = "capability-refusal"
    case budgetRefusal = "budget-refusal"
    case rateLimit = "rate-limit"
    case idempotencyConflict = "idempotency-conflict"
    case decodeFailure = "decode-failure"
    case unknownOutcome = "unknown-outcome"
    case internalFault = "internal-fault"
}

public struct PlatformSDKError: Error, Sendable, Equatable, CustomStringConvertible {
    public let code: SDKErrorCode
    public let retry: RetryClass
    public let requestID: String?
    public let protocolResultCode: Int32?
    public let retryAfterMilliseconds: UInt64?

    public init(code: SDKErrorCode, retry: RetryClass, requestID: String? = nil, protocolResultCode: Int32? = nil, retryAfterMilliseconds: UInt64? = nil) {
        self.code = code
        self.retry = retry
        self.requestID = requestID
        self.protocolResultCode = protocolResultCode
        self.retryAfterMilliseconds = retryAfterMilliseconds
    }

    public var description: String { "LayerX SDK error: \(code.rawValue)" }
}

public struct IdempotencyKey: RawRepresentable, Hashable, Sendable {
    public let rawValue: String

    public init?(rawValue: String) {
        guard !rawValue.isEmpty, rawValue.utf8.count <= 255, !rawValue.contains("\0") else { return nil }
        self.rawValue = rawValue
    }

    public init(_ value: String) throws {
        guard let parsed = Self(rawValue: value) else {
            throw PlatformSDKError(code: .invalidArgument, retry: .never)
        }
        self = parsed
    }
}

public struct ProtocolAmount: Hashable, Sendable, Codable, CustomStringConvertible {
    private static let maximum = "340282366920938463463374607431768211455"
    public let decimal: String

    public init(_ value: String) throws {
        let asciiDigits = !value.isEmpty && value.utf8.allSatisfy { $0 >= 48 && $0 <= 57 }
        guard asciiDigits, (value == "0" || value.first != "0") else {
            throw PlatformSDKError(code: .invalidArgument, retry: .never)
        }
        guard value.count < Self.maximum.count || (value.count == Self.maximum.count && value <= Self.maximum) else {
            throw PlatformSDKError(code: .invalidArgument, retry: .never)
        }
        decimal = value
    }

    public var description: String { decimal }
    public init(from decoder: Decoder) throws { try self.init(decoder.singleValueContainer().decode(String.self)) }
    public func encode(to encoder: Encoder) throws { var container = encoder.singleValueContainer(); try container.encode(decimal) }
}

public final class SecretBytes: @unchecked Sendable, CustomStringConvertible {
    private let lock = NSLock()
    private var storage: Data
    private var destroyed = false

    public init(_ bytes: Data) throws {
        guard !bytes.isEmpty else { throw PlatformSDKError(code: .invalidArgument, retry: .never) }
        storage = Data(bytes)
    }

    deinit { destroy() }

    public func withBytes<T>(_ consume: (Data) throws -> T) throws -> T {
        lock.lock()
        defer { lock.unlock() }
        guard !destroyed else { throw PlatformSDKError(code: .invalidArgument, retry: .never) }
        return try consume(storage)
    }

    public func destroy() {
        lock.lock()
        defer { lock.unlock() }
        guard !destroyed else { return }
        storage.resetBytes(in: 0..<storage.count)
        storage.removeAll(keepingCapacity: false)
        destroyed = true
    }

    public var description: String { "[REDACTED]" }
}

public struct OperationDescriptor: Sendable, Equatable {
    public let plane: PlatformPlane
    public let name: String
    public let method: SDKHTTPMethod
    public let path: String
    public let requestType: String
    public let responseType: String
    public let requiresIdempotency: Bool
    public let bodyless: Bool

    public init(plane: PlatformPlane, name: String, method: SDKHTTPMethod, path: String, requestType: String, responseType: String, requiresIdempotency: Bool, bodyless: Bool) {
        self.plane = plane; self.name = name; self.method = method; self.path = path
        self.requestType = requestType; self.responseType = responseType
        self.requiresIdempotency = requiresIdempotency; self.bodyless = bodyless
    }
}

public struct TransportCall: Sendable {
    public let operation: PlatformOperation
    public let request: JSONValue
    public let pathParameters: [String: String]
    public let idempotencyKey: IdempotencyKey?
}

public struct ProgramTransportCall: Sendable {
    public let operation: String
    public let request: JSONValue
    public let pathParameters: [String: String]
    public let idempotencyKey: IdempotencyKey?

    public init(operation: String, request: JSONValue, pathParameters: [String: String] = [:],
                idempotencyKey: IdempotencyKey? = nil) {
        self.operation = operation; self.request = request
        self.pathParameters = pathParameters; self.idempotencyKey = idempotencyKey
    }
}

public protocol PlatformTransport: Sendable {
    func send(_ call: TransportCall) async throws -> JSONValue
    func sendProgram(_ call: ProgramTransportCall) async throws -> JSONValue
}

public extension PlatformTransport {
    func sendProgram(_ call: ProgramTransportCall) async throws -> JSONValue {
        throw PlatformSDKError(code: .unavailableCapability, retry: .never)
    }
}

public struct SDKTelemetryEvent: Sendable {
    public enum Outcome: Sendable { case completed, refused }
    public let plane: PlatformPlane
    public let operation: String
    public let outcome: Outcome
    public let code: SDKErrorCode?
}

public typealias SDKTelemetry = @Sendable (SDKTelemetryEvent) -> Void

public struct SDKMetadata: Sendable, Equatable {
    public let name: String
    public let version: String
    public let agentOperations: Int
    public let humanOperations: Int
    public init(name: String, version: String, agentOperations: Int, humanOperations: Int) {
        self.name = name; self.version = version; self.agentOperations = agentOperations; self.humanOperations = humanOperations
    }
}

public final class PlatformClient: @unchecked Sendable {
    private let transport: PlatformTransport
    private let telemetry: SDKTelemetry?

    public init(transport: PlatformTransport, telemetry: SDKTelemetry? = nil) {
        self.transport = transport; self.telemetry = telemetry
    }

    public func read(_ operation: PlatformOperation, request: JSONValue = .emptyObject, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        guard !operation.descriptor.requiresIdempotency else {
            throw PlatformSDKError(code: .idempotencyRequired, retry: .never)
        }
        return try await execute(operation, request: request, idempotencyKey: nil, pathParameters: pathParameters)
    }

    public func mutate(_ operation: PlatformOperation, request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        guard operation.descriptor.requiresIdempotency else {
            throw PlatformSDKError(code: .invalidArgument, retry: .never)
        }
        return try await execute(operation, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }

    func program(_ operation: String, request: JSONValue, idempotencyKey: IdempotencyKey? = nil,
                 pathParameters: [String: String] = [:]) async throws -> JSONValue {
        do {
            let response = try await transport.sendProgram(.init(operation: operation, request: request,
                pathParameters: pathParameters, idempotencyKey: idempotencyKey))
            telemetry?(.init(plane: .agent, operation: operation, outcome: .completed, code: nil))
            return response
        } catch let error as PlatformSDKError {
            telemetry?(.init(plane: .agent, operation: operation, outcome: .refused, code: error.code))
            throw error
        } catch {
            let code: SDKErrorCode = operation == "program.call" ? .unknownOutcome : .transportFailure
            let retry: RetryClass = operation == "program.call" ? .unknownOutcome : .safe
            telemetry?(.init(plane: .agent, operation: operation, outcome: .refused, code: code))
            throw PlatformSDKError(code: code, retry: retry)
        }
    }

    private func execute(_ operation: PlatformOperation, request: JSONValue, idempotencyKey: IdempotencyKey?, pathParameters: [String: String]) async throws -> JSONValue {
        do {
            let response = try await transport.send(.init(operation: operation, request: request, pathParameters: pathParameters, idempotencyKey: idempotencyKey))
            telemetry?(.init(plane: operation.descriptor.plane, operation: operation.descriptor.name, outcome: .completed, code: nil))
            return response
        } catch let error as PlatformSDKError {
            telemetry?(.init(plane: operation.descriptor.plane, operation: operation.descriptor.name, outcome: .refused, code: error.code))
            throw error
        } catch {
            let code: SDKErrorCode = idempotencyKey == nil ? .transportFailure : .unknownOutcome
            let retry: RetryClass = idempotencyKey == nil ? .safe : .unknownOutcome
            telemetry?(.init(plane: operation.descriptor.plane, operation: operation.descriptor.name, outcome: .refused, code: code))
            throw PlatformSDKError(code: code, retry: retry)
        }
    }
}
