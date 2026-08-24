import Foundation
import LayerXSDK
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif

public struct MobileIntegrationMetadata: Sendable, Equatable {
    public let name: String
    public let version: String
    public let sdk: SDKMetadata
    public let credentialModel: String
    public let eventVerification: String
    public let replayProtection: String

    public init(
        name: String,
        version: String,
        sdk: SDKMetadata,
        credentialModel: String,
        eventVerification: String,
        replayProtection: String
    ) {
        self.name = name
        self.version = version
        self.sdk = sdk
        self.credentialModel = credentialModel
        self.eventVerification = eventVerification
        self.replayProtection = replayProtection
    }
}

public final class LayerXMobile: @unchecked Sendable {
    public let configuration: PublishableConfiguration
    public let client: LayerXMobileClient
    public let events: VerifiedEventConsumer
    public let sessions: SessionTokenProvider

    public init(
        configuration: PublishableConfiguration,
        session: URLSession = .shared,
        deliveries: EventDeliveryStore? = nil,
        deliveryStoreURL: URL? = nil,
        telemetry: SDKTelemetry? = nil
    ) throws {
        let provider = try BrokeredSessionTokenProvider(brokerURL: configuration.sessionBrokerURL, session: session)
        self.configuration = configuration
        self.sessions = provider
        self.client = LayerXMobileClient(
            configuration: configuration,
            sessions: provider,
            session: session,
            telemetry: telemetry
        )
        let deliveryStore = try deliveries ?? FileEventDeliveryStore(
            fileURL: deliveryStoreURL ?? FileEventDeliveryStore.applicationSupportURL()
        )
        self.events = try VerifiedEventConsumer(
            configuration: configuration,
            deliveries: deliveryStore
        )
    }

    public convenience init(declaredKeys: [String: String], session: URLSession = .shared) throws {
        try self.init(configuration: try PublishableConfiguration(declaredKeys: declaredKeys), session: session)
    }

    public func gate(receipts: MobileReceiptResolver) -> ReceiptGate {
        ReceiptGate(receipts: receipts)
    }

    @discardableResult
    public func consume(
        rawBody: Data,
        headerFields: [String: String],
        handle: (JSONValue, String) async throws -> Void
    ) async throws -> EventConsumeOutcome {
        try await events.consume(rawBody: rawBody, headers: try EventEnvelopeHeaders(fields: headerFields), handle: handle)
    }
}

private let integrationMetadata = MobileIntegrationMetadata(
    name: "LayerXMobile",
    version: "0.1.0",
    sdk: platform_sdk_swift(),
    credentialModel: "brokered-ephemeral-session-token",
    eventVerification: "ed25519-v1",
    replayProtection: "durable-leased-delivery-claim"
)

public func platform_int_ios() -> MobileIntegrationMetadata { integrationMetadata }
