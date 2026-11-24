import Foundation

/// Snapshots of a real race (IMSA Spa, 23 Nov 2026, AMG GT3, RM → RS → RS) taken from its
/// recording: the app's demo mode and the tests use them.
public enum Samples {
    /// In race order: start, before the first stop, standing in the box, after it, …, the finish
    public static let names = ["lap02", "lap08", "stop10", "lap11", "lap16", "stop17", "lap18", "lap21", "lap22"]

    public static func snapshot(_ name: String) -> Snapshot? {
        guard let url = Bundle.module.url(forResource: name, withExtension: "json", subdirectory: "Samples"),
              let data = try? Data(contentsOf: url) else { return nil }
        return try? JSONDecoder.box.decode(Snapshot.self, from: data)
    }
}
