import Foundation
import Crypto
#if canImport(Darwin)
import Darwin
#elseif canImport(Glibc)
import Glibc
#endif

public struct MirrorCandidate: Sendable {
    public let source: Int
    public let commitment: Data
    public init(source: Int, commitment: Data) {
        self.source = source
        self.commitment = commitment
    }
}

public struct MirrorPolicy: Sendable {
    public let kind: MirrorPolicyKind
    public let candidates: [MirrorCandidate]
    public let minimum: Int
    public init(kind: MirrorPolicyKind, candidates: [MirrorCandidate], minimum: Int = 1) {
        self.kind = kind
        self.candidates = candidates
        self.minimum = minimum
    }
}

public struct MirrorVerification: Sendable {
    public let level: String
    public let batchNumber: UInt64
    public let headerDigest: Data
    public let evidenceDigest: Data
    public let sourceID: String
    public let target: String
    public let canonicalPosition: String
    public let provenance: String
    public let latestBatch: UInt64?
    public let batchLag: String
    public let failoverCount: Int
    public let agreeingSources: Int
    public let checkpointLevel: String
}

public protocol MirrorSourceVerifying: Sendable {
    func receipt(batchNumber: UInt64, policy: MirrorPolicy,
                 canonicalReceipt: Data) throws -> MirrorVerification
    func state(batchNumber: UInt64, policy: MirrorPolicy, canonicalState: Data,
               canonicalProof: Data) throws -> MirrorVerification
}

#if os(macOS) || os(Linux)
public final class LocalMirrorExecutableVerifier: MirrorSourceVerifying, @unchecked Sendable {
    private static let maximumRequestBytes = 40 * 1024 * 1024
    private static let maximumResponseBytes = 1024 * 1024
    private static let maximumEvidenceBytes = (maximumRequestBytes - 64 * 1024) / 2
    private static let maximumExecutableBytes: UInt64 = 512 * 1024 * 1024
    private static let maximumConfigurationBytes: UInt64 = 16 * 1024 * 1024

    private let executable: URL
    private let configuration: URL
    private let executableDigest: Data
    private let configurationDigest: Data
    private let timeout: TimeInterval

    public init(executable: URL, configuration: URL, timeout: TimeInterval = 120) throws {
        guard timeout >= 0.1, timeout <= 120 else {
            throw MirrorErrorCode.configuration
        }
        let executableInput = try Self.trustedInput(
            executable, executable: true, maximum: Self.maximumExecutableBytes)
        let configurationInput = try Self.trustedInput(
            configuration, executable: false, maximum: Self.maximumConfigurationBytes)
        self.executable = executableInput.url
        self.configuration = configurationInput.url
        self.executableDigest = executableInput.digest
        self.configurationDigest = configurationInput.digest
        self.timeout = timeout
    }

    public func receipt(batchNumber: UInt64, policy: MirrorPolicy,
                        canonicalReceipt: Data) throws -> MirrorVerification {
        guard canonicalReceipt.count <= Self.maximumEvidenceBytes else {
            throw MirrorErrorCode.bounds
        }
        return try verify(batchNumber: batchNumber, policy: policy,
                          evidence: ["kind": "receipt", "canonical_hex": canonicalReceipt.hex])
    }

    public func state(batchNumber: UInt64, policy: MirrorPolicy, canonicalState: Data,
                      canonicalProof: Data) throws -> MirrorVerification {
        guard canonicalState.count <= Self.maximumEvidenceBytes,
              canonicalProof.count <= Self.maximumEvidenceBytes - canonicalState.count else {
            throw MirrorErrorCode.bounds
        }
        return try verify(batchNumber: batchNumber, policy: policy,
                          evidence: ["kind": "state", "canonical_hex": canonicalState.hex,
                                     "proof_hex": canonicalProof.hex])
    }

    private func verify(batchNumber: UInt64, policy: MirrorPolicy,
                        evidence: [String: Any]) throws -> MirrorVerification {
        guard batchNumber > 0, !policy.candidates.isEmpty,
              policy.candidates.count <= MirrorSchemaV2.maximumSources else {
            throw MirrorErrorCode.configuration
        }
        var seen = Set<Int>()
        let candidates = try policy.candidates.map { value -> [String: Any] in
            guard value.source >= 0, seen.insert(value.source).inserted,
                  value.commitment.count == 32 else {
                throw MirrorErrorCode.configuration
            }
            return ["source": value.source, "commitment_hex": value.commitment.hex]
        }
        let wirePolicy: [String: Any]
        switch policy.kind {
        case .exact:
            guard candidates.count == 1 else { throw MirrorErrorCode.configuration }
            wirePolicy = ["kind": "exact", "candidate": candidates[0]]
        case .orderedPreference:
            wirePolicy = ["kind": "ordered-preference", "candidates": candidates]
        case .agreement:
            guard policy.minimum > 0, policy.minimum <= candidates.count else {
                throw MirrorErrorCode.configuration
            }
            wirePolicy = ["kind": "agreement", "candidates": candidates,
                          "minimum": policy.minimum]
        }
        let request = try JSONSerialization.data(withJSONObject:
            ["batch_number": batchNumber.description, "evidence": evidence,
             "policy": wirePolicy])
        guard request.count <= Self.maximumRequestBytes else { throw MirrorErrorCode.bounds }

        let process = Process()
        let input = Pipe()
        let output = Pipe()
        try requireTrustedInputs()
        process.executableURL = executable
        process.arguments = [configuration.path]
        process.standardInput = input
        process.standardOutput = output
        process.standardError = FileHandle.nullDevice
        let retained = BoundedOutput(maximum: Self.maximumResponseBytes)
        let inputState = ProcessInputState()
        let inputGroup = DispatchGroup()
        let outputGroup = DispatchGroup()
        do {
            try process.run()
        } catch {
            throw MirrorErrorCode.unavailable
        }
        outputGroup.enter()
        DispatchQueue.global(qos: .utility).async {
            defer { outputGroup.leave() }
            do {
                while let chunk = try output.fileHandleForReading.read(upToCount: 8192),
                      !chunk.isEmpty {
                    retained.append(chunk)
                }
            } catch {
                retained.fail()
            }
        }
        inputGroup.enter()
        DispatchQueue.global(qos: .utility).async {
            defer {
                try? input.fileHandleForWriting.close()
                inputGroup.leave()
            }
            do {
                try input.fileHandleForWriting.write(contentsOf: request)
            } catch {
                inputState.fail()
            }
        }

        let deadline = DispatchTime.now() + .milliseconds(Int(timeout * 1_000))
        while process.isRunning && DispatchTime.now() < deadline {
            Thread.sleep(forTimeInterval: 0.005)
        }
        if process.isRunning {
            process.terminate()
            let grace = DispatchTime.now() + .milliseconds(100)
            while process.isRunning && DispatchTime.now() < grace {
                Thread.sleep(forTimeInterval: 0.005)
            }
            if process.isRunning {
                Self.forceKill(process.processIdentifier)
            }
            try? input.fileHandleForWriting.close()
            try? output.fileHandleForReading.close()
            throw MirrorErrorCode.unavailable
        }
        guard inputGroup.wait(timeout: .now() + .seconds(1)) == .success,
              outputGroup.wait(timeout: .now() + .seconds(1)) == .success,
              !inputState.failed, !retained.failed, process.terminationStatus == 0 else {
            throw MirrorErrorCode.unavailable
        }
        try requireTrustedInputs()
        guard !retained.exceeded else { throw MirrorErrorCode.bounds }
        return try parse(retained.data, requestedBatch: batchNumber, policy: policy)
    }

    private func parse(_ bytes: Data, requestedBatch: UInt64,
                       policy: MirrorPolicy) throws -> MirrorVerification {
        guard let response = try JSONSerialization.jsonObject(with: bytes) as? [String: Any]
        else { throw MirrorErrorCode.malformed }
        guard response["ok"] as? Bool == true else {
            throw MirrorErrorCode(rawValue: response["error"] as? String ?? "malformed")
                ?? MirrorErrorCode.malformed
        }
        guard let value = response["verification"] as? [String: Any],
              let level = text(value["level"], maximum: 64),
              let batch = uint64(value["batchNumber"]), batch == requestedBatch,
              let header = Data(hex: value["headerDigest"] as? String), header.count == 32,
              let digest = Data(hex: value["evidenceDigest"] as? String), digest.count == 32,
              let source = text(value["sourceId"], maximum: 64),
              let target = text(value["target"], maximum: 2048),
              let position = text(value["canonicalPosition"], maximum: 2048),
              let provenance = text(value["provenance"], maximum: 16),
              provenance == "Canonical" || provenance == "Reorged",
              let lag = text(value["batchLag"], maximum: 64),
              let failover = integer(value["failoverCount"]), failover >= 0,
              failover < policy.candidates.count,
              let agreeing = integer(value["agreeingSources"]), agreeing > 0,
              agreeing <= policy.candidates.count,
              policy.kind != .agreement || agreeing >= policy.minimum,
              let checkpoint = text(value["checkpointLevel"], maximum: 32),
              checkpoint == "unavailable" else {
            throw MirrorErrorCode.malformed
        }
        let latest: UInt64?
        if let raw = value["latestBatch"], !(raw is NSNull) {
            guard let parsed = uint64(raw) else { throw MirrorErrorCode.malformed }
            latest = parsed
        } else {
            latest = nil
        }
        return MirrorVerification(level: level, batchNumber: batch, headerDigest: header,
            evidenceDigest: digest, sourceID: source, target: target,
            canonicalPosition: position, provenance: provenance, latestBatch: latest,
            batchLag: lag, failoverCount: failover, agreeingSources: agreeing,
            checkpointLevel: checkpoint)
    }

    private struct TrustedInput {
        let url: URL
        let digest: Data
    }

    private static func trustedInput(_ url: URL, executable: Bool,
                                     maximum: UInt64) throws -> TrustedInput {
        let normalized = url.standardizedFileURL
        guard url.isFileURL, url.path.hasPrefix("/"), normalized.path == url.path,
              url.resolvingSymlinksInPath().path == url.path else {
            throw MirrorErrorCode.configuration
        }
        var current = url
        while current.path != "/" {
            let values = try current.resourceValues(forKeys: [
                .isRegularFileKey, .isDirectoryKey, .isSymbolicLinkKey,
            ])
            guard values.isSymbolicLink != true else { throw MirrorErrorCode.configuration }
            if current.path == url.path {
                guard values.isRegularFile == true else { throw MirrorErrorCode.configuration }
            } else {
                guard values.isDirectory == true else { throw MirrorErrorCode.configuration }
            }
            try requireProtectedOwnerAndMode(current)
            current.deleteLastPathComponent()
        }
        try requireProtectedOwnerAndMode(current)
        guard !executable || FileManager.default.isExecutableFile(atPath: url.path) else {
            throw MirrorErrorCode.configuration
        }
        let before = try FileManager.default.attributesOfItem(atPath: url.path)
        guard let declaredSize = (before[.size] as? NSNumber)?.uint64Value,
              declaredSize <= maximum else { throw MirrorErrorCode.configuration }
        let handle = try FileHandle(forReadingFrom: url)
        defer { try? handle.close() }
        var hasher = SHA256()
        var total: UInt64 = 0
        while let chunk = try handle.read(upToCount: 64 * 1024), !chunk.isEmpty {
            total += UInt64(chunk.count)
            guard total <= maximum else { throw MirrorErrorCode.configuration }
            hasher.update(data: chunk)
        }
        let after = try FileManager.default.attributesOfItem(atPath: url.path)
        guard total == declaredSize,
              stableIdentity(before) == stableIdentity(after) else {
            throw MirrorErrorCode.configuration
        }
        return TrustedInput(url: url, digest: Data(hasher.finalize()))
    }

    private static func requireProtectedOwnerAndMode(_ url: URL) throws {
        let attributes = try FileManager.default.attributesOfItem(atPath: url.path)
        guard let owner = (attributes[.ownerAccountID] as? NSNumber)?.uint32Value,
              owner == 0 || owner == geteuid(),
              let permissions = (attributes[.posixPermissions] as? NSNumber)?.uint16Value,
              permissions & 0o022 == 0 else {
            throw MirrorErrorCode.configuration
        }
    }

    private static func stableIdentity(_ attributes: [FileAttributeKey: Any]) -> String {
        let system = (attributes[.systemNumber] as? NSNumber)?.stringValue ?? ""
        let file = (attributes[.systemFileNumber] as? NSNumber)?.stringValue ?? ""
        let size = (attributes[.size] as? NSNumber)?.stringValue ?? ""
        let modified = (attributes[.modificationDate] as? Date)?.timeIntervalSince1970.description ?? ""
        return "\(system):\(file):\(size):\(modified)"
    }

    private func requireTrustedInputs() throws {
        let currentExecutable = try Self.trustedInput(
            executable, executable: true, maximum: Self.maximumExecutableBytes)
        let currentConfiguration = try Self.trustedInput(
            configuration, executable: false, maximum: Self.maximumConfigurationBytes)
        guard currentExecutable.digest == executableDigest,
              currentConfiguration.digest == configurationDigest else {
            throw MirrorErrorCode.configuration
        }
    }

    private static func forceKill(_ identifier: Int32) {
        #if canImport(Darwin)
        _ = Darwin.kill(identifier, SIGKILL)
        #elseif canImport(Glibc)
        _ = Glibc.kill(identifier, SIGKILL)
        #endif
    }
}

private final class BoundedOutput: @unchecked Sendable {
    private let lock = NSLock()
    private let maximum: Int
    private var retained = Data()
    private(set) var exceeded = false
    private(set) var failed = false

    init(maximum: Int) { self.maximum = maximum }

    func append(_ chunk: Data) {
        lock.lock()
        defer { lock.unlock() }
        let keep = min(max(maximum - retained.count, 0), chunk.count)
        if keep > 0 { retained.append(contentsOf: chunk.prefix(keep)) }
        if keep != chunk.count { exceeded = true }
    }

    func fail() {
        lock.lock()
        failed = true
        lock.unlock()
    }

    var data: Data {
        lock.lock()
        defer { lock.unlock() }
        return retained
    }
}

private final class ProcessInputState: @unchecked Sendable {
    private let lock = NSLock()
    private var value = false
    func fail() {
        lock.lock()
        value = true
        lock.unlock()
    }
    var failed: Bool {
        lock.lock()
        defer { lock.unlock() }
        return value
    }
}

private extension Data {
    var hex: String { map { String(format: "%02x", $0) }.joined() }
    init?(hex: String?) {
        guard let hex, hex.count % 2 == 0 else { return nil }
        var value = Data()
        var index = hex.startIndex
        while index < hex.endIndex {
            let end = hex.index(index, offsetBy: 2)
            guard let byte = UInt8(hex[index..<end], radix: 16) else { return nil }
            value.append(byte)
            index = end
        }
        self = value
    }
}

private func uint64(_ value: Any?) -> UInt64? {
    guard let text = value as? String, !text.isEmpty, text.first != "0",
          text.allSatisfy({ $0.isASCII && $0.isNumber }), let result = UInt64(text),
          result > 0 else { return nil }
    return result
}

private func integer(_ value: Any?) -> Int? {
    guard let number = value as? NSNumber, !(value is Bool),
          let result = Int(number.stringValue), String(result) == number.stringValue
    else { return nil }
    return result
}

private func text(_ value: Any?, maximum: Int) -> String? {
    guard let value = value as? String, !value.isEmpty, value.utf8.count <= maximum
    else { return nil }
    return value
}
#endif
