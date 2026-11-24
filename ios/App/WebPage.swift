import BoxCore
import SwiftUI
import WebKit

/// The server's web dashboard inside the app (everything that is not native yet).
struct DashboardPage: View {
    @AppStorage("server") private var server = "192.168.1.156:8099"
    @AppStorage("demo") private var demo = false
    @Environment(\.scenePhase) private var phase
    @State private var wake = 0

    var body: some View {
        ZStack {
            Theme.bg.ignoresSafeArea()
            if demo {
                hint("Dashboard potřebuje server", "S ukázkovými daty je k dispozici jen obrazovka Závod. Vypni je v Nastavení.")
            } else if let url = BoxAPI.base(from: server) {
                WebPage(url: url, wake: wake)
                    .padding(.bottom, 26) // room for the page dots
            } else {
                hint("Chybí adresa serveru", "Zadej ji v Nastavení na obrazovce Závod.")
            }
        }
        .onChange(of: phase) { _, p in if p == .active { wake += 1 } }
    }

    private func hint(_ title: String, _ text: String) -> some View {
        VStack(spacing: 8) {
            Text(title).font(.headline).foregroundStyle(Theme.text)
            Text(text).font(.footnote).foregroundStyle(Theme.dim).multilineTextAlignment(.center)
        }
        .padding(32)
    }
}

/// WKWebView with a dark background (no white flash) that reloads when the address changes
/// and, after a failed load, when the app is back in front (`wake` changes).
struct WebPage: UIViewRepresentable {
    let url: URL
    let wake: Int

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeUIView(context: Context) -> WKWebView {
        let config = WKWebViewConfiguration()
        config.allowsInlineMediaPlayback = true
        config.mediaTypesRequiringUserActionForPlayback = [] // braking tones without a tap
        let web = WKWebView(frame: .zero, configuration: config)
        web.isOpaque = false
        web.backgroundColor = .black
        web.scrollView.backgroundColor = .black
        web.scrollView.contentInsetAdjustmentBehavior = .never
        web.navigationDelegate = context.coordinator
        web.uiDelegate = context.coordinator // confirm() of the mode switch, alert()
        #if DEBUG
        web.isInspectable = true // Safari → Vývojář → simulátor
        #endif
        context.coordinator.wake = wake
        context.coordinator.load(url, in: web)
        return web
    }

    func updateUIView(_ web: WKWebView, context: Context) {
        let c = context.coordinator
        let retry = c.failed && c.wake != wake
        c.wake = wake
        if c.loaded != url || retry { c.load(url, in: web) }
    }

    final class Coordinator: NSObject, WKNavigationDelegate, WKUIDelegate {
        var loaded: URL?
        var failed = false
        var wake = 0

        func load(_ url: URL, in web: WKWebView) {
            loaded = url
            failed = false
            web.load(URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 10))
        }

        // Without these WKWebView answers every alert/confirm/prompt silently with "no".

        func webView(_ webView: WKWebView, runJavaScriptAlertPanelWithMessage message: String,
                     initiatedByFrame _: WKFrameInfo, completionHandler: @escaping () -> Void) {
            let a = UIAlertController(title: nil, message: message, preferredStyle: .alert)
            a.addAction(UIAlertAction(title: "OK", style: .default) { _ in completionHandler() })
            present(a, from: webView) { completionHandler() }
        }

        func webView(_ webView: WKWebView, runJavaScriptConfirmPanelWithMessage message: String,
                     initiatedByFrame _: WKFrameInfo, completionHandler: @escaping (Bool) -> Void) {
            let a = UIAlertController(title: nil, message: message, preferredStyle: .alert)
            a.addAction(UIAlertAction(title: "Zrušit", style: .cancel) { _ in completionHandler(false) })
            a.addAction(UIAlertAction(title: "OK", style: .default) { _ in completionHandler(true) })
            present(a, from: webView) { completionHandler(false) }
        }

        func webView(_ webView: WKWebView, runJavaScriptTextInputPanelWithPrompt prompt: String, defaultText: String?,
                     initiatedByFrame _: WKFrameInfo, completionHandler: @escaping (String?) -> Void) {
            let a = UIAlertController(title: nil, message: prompt, preferredStyle: .alert)
            a.addTextField { $0.text = defaultText }
            a.addAction(UIAlertAction(title: "Zrušit", style: .cancel) { _ in completionHandler(nil) })
            a.addAction(UIAlertAction(title: "OK", style: .default) { _ in completionHandler(a.textFields?.first?.text) })
            present(a, from: webView) { completionHandler(nil) }
        }

        /// Shows the dialog over the topmost screen; `fallback` answers when there is none.
        private func present(_ a: UIAlertController, from web: WKWebView, fallback: () -> Void) {
            var top = web.window?.rootViewController
            while let next = top?.presentedViewController { top = next }
            guard let top else { return fallback() }
            top.present(a, animated: true)
        }

        func webView(_ webView: WKWebView, didFailProvisionalNavigation _: WKNavigation!, withError error: Error) {
            failed = true
            let msg = error.localizedDescription.replacingOccurrences(of: "<", with: "&lt;")
            webView.loadHTMLString("""
            <meta name=viewport content="width=device-width"><body style="background:#000;color:#8A8A94;font:15px -apple-system;\
            display:flex;align-items:center;justify-content:center;height:90vh;text-align:center;padding:24px">\
            <div><b style="color:#F4F4F6">Server nedostupný</b><br>\(msg)<br><br>Přejeď zpět na Závod a zkontroluj adresu v Nastavení.</div>
            """, baseURL: nil)
        }
    }
}
