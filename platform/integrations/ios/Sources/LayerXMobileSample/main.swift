import Foundation
import LayerXMobile
import LayerXMobileSampleKit
import LayerXSDK
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif

func fail(_ reason: String) -> Never {
    FileHandle.standardError.write(Data("layerx-ios-sample: \(reason)\n".utf8))
    exit(2)
}

func required(_ name: String, in environment: [String: String]) -> String {
    guard let value = environment[name], !value.isEmpty else {
        fail("missing \(name)")
    }
    return value
}

func emit(_ value: [String: JSONValue]) {
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.sortedKeys]
    guard let data = try? encoder.encode(JSONValue.object(value)) else {
        fail("encode failure")
    }
    FileHandle.standardOutput.write(data)
    FileHandle.standardOutput.write(Data("\n".utf8))
}

func decodeJSON(_ text: String) -> JSONValue {
    guard let value = try? JSONDecoder().decode(JSONValue.self, from: Data(text.utf8)) else {
        fail("invalid json payload")
    }
    return value
}

struct SignedWebhookDelivery: Decodable {
    let body: String
    let headers: [String: String]
}

func signedWebhookDelivery(at path: String) -> (Data, [String: String]) {
    guard let encoded = FileManager.default.contents(atPath: path),
          let delivery = try? JSONDecoder().decode(SignedWebhookDelivery.self, from: encoded),
          let body = Data(base64Encoded: delivery.body), !body.isEmpty else {
        fail("invalid signed webhook delivery")
    }
    return (body, delivery.headers)
}

let environment = ProcessInfo.processInfo.environment

let configuration: PublishableConfiguration
do {
    configuration = try SampleEnvironment.configuration(from: environment)
} catch {
    fail("configuration refused: \((error as? MobileIntegrationError)?.code.rawValue ?? "invalid-configuration")")
}

let urlSession = SampleEnvironment.session(timeoutSeconds: configuration.requestTimeoutSeconds)

let mobile: LayerXMobile
do {
    mobile = try LayerXMobile(configuration: configuration, session: urlSession)
} catch {
    fail("client refused: \((error as? MobileIntegrationError)?.code.rawValue ?? "invalid-configuration")")
}

let relay = required("LAYERX_RECEIPT_RELAY_URL", in: environment)
guard let relayURL = URL(string: relay) else { fail("invalid LAYERX_RECEIPT_RELAY_URL") }
let resolver: RelayReceiptResolver
do {
    resolver = try RelayReceiptResolver(relayURL: relayURL, session: urlSession)
} catch {
    fail("relay refused")
}

let model = WalletModel(mobile: mobile, receipts: resolver)
var report: [String: JSONValue] = [:]
var verified = false

let refreshed = await model.refresh()
report["service_version"] = .string(refreshed.serviceVersion)
report["activity_count"] = .integer(Int64(refreshed.activityCount))
if let refusal = refreshed.refusal {
    report["refusal"] = .string(refusal)
    emit(report)
    exit(3)
}

let expectation: MobileSettlementExpectation
do {
    expectation = try MobileSettlementExpectation(
        asset: try RelayReceiptResolver.hex32(required("LAYERX_SAMPLE_ASSET", in: environment)),
        recipient: try RelayReceiptResolver.hex32(required("LAYERX_SAMPLE_RECIPIENT", in: environment)),
        amount: required("LAYERX_SAMPLE_AMOUNT", in: environment)
    )
} catch {
    fail("invalid settlement expectation")
}

guard let key = IdempotencyKey(rawValue: required("LAYERX_SAMPLE_IDEMPOTENCY_KEY", in: environment)) else {
    fail("invalid LAYERX_SAMPLE_IDEMPOTENCY_KEY")
}

let paid = await model.pay(
    quote: decodeJSON(required("LAYERX_SAMPLE_QUOTE_JSON", in: environment)),
    expecting: expectation,
    idempotencyKey: key
)

switch paid.settlement {
case let .some(.verified(level, digest)):
    report["settlement"] = .string(level)
    report["receipt_digest"] = .string(digest)
    verified = true
case let .some(.pending(reference)):
    let settled = await model.awaitSettlement(
        journeyID: reference,
        expecting: expectation,
        attempts: 20,
        wait: { nanoseconds in try await Task.sleep(nanoseconds: nanoseconds) }
    )
    switch settled.settlement {
    case let .some(.verified(level, digest)):
        report["settlement"] = .string(level)
        report["receipt_digest"] = .string(digest)
        verified = true
    case let .some(.refused(code)):
        report["settlement"] = .string("refused")
        report["refusal"] = .string(code)
    default:
        report["settlement"] = .string("pending")
    }
case let .some(.refused(code)):
    report["settlement"] = .string("refused")
    report["refusal"] = .string(code)
case .none:
    report["settlement"] = .string("refused")
    report["refusal"] = .string(paid.refusal ?? "verification-failure")
}

if let deliveryPath = environment["LAYERX_SAMPLE_WEBHOOK_DELIVERY_PATH"] {
    let (body, headers) = signedWebhookDelivery(at: deliveryPath)
    var tampered = body
    tampered[tampered.index(before: tampered.endIndex)] ^= 1
    let rejected = await model.deliver(rawBody: tampered, headerFields: headers)
    guard rejected.refusal == MobileErrorCode.invalidEvent.rawValue else {
        fail("tampered event was not rejected")
    }
    let first = await model.deliver(rawBody: body, headerFields: headers)
    if let refusal = first.refusal {
        report["event"] = .string("refused")
        report["refusal"] = .string(refusal)
        emit(report)
        exit(4)
    }
    let replayed = await model.deliver(rawBody: body, headerFields: headers)
    report["event_tamper"] = .string("rejected")
    report["event"] = .string("verified")
    report["event_replay"] = .string(replayed.deliveries.last ?? "duplicate")
} else if let eventPath = environment["LAYERX_SAMPLE_EVENT_PATH"] {
    guard let body = FileManager.default.contents(atPath: eventPath) else {
        fail("missing event payload at \(eventPath)")
    }
    let headers: [String: String] = [
        EventEnvelopeHeaders.idHeader: required("LAYERX_SAMPLE_EVENT_ID", in: environment),
        EventEnvelopeHeaders.timestampHeader: required("LAYERX_SAMPLE_EVENT_TIMESTAMP", in: environment),
        EventEnvelopeHeaders.keyIDHeader: required("LAYERX_SAMPLE_EVENT_KEY_ID", in: environment),
        EventEnvelopeHeaders.signatureHeader: required("LAYERX_SAMPLE_EVENT_SIGNATURE", in: environment),
    ]
    let first = await model.deliver(rawBody: body, headerFields: headers)
    if let refusal = first.refusal {
        report["event"] = .string("refused")
        report["refusal"] = .string(refusal)
        emit(report)
        exit(4)
    }
    let replayed = await model.deliver(rawBody: body, headerFields: headers)
    report["event"] = .string("verified")
    report["event_replay"] = .string(replayed.deliveries.last ?? "duplicate")
}

report["integration"] = .string(platform_int_ios().name)
emit(report)
exit(verified ? 0 : 5)
