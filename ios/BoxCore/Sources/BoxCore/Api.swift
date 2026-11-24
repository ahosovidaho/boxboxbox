import Foundation

/// The HTTP API of the server (`docs/API.md`).
public struct BoxAPI: Sendable {
    /// The API version this app was written for (`API_VERSION` of the server)
    public static let version = 1

    public let base: URL

    public init(base: URL) {
        self.base = base
    }

    /// "192.168.1.156:8099" or "http://…" → the server's base URL
    public static func base(from address: String) -> URL? {
        var a = address.trimmingCharacters(in: .whitespacesAndNewlines)
        if a.isEmpty { return nil }
        if !a.contains("://") { a = "http://" + a }
        while a.hasSuffix("/") { a.removeLast() }
        return URL(string: a)
    }

    /// ws://…/ws of the same server
    public var socket: URL? {
        guard var c = URLComponents(url: base, resolvingAgainstBaseURL: false) else { return nil }
        c.scheme = c.scheme == "https" ? "wss" : "ws"
        c.path = "/ws"
        return c.url
    }

    public func serverVersion() async throws -> ServerVersion {
        let (data, _) = try await URLSession.shared.data(from: base.appendingPathComponent("api/version"))
        return try JSONDecoder.box.decode(ServerVersion.self, from: data)
    }

    /// The compound on the car now (confirms or corrects the plan's guess after a stop)
    public func setCompound(_ compound: String) async throws {
        try await action(["action": "compound", "compound": compound])
    }

    public func action(_ body: [String: String]) async throws {
        var r = URLRequest(url: base.appendingPathComponent("api/strategy/action"))
        r.httpMethod = "POST"
        r.setValue("application/json", forHTTPHeaderField: "Content-Type")
        r.httpBody = try JSONSerialization.data(withJSONObject: body)
        let (_, resp) = try await URLSession.shared.data(for: r)
        if let h = resp as? HTTPURLResponse, h.statusCode >= 400 {
            throw URLError(.badServerResponse)
        }
    }
}
