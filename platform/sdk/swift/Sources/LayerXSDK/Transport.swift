import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif

private let maximumHTTPResponseBytes = 8 * 1024 * 1024

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
        if descriptor.name == "program.call" {
            guard let key = call.idempotencyKey, Self.hex32(key.rawValue) else {
                throw PlatformSDKError(code: .invalidArgument, retry: .never)
            }
        } else if call.idempotencyKey != nil {
            throw PlatformSDKError(code: .invalidArgument, retry: .never)
        }
        let path = try Self.resolvePath(descriptor.path, parameters: call.pathParameters)
        guard let target = Self.endpoint(baseURL, path: path) else {
            throw PlatformSDKError(code: .invalidArgument, retry: .never)
        }
        var request = URLRequest(url: target)
        request.httpMethod = descriptor.method.rawValue
        request.httpBody = try JSONEncoder().encode(call.request)
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("layerx-swift/0.1.0", forHTTPHeaderField: "User-Agent")
        if let key = call.idempotencyKey { request.setValue(key.rawValue, forHTTPHeaderField: "Idempotency-Key") }
        try credential?.authorize(&request)
        let (data, response) = try await session.data(for: request)
        guard data.count <= maximumHTTPResponseBytes, let http = response as? HTTPURLResponse else {
            throw PlatformSDKError(code: .decodeFailure, retry: .never)
        }
        let envelope: AgentEnvelope
        do { envelope = try JSONDecoder().decode(AgentEnvelope.self, from: data) }
        catch { throw PlatformSDKError(code: .decodeFailure, retry: .never) }
        if let error = envelope.errorClass {
            guard !(200..<300).contains(http.statusCode), envelope.value == nil else {
                throw PlatformSDKError(code: .decodeFailure, retry: .never)
            }
            throw try error.sdkError(envelope)
        }
        guard (200..<300).contains(http.statusCode), !envelope.requestID.isEmpty,
              let value = envelope.value, Self.sequencerSigned(envelope.verificationStatus) else {
            if !envelope.requestID.isEmpty, envelope.value != nil {
                throw PlatformSDKError(code: .verificationFailure, retry: .never, requestID: envelope.requestID)
            }
            throw PlatformSDKError(code: .decodeFailure, retry: .never, requestID: envelope.requestID.isEmpty ? nil : envelope.requestID)
        }
        return value
    }

    private static func sequencerSigned(_ value: JSONValue?) -> Bool {
        guard let status = value?.objectValue else { return false }
        return status["state"]?.stringValue == "Achieved" && status["level"]?.stringValue == "SequencerSigned"
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

    private static func endpoint(_ base: URL, path: String) -> URL? {
        guard var components = URLComponents(url: base, resolvingAgainstBaseURL: false) else { return nil }
        let prefix = components.path.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        let suffix = path.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        components.path = "/" + [prefix, suffix].filter { !$0.isEmpty }.joined(separator: "/")
        return components.url
    }

    private static func percentEncodePathSegment(_ value: String) -> String {
        let allowed = CharacterSet(charactersIn: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~")
        return value.addingPercentEncoding(withAllowedCharacters: allowed) ?? ""
    }

    private static func hex32(_ value: String) -> Bool {
        value.utf8.count == 64 && value.utf8.allSatisfy { ($0 >= 48 && $0 <= 57) || ($0 >= 97 && $0 <= 102) }
    }

    private static func isLoopback(_ host: String?) -> Bool {
        guard let host = host?.lowercased() else { return false }
        if host == "localhost" || host == "::1" || host == "[::1]" { return true }
        let octets = host.split(separator: ".")
        return octets.count == 4 && octets.first == "127" && octets.allSatisfy { UInt8($0) != nil }
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
