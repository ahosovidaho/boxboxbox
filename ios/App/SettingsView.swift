import BoxCore
import SwiftUI

struct SettingsView: View {
    @EnvironmentObject private var live: LiveConnection
    @EnvironmentObject private var announcer: Announcer
    @AppStorage("server") private var server = "192.168.1.156:8099"
    @AppStorage("demo") private var demo = false
    @AppStorage("awake") private var awake = true
    @AppStorage("voice") private var voice = true
    @Environment(\.dismiss) private var dismiss
    @State private var address = ""

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("192.168.1.156:8099", text: $address)
                        .keyboardType(.URL).textInputAutocapitalization(.never).autocorrectionDisabled()
                        .onSubmit { server = address }
                    Toggle("Ukázková data (závod Spa)", isOn: $demo)
                } header: {
                    Text("Server")
                } footer: {
                    Text("Adresa BOXBOXBOX na Mac mini (stejná jako dashboard v prohlížeči). Mimo domov přes Tailscale jeho adresa 100.x.x.x:8099. Ukázková data přehrávají závod ze Spa bez serveru.")
                }
                Section("Hlas a obrazovka") {
                    Toggle("Hlasový inženýr", isOn: $voice)
                    Button("Vyzkoušet hlas") { announcer.say("Box na konci tohoto kola, dotankovat plus 56 celá 3") }
                    Toggle("Nezhasínat displej", isOn: $awake)
                }
                Section("Verze") {
                    LabeledContent("Aplikace", value: Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "–")
                    LabeledContent("API aplikace", value: "\(BoxAPI.version)")
                    LabeledContent("Server", value: live.server.map { "\($0.server) · API \($0.api)" } ?? "–")
                }
            }
            .navigationTitle("Nastavení")
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Hotovo") {
                        if address != server { server = address }
                        dismiss()
                    }
                }
            }
            .onAppear { address = server }
        }
    }
}
