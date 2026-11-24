import BoxCore
import SwiftUI

/// The race on one screen: the engineer's call on top, tyres to confirm after a stop, then
/// gear and speed, fuel, next stop, tyres and lap times. Portrait or landscape.
struct RaceView: View {
    @EnvironmentObject private var live: LiveConnection
    @State private var settings = false

    private var s: Snapshot { live.snapshot ?? Snapshot() }
    private var plan: Plan? { s.strategy }

    var body: some View {
        ZStack {
            Theme.bg.ignoresSafeArea()
            ScrollView {
                VStack(spacing: 10) {
                    header
                    if live.outdated { Banner(text: "Server je novější než aplikace – aktualizuj ji.", color: Theme.warn) }
                    callBanner
                    if plan?.compoundAsk == true { compoundAsk }
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 165), spacing: 10)], spacing: 10) {
                        driveTile
                        fuelTile
                        boxTile
                        tyreTile
                        timesTile
                    }
                    planLine
                    otherCalls
                }
                .padding(12)
                .padding(.bottom, 24) // the page dots
            }
        }
        .sheet(isPresented: $settings) { SettingsView() }
    }

    // MARK: Header

    private var header: some View {
        HStack(spacing: 10) {
            Text("BOX").font(Theme.brand) + Text("BOX").font(Theme.brand).foregroundColor(Theme.red) + Text("BOX").font(Theme.brand)
            Spacer()
            if let lap = lapText {
                Text(lap).font(Theme.label).foregroundStyle(Theme.text)
            }
            StatusDot(status: live.status)
            Button { settings = true } label: { Image(systemName: "gearshape.fill").foregroundStyle(Theme.dim) }
                .accessibilityLabel("Nastavení")
        }
    }

    private var lapText: String? {
        guard let lap = s.lap, lap > 0 else { return nil }
        if let total = s.totalLaps, total > 0 { return "KOLO \(lap) / \(total)" }
        return "KOLO \(lap)"
    }

    // MARK: The call

    /// The most urgent call: box > warn > "Box v kole …"
    private var mainCall: Call? {
        let calls = plan?.calls ?? []
        return calls.first { $0.level == "box" } ?? calls.first { $0.level == "warn" && !$0.text.hasPrefix("Nasazeno") }
            ?? calls.first { $0.text.hasPrefix("Box v kole") || $0.text.hasPrefix("Cíl") } ?? calls.first
    }

    @ViewBuilder private var callBanner: some View {
        if let c = mainCall {
            Banner(text: c.text, color: Theme.color(c.level), big: c.level == "box")
        } else if live.snapshot == nil {
            Banner(text: statusText, color: Theme.panel)
        }
    }

    private var statusText: String {
        switch live.status {
        case .idle: return "Odpojeno"
        case .connecting: return "Připojuji se k serveru…"
        case .connected: return "Čekám na data z GT7…"
        case .demo: return "Ukázková data"
        case .failed(let e): return "Server nedostupný – \(e)"
        }
    }

    // MARK: Tyres after a stop

    private var compoundAsk: some View {
        let current = plan?.compound ?? ""
        let kind = current.first.map(String.init) ?? "R"
        return VStack(alignment: .leading, spacing: 8) {
            Text("NASAZENO?").font(Theme.label).foregroundStyle(Theme.warn)
            HStack(spacing: 8) {
                ForEach(["S", "M", "H"].map { kind + $0 }, id: \.self) { c in
                    Button {
                        Task { try? await live.api?.setCompound(c) }
                    } label: {
                        Text(c).font(.system(size: 24, weight: .heavy, design: .rounded))
                            .frame(maxWidth: .infinity, minHeight: 56)
                            .background(c == current ? Theme.warn : Theme.panel2)
                            .foregroundStyle(c == current ? Color.black : Theme.text)
                            .clipShape(RoundedRectangle(cornerRadius: 12))
                    }
                    .disabled(live.status == .demo)
                }
            }
            Text("Odhad podle plánu – ťukni na směs, kterou máš na autě.").font(.footnote).foregroundStyle(Theme.dim)
        }
        .padding(12)
        .background(Theme.warn.opacity(0.10))
        .overlay(RoundedRectangle(cornerRadius: 14).stroke(Theme.warn, lineWidth: 1.5))
        .clipShape(RoundedRectangle(cornerRadius: 14))
    }

    // MARK: Tiles

    private var driveTile: some View {
        Tile(title: "Jízda") {
            HStack(alignment: .firstTextBaseline) {
                Text(gearText).font(.system(size: 64, weight: .black, design: .rounded)).monospacedDigit()
                Spacer()
                VStack(alignment: .trailing) {
                    Text(Fmt.num(s.speedKmh, 0)).font(Theme.big).monospacedDigit()
                    Text("km/h").font(Theme.label).foregroundStyle(Theme.dim)
                }
            }
            RevBar(rpm: s.rpm ?? 0, from: Double(s.rpmAlertMin ?? 0), to: Double(s.rpmAlertMax ?? 0))
        }
    }

    private var gearText: String {
        guard let g = s.gear else { return "–" }
        return g == 0 ? "N" : "\(g)"
    }

    private var fuelTile: some View {
        Tile(title: "Palivo") {
            Row("V nádrži", Fmt.num(s.fuel) + " l", big: true)
            Row("Na kolo", Fmt.num(s.fuelPerLap ?? plan?.fuelPerLap, 2) + " l")
            Row("Do cíle", margin)
        }
    }

    private var margin: String {
        guard let m = plan?.fuelMarginLaps else { return "–" }
        return (m >= 0 ? "+" : "−") + Fmt.num(abs(m)) + " kola"
    }

    private var boxTile: some View {
        Tile(title: "Zastávka") {
            if let add = plan?.stopAdd {
                Row("Dotankuj", "+" + Fmt.num(add) + " l", big: true, color: Theme.red)
                Row("Na", "\(plan?.stopAddLaps ?? 0) kol")
            } else if let t = plan?.targetLap {
                Row("Box v kole", "\(t)", big: true)
                Row("Dotankovat", plan?.nextStopFuel.map { "+" + Fmt.num($0) + " l" } ?? "–")
                Row("Nasadit", nextCompound ?? "–")
            } else {
                Row("Plán", (plan?.stopsNeeded ?? 0) == 0 ? "do cíle" : "–", big: true)
                if plan?.pitRequired == true { Row("Povinná", plan?.pitDone == true ? "✓" : "ještě ne") }
            }
        }
    }

    private var nextCompound: String? { plan?.options?.best?.stops.first?.compound }

    private var tyreTile: some View {
        Tile(title: "Gumy") {
            Row("Směs", plan?.compound ?? "–", big: true, color: Theme.compound(plan?.compound))
            Row("Kol na sadě", Fmt.num(plan?.stintLaps, 0))
            if let m = plan?.mandatory { Row("Povinně \(m)", plan?.mandatoryDone == true ? "✓" : "ještě ne") }
            if let t = s.tireTemp, t.count == 4 {
                Row("Teploty", t.map { Fmt.num($0, 0) }.joined(separator: " · "))
            }
        }
    }

    private var timesTile: some View {
        Tile(title: "Časy") {
            Row("Poslední", Fmt.lap(s.lastLapMs), big: true)
            Row("Nejlepší", Fmt.lap(s.bestLapMs), color: Theme.purple)
            Row("Delta", Fmt.delta(s.liveDeltaMs), color: (s.liveDeltaMs ?? 0) <= 0 ? Theme.green : Theme.red)
        }
    }

    // MARK: Plan and the rest of the calls

    @ViewBuilder private var planLine: some View {
        if let best = plan?.options?.best {
            VStack(alignment: .leading, spacing: 4) {
                Text(plan?.following == "custom" ? "VLASTNÍ PLÁN" : "PLÁN").font(Theme.label).foregroundStyle(Theme.dim)
                Text(best.stints.map { "\($0.compound) \($0.laps)" }.joined(separator: " → "))
                    .font(.system(.title3, design: .rounded).weight(.bold))
                Text(best.stops.map { "box \($0.lap)" + ($0.fuel > 0.05 ? " +\(Fmt.num($0.fuel, 0)) l" : "") }.joined(separator: " · "))
                    .font(.footnote).foregroundStyle(Theme.dim)
                if let note = plan?.customNote { Text(note).font(.footnote).foregroundStyle(Theme.warn) }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(12)
            .background(Theme.panel)
            .clipShape(RoundedRectangle(cornerRadius: 14))
        }
    }

    @ViewBuilder private var otherCalls: some View {
        let rest = (plan?.calls ?? []).filter { $0 != mainCall }
        if !rest.isEmpty {
            VStack(alignment: .leading, spacing: 6) {
                ForEach(rest, id: \.self) { c in
                    HStack(alignment: .top, spacing: 8) {
                        Circle().fill(Theme.color(c.level)).frame(width: 8, height: 8).padding(.top, 6)
                        Text(c.text).font(.callout)
                    }
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(12)
            .background(Theme.panel)
            .clipShape(RoundedRectangle(cornerRadius: 14))
        }
    }
}

// MARK: - Pieces

struct Tile<Content: View>: View {
    let title: String
    @ViewBuilder var content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title.uppercased()).font(Theme.label).foregroundStyle(Theme.dim)
            content
        }
        .frame(maxWidth: .infinity, minHeight: 130, alignment: .topLeading)
        .padding(12)
        .background(Theme.panel)
        .clipShape(RoundedRectangle(cornerRadius: 14))
    }
}

struct Row: View {
    let label: String
    let value: String
    var big = false
    var color: Color = Theme.text

    init(_ label: String, _ value: String, big: Bool = false, color: Color = Theme.text) {
        self.label = label
        self.value = value
        self.big = big
        self.color = color
    }

    var body: some View {
        HStack(alignment: .firstTextBaseline) {
            Text(label).font(.footnote).foregroundStyle(Theme.dim)
            Spacer(minLength: 6)
            Text(value).font(big ? Theme.big : .system(.body, design: .rounded).weight(.semibold))
                .monospacedDigit().foregroundStyle(color).lineLimit(1).minimumScaleFactor(0.6)
        }
    }
}

struct Banner: View {
    let text: String
    let color: Color
    var big = false

    var body: some View {
        Text(text)
            .font(.system(size: big ? 26 : 19, weight: .heavy, design: .rounded))
            .italic(big)
            .multilineTextAlignment(.leading)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 14).padding(.vertical, big ? 16 : 12)
            .background(color)
            .foregroundStyle(color == Theme.warn ? Color.black : Color.white)
            .clipShape(RoundedRectangle(cornerRadius: 14))
            .animation(.easeOut(duration: 0.2), value: text)
    }
}

/// Engine speed against the shift light window of GT7 (green → red)
struct RevBar: View {
    let rpm: Double
    let from: Double
    let to: Double

    var body: some View {
        GeometryReader { g in
            let top = max(to, rpm, 1) * 1.02
            let x = min(1, rpm / top)
            ZStack(alignment: .leading) {
                Capsule().fill(Theme.panel2)
                Capsule().fill(rpm >= to && to > 0 ? Theme.red : rpm >= from && from > 0 ? Theme.warn : Theme.green)
                    .frame(width: g.size.width * x)
            }
        }
        .frame(height: 8)
        .animation(.linear(duration: 0.07), value: rpm)
    }
}

struct StatusDot: View {
    let status: LiveConnection.Status

    var body: some View {
        Circle().fill(color).frame(width: 10, height: 10).accessibilityLabel(label)
    }

    private var color: Color {
        switch status {
        case .connected: return Theme.green
        case .demo: return Theme.purple
        case .connecting: return Theme.warn
        case .idle, .failed: return Theme.red
        }
    }

    private var label: String {
        switch status {
        case .connected: return "Připojeno"
        case .demo: return "Ukázka"
        case .connecting: return "Připojuji"
        case .idle, .failed: return "Odpojeno"
        }
    }
}

enum Theme {
    static let bg = Color(red: 0.03, green: 0.03, blue: 0.04)
    static let panel = Color(red: 0.08, green: 0.08, blue: 0.10)
    static let panel2 = Color(red: 0.14, green: 0.14, blue: 0.17)
    static let text = Color(white: 0.95)
    static let dim = Color(white: 0.55)
    static let red = Color(red: 0.88, green: 0.02, blue: 0.0)
    static let warn = Color(red: 0.95, green: 0.76, blue: 0.19)
    static let green = Color(red: 0.18, green: 0.83, blue: 0.42)
    static let purple = Color(red: 0.70, green: 0.36, blue: 1.0)
    static let brand = Font.system(size: 22, weight: .black, design: .default).italic()
    static let label = Font.system(size: 12, weight: .bold, design: .default).width(.condensed)
    static let big = Font.system(size: 30, weight: .heavy, design: .rounded)

    static func color(_ level: String) -> Color {
        switch level {
        case "box": return red
        case "warn": return warn
        default: return panel2
        }
    }

    /// GT7 colours: soft red, medium yellow, hard white
    static func compound(_ c: String?) -> Color {
        switch c?.last {
        case "S": return Color(red: 1.0, green: 0.27, blue: 0.24)
        case "M": return warn
        case "H": return text
        default: return text
        }
    }
}

#Preview("Závod Spa (ukázka)") {
    DemoPreview()
}

private struct DemoPreview: View {
    @StateObject private var live = LiveConnection()

    var body: some View {
        RaceView().environmentObject(live).environmentObject(Announcer())
            .preferredColorScheme(.dark)
            .onAppear { live.startDemo() }
    }
}
