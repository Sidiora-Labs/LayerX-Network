// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "LayerXSDK",
    platforms: [.iOS(.v15), .macOS(.v12)],
    products: [.library(name: "LayerXSDK", targets: ["LayerXSDK"])],
    dependencies: [
        .package(url: "https://github.com/apple/swift-crypto.git", exact: "3.12.5")
    ],
    targets: [
        .target(
            name: "LayerXSDK",
            dependencies: [.product(name: "Crypto", package: "swift-crypto")],
            path: "Sources/LayerXSDK"
        ),
        .testTarget(
            name: "LayerXSDKTests",
            dependencies: ["LayerXSDK"],
            path: "Tests/LayerXSDKTests"
        )
    ]
)
