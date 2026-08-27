// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "LayerXMobile",
    platforms: [.iOS(.v15), .macOS(.v12)],
    products: [
        .library(name: "LayerXMobile", targets: ["LayerXMobile"]),
        .library(name: "LayerXMobileSampleKit", targets: ["LayerXMobileSampleKit"]),
        .executable(name: "layerx-ios-sample", targets: ["LayerXMobileSample"]),
        .executable(name: "layerx-ios-secret-scan", targets: ["LayerXMobileSecretScan"]),
    ],
    dependencies: [
        .package(path: "../../sdk/swift"),
        .package(url: "https://github.com/apple/swift-crypto.git", exact: "3.12.5"),
    ],
    targets: [
        .target(
            name: "LayerXMobile",
            dependencies: [
                .product(name: "LayerXSDK", package: "swift"),
                .product(name: "Crypto", package: "swift-crypto"),
            ],
            path: "Sources/LayerXMobile"
        ),
        .target(
            name: "LayerXMobileSampleKit",
            dependencies: ["LayerXMobile", .product(name: "LayerXSDK", package: "swift")],
            path: "Sources/LayerXMobileSampleKit"
        ),
        .executableTarget(
            name: "LayerXMobileSample",
            dependencies: ["LayerXMobile", "LayerXMobileSampleKit", .product(name: "LayerXSDK", package: "swift")],
            path: "Sources/LayerXMobileSample"
        ),
        .executableTarget(
            name: "LayerXMobileSecretScan",
            dependencies: ["LayerXMobile"],
            path: "Sources/LayerXMobileSecretScan"
        ),
    ]
)
