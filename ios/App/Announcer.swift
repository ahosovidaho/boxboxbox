import AVFoundation
import BoxCore
import SwiftUI
import UIKit

/// Says the race engineer's calls aloud (Czech voice) and taps the wrist (haptics): a new "box"
/// or "warn" call once, the box call again when it changes (another lap, another amount).
@MainActor
final class Announcer: ObservableObject {
    @AppStorage("voice") var voice = true
    private let synth = AVSpeechSynthesizer()
    /// What was said already in this race (a call is said once)
    private var said: Set<String> = []
    private var lastLap = 0

    init() {
        // Over the game sound and music, lowering them while speaking
        try? AVAudioSession.sharedInstance().setCategory(.playback, mode: .voicePrompt, options: [.duckOthers, .mixWithOthers])
    }

    func update(_ s: Snapshot) {
        // A new race (lap counter back): everything may be said again
        let lap = s.lap ?? 0
        if lap < lastLap { said.removeAll() }
        lastLap = lap
        guard let calls = s.strategy?.calls else { return }
        for c in calls where c.level == "box" || c.level == "warn" {
            guard !said.contains(c.text) else { continue }
            said.insert(c.text)
            if c.level == "box" {
                UINotificationFeedbackGenerator().notificationOccurred(.warning)
            }
            say(c.text)
        }
    }

    func say(_ text: String) {
        guard voice else { return }
        try? AVAudioSession.sharedInstance().setActive(true)
        let u = AVSpeechUtterance(string: Self.spoken(text))
        u.voice = AVSpeechSynthesisVoice(language: "cs-CZ")
        u.rate = AVSpeechUtteranceDefaultSpeechRate * 1.05
        synth.speak(u)
    }

    /// "+80,1" reads better as "plus 80 celá 1", "RS" as "er es"
    static func spoken(_ text: String) -> String {
        var t = text
        for (a, b) in [("RS", "er es"), ("RM", "er em"), ("RH", "er há"), ("+", "plus ")] {
            t = t.replacingOccurrences(of: a, with: b)
        }
        return t.replacingOccurrences(of: #"(\d),(\d)"#, with: "$1 celá $2", options: .regularExpression)
    }
}
