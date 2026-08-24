import Crypto
import Foundation
import LayerXSDK

public struct EventEnvelopeHeaders: Sendable, Equatable {
    public static let idHeader = "LayerX-Delivery-Id"
    public static let timestampHeader = "LayerX-Timestamp"
    public static let keyIDHeader = "LayerX-Key-Id"
    public static let signatureHeader = "LayerX-Signature"

    public let id: String
    public let timestamp: String
    public let keyID: String
    public let signature: String

    public init(id: String, timestamp: String, keyID: String, signature: String) {
        self.id = id
        self.timestamp = timestamp
        self.keyID = keyID
        self.signature = signature
    }

    public init(fields: [String: String]) throws {
        var normalized: [String: String] = [:]
        for (name, value) in fields {
            normalized[name.lowercased()] = value
        }
        guard let id = normalized[Self.idHeader.lowercased()],
              let timestamp = normalized[Self.timestampHeader.lowercased()],
              let keyID = normalized[Self.keyIDHeader.lowercased()],
              let signature = normalized[Self.signatureHeader.lowercased()] else {
            throw MobileIntegrationError(.invalidEvent)
        }
        self.init(id: id, timestamp: timestamp, keyID: keyID, signature: signature)
    }
}

public enum EventDeliveryClaim: String, Sendable {
    case claimed, processing, completed, conflict
}

public protocol EventDeliveryStore: Sendable {
    func claim(deliveryID: String, payloadDigest: String, leaseUntilMilliseconds: Int64) async throws -> EventDeliveryClaim
    func complete(deliveryID: String, payloadDigest: String) async throws
    func release(deliveryID: String, payloadDigest: String) async throws
}

public actor InMemoryEventDeliveryStore: EventDeliveryStore {
    private struct Entry {
        let payloadDigest: String
        var leaseUntilMilliseconds: Int64
        var completed: Bool
    }

    private var entries: [String: Entry] = [:]
    private let now: @Sendable () -> Int64
    private let capacity: Int

    public init(capacity: Int = 8_192, now: (@Sendable () -> Int64)? = nil) {
        self.capacity = max(capacity, 1)
        self.now = now ?? { Int64(Date().timeIntervalSince1970 * 1_000) }
    }

    public func claim(deliveryID: String, payloadDigest: String, leaseUntilMilliseconds: Int64) throws -> EventDeliveryClaim {
        if let existing = entries[deliveryID] {
            guard existing.payloadDigest == payloadDigest else { return .conflict }
            if existing.completed { return .completed }
            if existing.leaseUntilMilliseconds > now() { return .processing }
            entries[deliveryID] = Entry(payloadDigest: payloadDigest, leaseUntilMilliseconds: leaseUntilMilliseconds, completed: false)
            return .claimed
        }
        if entries.count >= capacity {
            evictOldest()
        }
        entries[deliveryID] = Entry(payloadDigest: payloadDigest, leaseUntilMilliseconds: leaseUntilMilliseconds, completed: false)
        return .claimed
    }

    public func complete(deliveryID: String, payloadDigest: String) throws {
        guard var entry = entries[deliveryID], entry.payloadDigest == payloadDigest else {
            throw MobileIntegrationError(.eventReplay)
        }
        entry.completed = true
        entry.leaseUntilMilliseconds = 0
        entries[deliveryID] = entry
    }

    public func release(deliveryID: String, payloadDigest: String) throws {
        guard let entry = entries[deliveryID], entry.payloadDigest == payloadDigest, !entry.completed else { return }
        entries.removeValue(forKey: deliveryID)
    }

    private func evictOldest() {
        let timestamp = now()
        for (identifier, entry) in entries where entry.completed || entry.leaseUntilMilliseconds <= timestamp {
            entries.removeValue(forKey: identifier)
        }
        if entries.count >= capacity, let victim = entries.keys.first {
            entries.removeValue(forKey: victim)
        }
    }
}

public actor FileEventDeliveryStore: EventDeliveryStore {
    private struct Entry: Codable {
        let payloadDigest: String
        var leaseUntilMilliseconds: Int64
        var completed: Bool
    }

    private struct Ledger: Codable {
        let version: Int
        var entries: [String: Entry]
    }

    private let fileURL: URL
    private let capacity: Int
    private let now: @Sendable () -> Int64
    private var entries: [String: Entry]

    public init(
        fileURL: URL,
        capacity: Int = 65_536,
        now: (@Sendable () -> Int64)? = nil
    ) throws {
        guard fileURL.isFileURL, capacity > 0 else {
            throw MobileIntegrationError(.invalidConfiguration)
        }
        self.fileURL = fileURL.standardizedFileURL
        self.capacity = capacity
        self.now = now ?? { Int64(Date().timeIntervalSince1970 * 1_000) }
        let loaded = try Self.load(fileURL: self.fileURL)
        guard loaded.count <= capacity else {
            throw MobileIntegrationError(.deliveryStoreFailure)
        }
        self.entries = loaded
    }

    public static func applicationSupportURL(
        applicationIdentifier: String = "com.sidiora.layerx.mobile"
    ) throws -> URL {
        guard !applicationIdentifier.isEmpty,
              applicationIdentifier.utf8.count <= 255,
              applicationIdentifier.allSatisfy({ $0.isASCII && ($0.isLetter || $0.isNumber || $0 == "." || $0 == "-") }),
              let root = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first else {
            throw MobileIntegrationError(.invalidConfiguration)
        }
        return root
            .appendingPathComponent(applicationIdentifier, isDirectory: true)
            .appendingPathComponent("layerx-event-deliveries-v1.json", isDirectory: false)
    }

    public func claim(
        deliveryID: String,
        payloadDigest: String,
        leaseUntilMilliseconds: Int64
    ) throws -> EventDeliveryClaim {
        guard Self.validDeliveryID(deliveryID), Self.validDigest(payloadDigest), leaseUntilMilliseconds > 0 else {
            throw MobileIntegrationError(.deliveryStoreFailure)
        }
        if let existing = entries[deliveryID] {
            guard existing.payloadDigest == payloadDigest else { return .conflict }
            if existing.completed { return .completed }
            if existing.leaseUntilMilliseconds > now() { return .processing }
            entries[deliveryID] = Entry(
                payloadDigest: payloadDigest,
                leaseUntilMilliseconds: leaseUntilMilliseconds,
                completed: false
            )
            try persist()
            return .claimed
        }
        if entries.count >= capacity {
            evict()
        }
        guard entries.count < capacity else {
            throw MobileIntegrationError(.deliveryStoreFailure)
        }
        entries[deliveryID] = Entry(
            payloadDigest: payloadDigest,
            leaseUntilMilliseconds: leaseUntilMilliseconds,
            completed: false
        )
        try persist()
        return .claimed
    }

    public func complete(deliveryID: String, payloadDigest: String) throws {
        guard Self.validDeliveryID(deliveryID), Self.validDigest(payloadDigest) else {
            throw MobileIntegrationError(.deliveryStoreFailure)
        }
        guard var entry = entries[deliveryID], entry.payloadDigest == payloadDigest else {
            throw MobileIntegrationError(.eventReplay)
        }
        entry.completed = true
        entry.leaseUntilMilliseconds = 0
        entries[deliveryID] = entry
        try persist()
    }

    public func release(deliveryID: String, payloadDigest: String) throws {
        guard Self.validDeliveryID(deliveryID), Self.validDigest(payloadDigest) else {
            throw MobileIntegrationError(.deliveryStoreFailure)
        }
        guard let entry = entries[deliveryID], entry.payloadDigest == payloadDigest, !entry.completed else { return }
        entries.removeValue(forKey: deliveryID)
        try persist()
    }

    private func evict() {
        let timestamp = now()
        for (identifier, entry) in entries where !entry.completed && entry.leaseUntilMilliseconds <= timestamp {
            entries.removeValue(forKey: identifier)
        }
        if entries.count >= capacity {
            return
        }
    }

    private func persist() throws {
        let directory = fileURL.deletingLastPathComponent()
        do {
            try FileManager.default.createDirectory(
                at: directory,
                withIntermediateDirectories: true,
                attributes: [.posixPermissions: 0o700]
            )
            try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: directory.path)
            let data = try JSONEncoder().encode(Ledger(version: 1, entries: entries))
            try data.write(to: fileURL, options: [.atomic])
            try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: fileURL.path)
        } catch {
            throw MobileIntegrationError(.deliveryStoreFailure)
        }
    }

    private static func load(fileURL: URL) throws -> [String: Entry] {
        guard FileManager.default.fileExists(atPath: fileURL.path) else { return [:] }
        do {
            let data = try Data(contentsOf: fileURL, options: [.mappedIfSafe])
            guard data.count <= 32 * 1024 * 1024 else {
                throw MobileIntegrationError(.deliveryStoreFailure)
            }
            let ledger = try JSONDecoder().decode(Ledger.self, from: data)
            guard ledger.version == 1, ledger.entries.count <= 65_536,
                  ledger.entries.allSatisfy({
                      validDeliveryID($0.key)
                          && validDigest($0.value.payloadDigest)
                          && $0.value.leaseUntilMilliseconds >= 0
                  }) else {
                throw MobileIntegrationError(.deliveryStoreFailure)
            }
            return ledger.entries
        } catch let error as MobileIntegrationError {
            throw error
        } catch {
            throw MobileIntegrationError(.deliveryStoreFailure)
        }
    }

    private static func validDeliveryID(_ value: String) -> Bool {
        !value.isEmpty && value.utf8.count <= 255 && !value.contains("\0")
    }

    private static func validDigest(_ value: String) -> Bool {
        value.utf8.count == 64 && value.utf8.allSatisfy {
            ($0 >= 48 && $0 <= 57) || ($0 >= 97 && $0 <= 102)
        }
    }
}

public enum EventConsumeOutcome: String, Sendable {
    case processed, duplicate, processing
}

public struct VerifiedEventConsumer: Sendable {
    private let publicKeys: [String: Data]
    private let deliveries: EventDeliveryStore
    private let maximumAgeMilliseconds: Int64
    private let leaseMilliseconds: Int64
    private let now: @Sendable () -> Int64

    public init(
        publicKeys: [String: Data],
        deliveries: EventDeliveryStore,
        maximumAgeMilliseconds: Int64 = 300_000,
        leaseMilliseconds: Int64 = 60_000,
        now: (@Sendable () -> Int64)? = nil
    ) throws {
        guard !publicKeys.isEmpty, maximumAgeMilliseconds > 0, leaseMilliseconds > 0,
              publicKeys.allSatisfy({ $0.value.count == 32 }) else {
            throw MobileIntegrationError(.invalidConfiguration)
        }
        self.publicKeys = publicKeys
        self.deliveries = deliveries
        self.maximumAgeMilliseconds = maximumAgeMilliseconds
        self.leaseMilliseconds = leaseMilliseconds
        self.now = now ?? { Int64(Date().timeIntervalSince1970 * 1_000) }
    }

    public init(configuration: PublishableConfiguration, deliveries: EventDeliveryStore) throws {
        try self.init(
            publicKeys: configuration.eventPublicKeys,
            deliveries: deliveries,
            maximumAgeMilliseconds: configuration.eventMaximumAgeMilliseconds
        )
    }

    @discardableResult
    public func consume(
        rawBody: Data,
        headers: EventEnvelopeHeaders,
        handle: (JSONValue, String) async throws -> Void
    ) async throws -> EventConsumeOutcome {
        let timestamp = now()
        let seconds = try canonicalSeconds(headers.timestamp)
        let issuedAt = seconds * 1_000
        guard boundedText(headers.id, limit: 255),
              identifier(headers.keyID, limit: 64),
              !rawBody.isEmpty, rawBody.count <= 1_048_576,
              issuedAt <= timestamp + 30_000,
              timestamp - issuedAt <= maximumAgeMilliseconds,
              let publicKey = publicKeys[headers.keyID] else {
            throw MobileIntegrationError(.invalidEvent)
        }
        let signature = try parseSignature(headers.signature)
        var message = Data("\(headers.id).\(headers.timestamp).".utf8)
        message.append(rawBody)
        guard let key = try? Curve25519.Signing.PublicKey(rawRepresentation: publicKey),
              key.isValidSignature(signature, for: message) else {
            throw MobileIntegrationError(.invalidEvent)
        }

        let payloadDigest = hexadecimal(Data(SHA256.hash(data: rawBody)))
        let claim = try await deliveries.claim(
            deliveryID: headers.id,
            payloadDigest: payloadDigest,
            leaseUntilMilliseconds: timestamp + leaseMilliseconds
        )
        switch claim {
        case .conflict:
            throw MobileIntegrationError(.eventReplay)
        case .completed:
            return .duplicate
        case .processing:
            return .processing
        case .claimed:
            break
        }

        do {
            let event = try decodeEvent(rawBody)
            try await handle(event, headers.id)
            try await deliveries.complete(deliveryID: headers.id, payloadDigest: payloadDigest)
        } catch {
            try? await deliveries.release(deliveryID: headers.id, payloadDigest: payloadDigest)
            throw error
        }
        return .processed
    }

    private func decodeEvent(_ rawBody: Data) throws -> JSONValue {
        guard let value = try? JSONDecoder().decode(JSONValue.self, from: rawBody), value.objectValue != nil else {
            throw MobileIntegrationError(.decodeFailure)
        }
        return value
    }

    private func canonicalSeconds(_ value: String) throws -> Int64 {
        guard !value.isEmpty, value.utf8.count <= 19,
              value.utf8.allSatisfy({ $0 >= 48 && $0 <= 57 }),
              value == "0" || value.first != "0",
              let seconds = Int64(value), seconds <= 253_402_300_799 else {
            throw MobileIntegrationError(.invalidEvent)
        }
        return seconds
    }

    private func parseSignature(_ value: String) throws -> Data {
        guard value.hasPrefix("v1="),
              let decoded = Data(base64Encoded: String(value.dropFirst(3))),
              decoded.count == 64 else {
            throw MobileIntegrationError(.invalidEvent)
        }
        return decoded
    }

    private func boundedText(_ value: String, limit: Int) -> Bool {
        !value.isEmpty && value.utf8.count <= limit && !value.contains("\0")
    }

    private func identifier(_ value: String, limit: Int) -> Bool {
        boundedText(value, limit: limit) && value.allSatisfy { character in
            character.isASCII && (character.isLetter || character.isNumber || character == "." || character == "_" || character == "-")
        }
    }

    private func hexadecimal(_ bytes: Data) -> String {
        bytes.map { String(format: "%02x", $0) }.joined()
    }
}
