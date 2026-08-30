import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif

private let maximumHTTPResponseBytes = 8 * 1024 * 1024
private let maximumProgramsRequestBytes = 8 * 1024 * 1024
private let maximumProgramBytes = 1_048_576

public final class AccessToken: @unchecked Sendable, CustomStringConvertible {
    private let secret: SecretBytes

    public init(_ bytes: Data) throws {
        guard String(data: bytes, encoding: .utf8) != nil else {
            throw PlatformSDKError(code: .invalidArgument, retry: .never)
        }
        secret = try SecretBytes(bytes)
    }

    fileprivate func authorize(_ request: inout URLRequest) throws {
        try secret.withBytes { bytes in
            guard let value = String(data: bytes, encoding: .utf8), !value.contains("\r"), !value.contains("\n") else {
                throw PlatformSDKError(code: .invalidArgument, retry: .never)
            }
            request.setValue("Bearer \(value)", forHTTPHeaderField: "Authorization")
        }
    }

    public func destroy() { secret.destroy() }
    public var description: String { "[REDACTED]" }
}

public final class LayerXKeyCredential: @unchecked Sendable, CustomStringConvertible {
    private let keyID: String
    private let secret: SecretBytes

    public init(keyID: String, secret: Data) throws {
        let validID = !keyID.isEmpty && keyID.utf8.count <= 64 && keyID.utf8.allSatisfy {
            ($0 >= 48 && $0 <= 57) || ($0 >= 65 && $0 <= 90) || ($0 >= 97 && $0 <= 122) || $0 == 45 || $0 == 95
        }
        guard validID else { throw PlatformSDKError(code: .invalidArgument, retry: .never) }
        self.keyID = keyID
        self.secret = try SecretBytes(secret)
    }

    fileprivate func authorize(_ request: inout URLRequest) throws {
        try secret.withBytes { bytes in
            guard let value = String(data: bytes, encoding: .ascii), value.hasPrefix("lxp_live_"), value.utf8.count == 73,
                  value.dropFirst(9).utf8.allSatisfy({ ($0 >= 48 && $0 <= 57) || ($0 >= 97 && $0 <= 102) }) else {
                throw PlatformSDKError(code: .invalidArgument, retry: .never)
            }
            request.setValue("LayerX-Key \(keyID):\(value)", forHTTPHeaderField: "Authorization")
        }
    }

    public func destroy() { secret.destroy() }
    public var description: String { "[REDACTED]" }
}

public final class AgentHTTPTransport: PlatformTransport, @unchecked Sendable {
    private static let operations: Set<String> = [
        "program.discover", "program.interface", "program.simulate",
        "program.call", "program.receipt", "program.activity",
    ]
    private let baseURL: URL
    private let session: URLSession
    private let credential: LayerXKeyCredential?
    private struct ProgramRoute {
        let method: String
        let path: String
        let pathParameters: Set<String>
        let idempotent: Bool
    }
    private static let programRoutes: [String: ProgramRoute] = [
        "program.discover": .init(method: "GET", path: "/v1/programs/registry/{program_id}",
            pathParameters: ["program_id"], idempotent: false),
        "program.interface": .init(method: "GET", path: "/v1/programs/registry/{program_id}/interface",
            pathParameters: ["program_id"], idempotent: false),
        "program.simulate": .init(method: "POST", path: "/v1/programs/simulate",
            pathParameters: [], idempotent: false),
        "program.call": .init(method: "POST", path: "/v1/programs/call",
            pathParameters: [], idempotent: true),
        "program.receipt": .init(method: "GET", path: "/v1/programs/receipts/by-idempotency/{idempotency_key}",
            pathParameters: ["idempotency_key"], idempotent: false),
        "program.activity": .init(method: "GET", path: "/v1/programs/activities/{activity_id}",
            pathParameters: ["activity_id"], idempotent: false),
    ]

    public init(baseURL: URL, session: URLSession = .shared, credential: LayerXKeyCredential? = nil) throws {
        guard baseURL.user == nil, baseURL.password == nil, baseURL.host != nil,
              baseURL.query == nil, baseURL.fragment == nil,
              baseURL.scheme == "https" || (baseURL.scheme == "http" && Self.isLoopback(baseURL.host)) else {
            throw PlatformSDKError(code: .invalidArgument, retry: .never)
        }
        self.baseURL = baseURL
        self.session = session
        self.credential = credential
    }

    public func send(_ call: TransportCall) async throws -> JSONValue {
        let descriptor = call.operation.descriptor
        guard descriptor.plane == .agent, Self.operations.contains(descriptor.name) else {
            throw PlatformSDKError(code: .unavailableCapability, retry: .never)
        }
        return try await sendProgram(.init(operation: descriptor.name, request: call.request,
            pathParameters: call.pathParameters, idempotencyKey: call.idempotencyKey))
    }

    public func sendProgram(_ call: ProgramTransportCall) async throws -> JSONValue {
        guard let route = Self.programRoutes[call.operation], route.pathParameters == Set(call.pathParameters.keys) else {
            throw PlatformSDKError(code: .invalidArgument, retry: .never)
        }
        if route.idempotent {
            guard let key = call.idempotencyKey, Self.hex32(key.rawValue) else {
                throw PlatformSDKError(code: .idempotencyRequired, retry: .never)
            }
        } else if call.idempotencyKey != nil {
            throw PlatformSDKError(code: .invalidArgument, retry: .never)
        }
        try Self.validateProgramRequest(call)
        let encoded = try JSONEncoder().encode(call.request)
        guard !encoded.isEmpty, encoded.count <= maximumProgramsRequestBytes else {
            throw PlatformSDKError(code: .invalidArgument, retry: .never)
        }
        var path = route.path
        for name in route.pathParameters {
            guard let value = call.pathParameters[name], Self.hex32(value),
                  call.request.objectValue?[name]?.stringValue == value else {
                throw PlatformSDKError(code: .invalidArgument, retry: .never)
            }
            path = path.replacingOccurrences(of: "{\(name)}", with: Self.percentEncodePathSegment(value))
        }
        guard let target = Self.rootEndpoint(baseURL, path: path) else {
            throw PlatformSDKError(code: .invalidArgument, retry: .never)
        }
        var request = URLRequest(url: target)
        request.httpMethod = route.method
        request.httpBody = encoded
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("layerx-swift/0.1.0", forHTTPHeaderField: "User-Agent")
        if let key = call.idempotencyKey { request.setValue(key.rawValue, forHTTPHeaderField: "Idempotency-Key") }
        guard let credential else {
            throw PlatformSDKError(code: .capabilityRefusal, retry: .never)
        }
        try credential.authorize(&request)
        let data: Data
        let response: URLResponse
        do {
            (data, response) = try await session.data(for: request, delegate: NoRedirectDelegate.shared)
        } catch {
            if call.operation == "program.call" { throw Self.unknownOutcome() }
            throw PlatformSDKError(code: .transportFailure, retry: .safe)
        }
        do {
            guard data.count <= maximumHTTPResponseBytes, let http = response as? HTTPURLResponse,
                  Self.jsonContentType(http) else { throw Self.decode() }
            return try Self.decodeProgramEnvelope(call.operation, status: http.statusCode, data: data)
        } catch let error as PlatformSDKError {
            if call.operation == "program.call",
               (error.code == .decodeFailure || error.code == .verificationFailure) {
                throw Self.unknownOutcome()
            }
            throw error
        }
    }

    private static func decodeProgramEnvelope(_ operation: String, status: Int, data: Data) throws -> JSONValue {
        let document: JSONValue
        do { document = try JSONDecoder().decode(JSONValue.self, from: data) }
        catch { throw decode() }
        guard let envelope = document.objectValue else { throw decode() }
        if envelope["class"] != nil {
            guard !(200..<300).contains(status), exact(envelope,
                ["class", "protocol_result_code", "retriability", "request_id", "reason"]) else { throw decode() }
            throw try serviceError(envelope)
        }
        guard (200..<300).contains(status), exact(envelope, ["request_id", "value", "verification_status"]),
              let requestID = envelope["request_id"]?.stringValue, validRequestID(requestID),
              let value = envelope["value"], value != .null,
              validVerification(operation, value: value, status: envelope["verification_status"]) else {
            throw decode(envelope["request_id"]?.stringValue)
        }
        return value
    }

    private static func validateProgramRequest(_ call: ProgramTransportCall) throws {
        guard let object = call.request.objectValue else { throw invalid() }
        switch call.operation {
        case "program.discover", "program.interface":
            guard exact(object, ["program_id", "requested_verification_level"]),
                  canonicalProgram(object["program_id"]),
                  object["requested_verification_level"]?.stringValue == "sequencer-signed" else { throw invalid() }
        case "program.receipt":
            guard exact(object, ["idempotency_key", "expected_activity_id", "requested_verification_level"]),
                  canonicalHex(object["idempotency_key"], bytes: 32, empty: false),
                  canonicalHex(object["expected_activity_id"], bytes: 32, empty: false),
                  object["requested_verification_level"]?.stringValue == "sequencer-signed" else { throw invalid() }
        case "program.activity":
            guard exact(object, ["activity_id", "requested_verification_level"]),
                  canonicalHex(object["activity_id"], bytes: 32, empty: false),
                  object["requested_verification_level"]?.stringValue == "sequencer-signed" else { throw invalid() }
        case "program.simulate", "program.call":
            try validateProgramCall(object)
        default: throw invalid()
        }
    }

    private static func validateProgramCall(_ object: [String: JSONValue]) throws {
        guard exact(object, ["program_id", "calldata", "budget", "capabilities", "signed_activity"]),
              canonicalProgram(object["program_id"]),
              boundedHex(object["calldata"], maximum: maximumProgramBytes, empty: true),
              boundedHex(object["signed_activity"], maximum: maximumProgramBytes, empty: false),
              let budget = object["budget"]?.objectValue,
              exact(budget, ["fuel", "fee_limit"]),
              canonicalUInt64(budget["fuel"], positive: true), canonicalUInt128(budget["fee_limit"]),
              case let .array(capabilities)? = object["capabilities"], capabilities.count <= 5 else { throw invalid() }
        let order = ["storage_read", "storage_write", "transfer", "emit_event", "compose"]
        var previous = -1
        for capability in capabilities {
            guard let name = capability.stringValue, let current = order.firstIndex(of: name), current > previous else {
                throw invalid()
            }
            previous = current
        }
    }

    static func validVerification(_ operation: String, value: JSONValue, status: JSONValue?) -> Bool {
        guard let object = status?.objectValue else { return false }
        if operation == "program.discover" || operation == "program.interface" {
            return exact(object, ["state", "level", "reason"])
                && object["state"]?.stringValue == "Unverified"
                && object["level"]?.stringValue == "SequencerSigned"
                && object["reason"]?.stringValue == "server_side_receipt_verification_only"
        }
        let pending = ["program.call", "program.receipt", "program.activity"].contains(operation)
            && value.objectValue?["state"]?.stringValue == "unknown"
        if pending {
            return exact(object, ["state", "level", "reason"])
                && object["state"]?.stringValue == "Unverified"
                && object["level"]?.stringValue == "SequencerSigned"
                && object["reason"]?.stringValue == "receipt_pending"
        }
        return ["program.simulate", "program.call", "program.receipt", "program.activity"].contains(operation)
            && exact(object, ["state", "level"])
            && object["state"]?.stringValue == "Achieved"
            && object["level"]?.stringValue == "SequencerSigned"
    }

    private static func serviceError(_ object: [String: JSONValue]) throws -> PlatformSDKError {
        guard let requestID = object["request_id"]?.stringValue, validRequestID(requestID),
              let errorClass = object["class"]?.stringValue,
              let retriability = object["retriability"]?.stringValue,
              let reason = object["reason"]?.stringValue, !reason.isEmpty, reason.utf8.count <= 128,
              reason.utf8.allSatisfy({ ($0 >= 48 && $0 <= 57) || ($0 >= 97 && $0 <= 122) || $0 == 95 || $0 == 46 }) else {
            throw decode()
        }
        let resultCode: Int32?
        switch object["protocol_result_code"] {
        case .null?: resultCode = nil
        case let .integer(value)?:
            guard let exact = Int32(exactly: value) else { throw decode(requestID) }
            resultCode = exact
        default: throw decode(requestID)
        }
        return try errorClass.sdkError(.init(requestID: requestID, value: nil,
            verificationStatus: nil, errorClass: errorClass, protocolResultCode: resultCode,
            retriability: retriability, reason: reason))
    }

    private static func exact(_ object: [String: JSONValue], _ fields: Set<String>) -> Bool {
        object.count == fields.count && Set(object.keys) == fields
    }

    private static func canonicalProgram(_ value: JSONValue?) -> Bool {
        canonicalHex(value, bytes: 32, empty: false) && value?.stringValue != String(repeating: "0", count: 64)
    }

    private static func canonicalHex(_ value: JSONValue?, bytes: Int, empty: Bool) -> Bool {
        guard let text = value?.stringValue else { return false }
        return empty && text.isEmpty || text.utf8.count == bytes * 2 && canonicalHexText(text)
    }

    private static func boundedHex(_ value: JSONValue?, maximum: Int, empty: Bool) -> Bool {
        guard let text = value?.stringValue, text.utf8.count % 2 == 0,
              text.utf8.count <= maximum * 2, empty || !text.isEmpty else { return false }
        return canonicalHexText(text)
    }

    private static func canonicalHexText(_ value: String) -> Bool {
        value.utf8.allSatisfy { ($0 >= 48 && $0 <= 57) || ($0 >= 97 && $0 <= 102) }
    }

    private static func canonicalUInt64(_ value: JSONValue?, positive: Bool) -> Bool {
        guard let text = value?.stringValue, canonicalDecimal(text), let parsed = UInt64(text) else { return false }
        return !positive || parsed > 0
    }

    private static func canonicalUInt128(_ value: JSONValue?) -> Bool {
        let maximum = "340282366920938463463374607431768211455"
        guard let text = value?.stringValue, canonicalDecimal(text) else { return false }
        return text.count < maximum.count || text.count == maximum.count && text <= maximum
    }

    private static func canonicalDecimal(_ value: String) -> Bool {
        !value.isEmpty && (value == "0" || value.first != "0")
            && value.utf8.allSatisfy { $0 >= 48 && $0 <= 57 }
    }

    private static func validRequestID(_ value: String) -> Bool {
        !value.isEmpty && value.utf8.count <= 128 && value.utf8.allSatisfy { $0 >= 0x21 && $0 <= 0x7e }
    }

    private static func jsonContentType(_ response: HTTPURLResponse) -> Bool {
        response.value(forHTTPHeaderField: "Content-Type")?.split(separator: ";", maxSplits: 1).first?
            .trimmingCharacters(in: .whitespaces).lowercased() == "application/json"
    }

    private static func resolvePath(_ template: String, parameters: [String: String]) throws -> String {
        var path = template
        for (name, value) in parameters {
            let token = "{\(name)}"
            guard !name.isEmpty, !value.isEmpty, path.contains(token) else {
                throw PlatformSDKError(code: .invalidArgument, retry: .never)
            }
            path = path.replacingOccurrences(of: token, with: percentEncodePathSegment(value))
        }
        guard path.first == "/", !path.contains("{"), !path.contains("}") else {
            throw PlatformSDKError(code: .invalidArgument, retry: .never)
        }
        return path
    }

    private static func rootEndpoint(_ base: URL, path: String) -> URL? {
        guard var components = URLComponents(url: base, resolvingAgainstBaseURL: false) else { return nil }
        components.path = path
        components.query = nil; components.fragment = nil
        return components.url
    }

    private static func percentEncodePathSegment(_ value: String) -> String {
        let allowed = CharacterSet(charactersIn: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~")
        return value.addingPercentEncoding(withAllowedCharacters: allowed) ?? ""
    }

    private static func hex32(_ value: String) -> Bool {
        value.utf8.count == 64 && value.utf8.allSatisfy { ($0 >= 48 && $0 <= 57) || ($0 >= 97 && $0 <= 102) }
    }

    private static func invalid() -> PlatformSDKError { .init(code: .invalidArgument, retry: .never) }
    private static func decode(_ requestID: String? = nil) -> PlatformSDKError {
        .init(code: .decodeFailure, retry: .never, requestID: requestID)
    }
    private static func unknownOutcome() -> PlatformSDKError { .init(code: .unknownOutcome, retry: .unknownOutcome) }

    private static func isLoopback(_ host: String?) -> Bool {
        guard let host = host?.lowercased() else { return false }
        if host == "localhost" || host == "::1" || host == "[::1]" { return true }
        let octets = host.split(separator: ".")
        return octets.count == 4 && octets.first == "127" && octets.allSatisfy { UInt8($0) != nil }
    }
}

private final class NoRedirectDelegate: NSObject, URLSessionTaskDelegate, @unchecked Sendable {
    static let shared = NoRedirectDelegate()
    func urlSession(_ session: URLSession, task: URLSessionTask,
                    willPerformHTTPRedirection response: HTTPURLResponse,
                    newRequest request: URLRequest,
                    completionHandler: @escaping (URLRequest?) -> Void) {
        completionHandler(nil)
    }
}

public final class HumanHTTPTransport: PlatformTransport, @unchecked Sendable {
    private let baseURL: URL
    private let session: URLSession
    private let accessToken: AccessToken?

    public init(baseURL: URL, session: URLSession = .shared, accessToken: AccessToken? = nil) throws {
        guard baseURL.user == nil, baseURL.password == nil, baseURL.host != nil,
              baseURL.scheme == "https" || (baseURL.scheme == "http" && Self.isLoopback(baseURL.host)) else {
            throw PlatformSDKError(code: .invalidArgument, retry: .never)
        }
        self.baseURL = baseURL
        self.session = session
        self.accessToken = accessToken
    }

    public func send(_ call: TransportCall) async throws -> JSONValue {
        let descriptor = call.operation.descriptor
        guard descriptor.plane == .human else {
            throw PlatformSDKError(code: .unavailableCapability, retry: .never)
        }
        let path = try Self.resolvePath(descriptor.path, parameters: call.pathParameters)
        guard let target = URL(string: path, relativeTo: baseURL)?.absoluteURL,
              target.scheme == baseURL.scheme, target.host == baseURL.host, target.port == baseURL.port else {
            throw PlatformSDKError(code: .invalidArgument, retry: .never)
        }
        var request = URLRequest(url: target)
        request.httpMethod = descriptor.method.rawValue
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        request.setValue("layerx-swift/0.1.0", forHTTPHeaderField: "User-Agent")
        if !descriptor.bodyless {
            request.httpBody = try JSONEncoder().encode(call.request)
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        }
        if let key = call.idempotencyKey {
            request.setValue(key.rawValue, forHTTPHeaderField: "Idempotency-Key")
        }
        try accessToken?.authorize(&request)

        let (data, response) = try await perform(request)
        guard data.count <= maximumHTTPResponseBytes, let http = response as? HTTPURLResponse else {
            throw PlatformSDKError(code: .decodeFailure, retry: .never)
        }
        let envelope: HumanEnvelope
        do {
            envelope = try JSONDecoder().decode(HumanEnvelope.self, from: data)
        } catch {
            throw PlatformSDKError(code: .decodeFailure, retry: .never)
        }
        guard !envelope.trace.isEmpty, envelope.trace.utf8.count <= 512,
              !envelope.trace.contains("\0"), !envelope.trace.contains("\r"), !envelope.trace.contains("\n") else {
            throw PlatformSDKError(code: .decodeFailure, retry: .never)
        }
        if envelope.ok {
            guard (200..<300).contains(http.statusCode), envelope.error == nil, let result = envelope.result else {
                throw PlatformSDKError(code: .decodeFailure, retry: .never)
            }
            return result
        }
        guard !(200..<300).contains(http.statusCode), envelope.result == nil, let error = envelope.error else {
            throw PlatformSDKError(code: .decodeFailure, retry: .never)
        }
        throw error.sdkError(trace: envelope.trace)
    }

    private func perform(_ request: URLRequest) async throws -> (Data, URLResponse) {
        let box = URLSessionTaskBox()
        return try await withTaskCancellationHandler(operation: {
            try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<(Data, URLResponse), Error>) in
                let task = session.dataTask(with: request) { data, response, error in
                    if let error { continuation.resume(throwing: error); return }
                    guard let data, let response else {
                        continuation.resume(throwing: PlatformSDKError(code: .transportFailure, retry: .safe))
                        return
                    }
                    continuation.resume(returning: (data, response))
                }
                box.install(task)
                task.resume()
            }
        }, onCancel: {
            box.cancel()
        })
    }

    private static func resolvePath(_ template: String, parameters: [String: String]) throws -> String {
        var path = template
        for (name, value) in parameters {
            guard !name.isEmpty, !value.isEmpty, !name.contains("{") && !name.contains("}") else {
                throw PlatformSDKError(code: .invalidArgument, retry: .never)
            }
            path = path.replacingOccurrences(of: "{\(name)}", with: percentEncodePathSegment(value))
        }
        guard !path.contains("{") && !path.contains("}") && path.first == "/" else {
            throw PlatformSDKError(code: .invalidArgument, retry: .never)
        }
        return path
    }

    private static func percentEncodePathSegment(_ value: String) -> String {
        let allowed = CharacterSet(charactersIn: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~")
        return value.addingPercentEncoding(withAllowedCharacters: allowed) ?? ""
    }

    private static func isLoopback(_ host: String?) -> Bool {
        guard let host = host?.lowercased() else { return false }
        return host == "localhost" || host == "127.0.0.1" || host == "::1" || host == "[::1]"
    }
}

private final class URLSessionTaskBox: @unchecked Sendable {
    private let lock = NSLock()
    private var task: URLSessionTask?
    private var cancelled = false

    func install(_ task: URLSessionTask) {
        lock.lock()
        self.task = task
        let shouldCancel = cancelled
        lock.unlock()
        if shouldCancel { task.cancel() }
    }

    func cancel() {
        lock.lock()
        cancelled = true
        let task = task
        lock.unlock()
        task?.cancel()
    }
}

private struct HumanEnvelope: Decodable {
    let ok: Bool
    let result: JSONValue?
    let error: HumanAPIError?
    let trace: String
}

private struct AgentEnvelope: Decodable {
    let requestID: String
    let value: JSONValue?
    let verificationStatus: JSONValue?
    let errorClass: String?
    let protocolResultCode: Int32?
    let retriability: String?
    let reason: String?

    enum CodingKeys: String, CodingKey {
        case value, retriability, reason
        case requestID = "request_id"
        case verificationStatus = "verification_status"
        case errorClass = "class"
        case protocolResultCode = "protocol_result_code"
    }
}

private extension String {
    func sdkError(_ envelope: AgentEnvelope) throws -> PlatformSDKError {
        guard !envelope.requestID.isEmpty, let retriability = envelope.retriability,
              let reason = envelope.reason, !reason.isEmpty,
              reason.utf8.allSatisfy({ ($0 >= 48 && $0 <= 57) || ($0 >= 97 && $0 <= 122) || $0 == 95 || $0 == 46 }) else {
            throw PlatformSDKError(code: .decodeFailure, retry: .never)
        }
        let code: SDKErrorCode
        switch self {
        case "TransportFailure": code = .transportFailure
        case "Deadline": code = .deadline
        case "ProtocolIncompatibility": code = .protocolIncompatibility
        case "UnavailableCapability": code = .unavailableCapability
        case "CoreRejection": code = .coreRejection
        case "VerificationFailure": code = .verificationFailure
        case "PolicyRefusal": code = .policyRefusal
        case "CapabilityRefusal": code = .capabilityRefusal
        case "BudgetRefusal": code = .budgetRefusal
        case "RateLimit": code = .rateLimit
        case "IdempotencyConflict": code = .idempotencyConflict
        case "InternalFault": code = .internalFault
        default: throw PlatformSDKError(code: .decodeFailure, retry: .never, requestID: envelope.requestID)
        }
        let retry: RetryClass
        switch retriability {
        case "Terminal": retry = .never
        case "Retriable": retry = .safe
        default: throw PlatformSDKError(code: .decodeFailure, retry: .never, requestID: envelope.requestID)
        }
        return PlatformSDKError(code: code, retry: retry, requestID: envelope.requestID, protocolResultCode: envelope.protocolResultCode)
    }
}

private struct HumanAPIError: Decodable {
    let code: String
    let retry: String
    let retryAfterMilliseconds: UInt64?

    enum CodingKeys: String, CodingKey {
        case code, retry
        case retryAfterMilliseconds = "retry_after_ms"
    }

    func sdkError(trace: String) -> PlatformSDKError {
        let mapped: SDKErrorCode
        switch code {
        case "rate-limited": mapped = .rateLimit
        case "unavailable", "upstream-degraded": mapped = .transportFailure
        case "refused-by-policy": mapped = .policyRefusal
        case "refused-by-budget", "refused-by-limit": mapped = .budgetRefusal
        case "refused-by-capability", "forbidden", "unauthenticated", "session-expired", "step-up-required": mapped = .capabilityRefusal
        case "conflict": mapped = .idempotencyConflict
        case "refused-by-protocol": mapped = .coreRejection
        default: mapped = .coreRejection
        }
        let retryClass: RetryClass
        switch retry {
        case "retriable": retryClass = .safe
        case "retriable-after":
            guard retryAfterMilliseconds != nil else { return PlatformSDKError(code: .decodeFailure, retry: .never) }
            retryClass = .after
        case "structural", "final": retryClass = .never
        default: return PlatformSDKError(code: .decodeFailure, retry: .never)
        }
        return PlatformSDKError(code: mapped, retry: retryClass, requestID: trace, retryAfterMilliseconds: retryAfterMilliseconds)
    }
}
