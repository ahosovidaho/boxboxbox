import Foundation

// The JSON of the server (`docs/API.md`), snake_case keys decoded to camelCase. Almost every
// field is optional: an older or newer server with a field more or less must not break the app.

/// One update of the live data (`/ws`).
public struct Snapshot: Decodable, Sendable, Equatable {
    public var live: Bool?
    public var onTrack: Bool?
    public var paused: Bool?
    public var speedKmh: Double?
    public var rpm: Double?
    public var rpmAlertMin: Int?
    public var rpmAlertMax: Int?
    public var gear: Int?
    public var suggestedGear: Int?
    public var throttle: Double?
    public var brake: Double?
    public var tireTemp: [Double]?
    public var fuel: Double?
    public var fuelCapacity: Double?
    public var fuelPerLap: Double?
    public var fuelLapsLeft: Double?
    public var lap: Int?
    public var totalLaps: Int?
    public var currentLapMs: Int?
    public var lastLapMs: Int?
    public var bestLapMs: Int?
    public var liveDeltaMs: Int?
    public var laps: [Int]?
    public var lapsFull: [Bool]?
    public var position: Int?
    public var cars: Int?
    public var carName: String?
    public var strategy: Plan?

    public init() {}
}

/// The race plan as it stands (the `strategy` of a snapshot).
public struct Plan: Decodable, Sendable, Equatable {
    public var race: String?
    public var totalLaps: Int?
    public var lapsLeft: Double?
    public var fuelPerLap: Double?
    public var fuelNeeded: Double?
    public var fuelMarginLaps: Double?
    public var stopsNeeded: Int?
    /// Earliest and latest lap to box for fuel
    public var window: [Int]?
    public var targetLap: Int?
    public var lapsToBox: Int?
    public var nextStopFuel: Double?
    /// Standing in the box: fuel to add now, for how many laps
    public var stopAdd: Double?
    public var stopAddLaps: Int?
    public var compound: String?
    public var stintLaps: Double?
    /// New tyres went on, the compound is only the plan's guess: ask the driver
    public var compoundAsk: Bool?
    public var tyreQuestion: Bool?
    public var mandatory: String?
    public var mandatoryDone: Bool?
    public var pitRequired: Bool?
    public var pitDone: Bool?
    public var wearNow: Double?
    public var wearLapsLeft: Double?
    public var wearLimit: Double?
    public var calls: [Call]?
    public var options: Options?
    /// "custom" when the calls follow the driver's own plan
    public var following: String?
    public var customNote: String?
    public var practice: PracticeState?

    public init() {}
}

public struct Call: Decodable, Sendable, Equatable, Hashable {
    /// "box", "warn" or "info"
    public var level: String
    public var text: String

    public init(level: String, text: String) {
        self.level = level
        self.text = text
    }
}

public struct Options: Decodable, Sendable, Equatable {
    public var best: StrategyOption?
    public var alternative: StrategyOption?
    public var second: StrategyOption?
}

public struct StrategyOption: Decodable, Sendable, Equatable {
    public var stints: [PlanStint]
    public var stops: [PlannedStop]
    public var cost: Double?
    /// Slower than the recommendation by, s
    public var delta: Double?
    public var overLimit: Bool?
    public var risks: [String]?
}

public struct PlanStint: Decodable, Sendable, Equatable {
    public var compound: String
    public var laps: Int
}

public struct PlannedStop: Decodable, Sendable, Equatable {
    /// Box at the end of this lap
    public var lap: Int
    public var compound: String
    /// Fuel to add, litres
    public var fuel: Double
}

public struct PracticeState: Decodable, Sendable, Equatable {
    public var active: Bool?
    public var track: String?
    public var compound: String?
}

/// `GET /api/version`
public struct ServerVersion: Decodable, Sendable, Equatable {
    public var server: String
    public var api: Int
}

public extension JSONDecoder {
    /// The decoder for everything the server sends
    static var box: JSONDecoder {
        let d = JSONDecoder()
        d.keyDecodingStrategy = .convertFromSnakeCase
        return d
    }
}
