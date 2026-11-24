import Foundation

/// Numbers as the dashboard writes them (Czech: decimal comma).
public enum Fmt {
    /// 130159 → "2:10.159"
    public static func lap(_ ms: Int?) -> String {
        guard let ms, ms > 0 else { return "–" }
        let m = ms / 60000, s = (ms % 60000) / 1000, t = ms % 1000
        return String(format: "%d:%02d.%03d", m, s, t)
    }

    /// 11.087 → "11,1" (digits after the comma)
    public static func num(_ v: Double?, _ digits: Int = 1) -> String {
        guard let v, v.isFinite else { return "–" }
        return String(format: "%.\(digits)f", v).replacingOccurrences(of: ".", with: ",")
    }

    /// −320 ms → "−0,32"
    public static func delta(_ ms: Int?) -> String {
        guard let ms else { return "–" }
        let v = Double(ms) / 1000
        let sign = v > 0.0005 ? "+" : v < -0.0005 ? "−" : "±"
        return sign + num(abs(v), 2)
    }
}
