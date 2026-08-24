import Foundation
import LayerXMobile
import LayerXMobileSampleKit
import LayerXSDK
import SwiftUI

struct WalletLaunch {
    let model: WalletModel
    let quoteRequest: JSONValue
    let expectation: MobileSettlementExpectation

    static func make(bundle: Bundle = .main) -> Result<WalletLaunch, MobileIntegrationError> {
        do {
            let configuration = try SampleEnvironment.configuration(bundle: bundle)
            let session = SampleEnvironment.session(timeoutSeconds: configuration.requestTimeoutSeconds)
            guard let bundleIdentifier = bundle.bundleIdentifier else {
                throw MobileIntegrationError(.invalidConfiguration)
            }
            let mobile = try LayerXMobile(
                configuration: configuration,
                session: session,
                deliveryStoreURL: try FileEventDeliveryStore.applicationSupportURL(
                    applicationIdentifier: bundleIdentifier
                )
            )
            guard let sample = bundle.object(forInfoDictionaryKey: "LayerXSample") as? [String: Any],
                  let relay = sample["receipt_relay_url"] as? String,
                  let relayURL = URL(string: relay),
                  let asset = sample["asset"] as? String,
                  let recipient = sample["recipient"] as? String,
                  let amount = sample["amount"] as? String,
                  let quote = sample["quote_json"] as? String,
                  let request = try? JSONDecoder().decode(JSONValue.self, from: Data(quote.utf8)) else {
                throw MobileIntegrationError(.invalidConfiguration)
            }
            let resolver = try RelayReceiptResolver(relayURL: relayURL, session: session)
            let expectation = try MobileSettlementExpectation(
                asset: try RelayReceiptResolver.hex32(asset),
                recipient: try RelayReceiptResolver.hex32(recipient),
                amount: amount
            )
            return .success(WalletLaunch(
                model: WalletModel(mobile: mobile, receipts: resolver),
                quoteRequest: request,
                expectation: expectation
            ))
        } catch let error as MobileIntegrationError {
            return .failure(error)
        } catch {
            return .failure(MobileIntegrationError(.invalidConfiguration))
        }
    }
}

struct RefusalScreen: View {
    let code: String

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("LayerX Wallet").font(.title2).bold()
            Text("The publishable configuration was refused.").font(.subheadline)
            Text(code).font(.footnote).foregroundColor(.red)
            Text("Correct the LayerX dictionary in Info.plist. The app carries no credential of its own; "
                 + "every session token is minted by the session broker at run time.")
                .font(.caption)
                .foregroundColor(.secondary)
        }
        .padding()
    }
}

@main
struct LayerXWalletApp: App {
    private let launch = WalletLaunch.make()

    var body: some Scene {
        WindowGroup {
            switch launch {
            case let .success(context):
                WalletScreen(
                    model: context.model,
                    quoteRequest: context.quoteRequest,
                    expectation: context.expectation
                )
                .padding()
            case let .failure(error):
                RefusalScreen(code: error.code.rawValue)
            }
        }
    }
}
