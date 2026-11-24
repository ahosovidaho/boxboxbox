import BoxCore
import SwiftUI
import UIKit

@main
struct BoxboxboxApp: App {
    @StateObject private var live = LiveConnection()
    @StateObject private var announcer = Announcer()
    @AppStorage("server") private var server = "192.168.1.156:8099"
    @AppStorage("demo") private var demo = false
    @AppStorage("awake") private var awake = true
    @Environment(\.scenePhase) private var phase

    var body: some Scene {
        WindowGroup {
            RaceView()
                .environmentObject(live)
                .environmentObject(announcer)
                .preferredColorScheme(.dark)
                .onAppear(perform: start)
                .onChange(of: phase) { _, p in
                    // Back in front: connect again (iOS drops the socket in the background)
                    if p == .active { start() }
                    UIApplication.shared.isIdleTimerDisabled = p == .active && awake
                }
                .onChange(of: server) { _, _ in start() }
                .onChange(of: demo) { _, _ in start() }
                .onChange(of: awake) { _, v in UIApplication.shared.isIdleTimerDisabled = v }
                .onReceive(live.$snapshot) { s in
                    if let s { announcer.update(s) }
                }
        }
    }

    private func start() {
        if demo { live.startDemo() } else { live.connect(to: server) }
        UIApplication.shared.isIdleTimerDisabled = awake
    }
}
