// swift-tools-version: 5.9
// BoxCore: what the iOS app shares with tests – the data from the BOXBOXBOX server (snapshot,
// strategy plan), the connection to it and sample snapshots of a real race (Spa, 23 Nov 2026).
import PackageDescription

let package = Package(
    name: "BoxCore",
    platforms: [.iOS(.v17), .macOS(.v14)],
    products: [.library(name: "BoxCore", targets: ["BoxCore"])],
    targets: [
        .target(name: "BoxCore", resources: [.copy("Samples")]),
        .testTarget(name: "BoxCoreTests", dependencies: ["BoxCore"]),
    ]
)
