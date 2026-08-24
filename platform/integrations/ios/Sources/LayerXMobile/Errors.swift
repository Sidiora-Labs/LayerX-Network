import Foundation

public enum MobileErrorCode: String, Sendable {
    case invalidConfiguration = "invalid-configuration"
    case embeddedSecret = "embedded-secret"
    case invalidSession = "invalid-session"
    case sessionExpired = "session-expired"
    case invalidEvent = "invalid-event"
    case eventReplay = "event-replay"
    case deliveryStoreFailure = "delivery-store-failure"
    case verificationFailure = "verification-failure"
    case decodeFailure = "decode-failure"
    case transportFailure = "transport-failure"
}

public struct MobileIntegrationError: Error, Sendable, Equatable, CustomStringConvertible {
    public let code: MobileErrorCode

    public init(_ code: MobileErrorCode) {
        self.code = code
    }

    public var description: String { "LayerX mobile error: \(code.rawValue)" }
}
