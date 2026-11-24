import Foundation

/// The live data of the server (`/ws`): connects, reconnects after a loss, decodes off the main
/// thread and publishes at most `maxRate` snapshots a second (the server sends up to 30).
@MainActor
public final class LiveConnection: ObservableObject {
    public enum Status: Equatable {
        case idle
        case connecting
        case connected
        case demo
        case failed(String)
    }

    @Published public private(set) var snapshot: Snapshot?
    @Published public private(set) var status: Status = .idle
    @Published public private(set) var server: ServerVersion?
    /// The server speaks a newer API than this app knows
    @Published public private(set) var outdated = false
    public private(set) var api: BoxAPI?

    public var maxRate: Double = 15
    private var task: URLSessionWebSocketTask?
    private let session = URLSession(configuration: .default)
    /// Bumped on every connect/disconnect: callbacks of an older connection are ignored
    private var generation = 0
    private var lastPublish = Date.distantPast
    private var demoTask: Task<Void, Never>?

    public init() {}

    public func connect(to address: String) {
        stop()
        guard let base = BoxAPI.base(from: address) else {
            status = .failed("Neplatná adresa serveru")
            return
        }
        api = BoxAPI(base: base)
        start(generation)
    }

    /// Cycles through the sample race (no server needed)
    public func startDemo() {
        stop()
        status = .demo
        let gen = generation
        demoTask = Task { [weak self] in
            var i = 0
            while !Task.isCancelled {
                guard let self, gen == self.generation else { return }
                self.snapshot = Samples.snapshot(Samples.names[i % Samples.names.count])
                i += 1
                try? await Task.sleep(nanoseconds: 3_000_000_000)
            }
        }
    }

    public func stop() {
        generation += 1
        task?.cancel(with: .goingAway, reason: nil)
        task = nil
        demoTask?.cancel()
        demoTask = nil
        status = .idle
    }

    private func start(_ gen: Int) {
        guard gen == generation, let url = api?.socket else { return }
        status = .connecting
        let t = session.webSocketTask(with: url)
        task = t
        t.resume()
        receive(t, gen)
        if let api {
            Task { [weak self] in
                let v = try? await api.serverVersion()
                guard let self, gen == self.generation else { return }
                self.server = v
                self.outdated = (v?.api ?? BoxAPI.version) > BoxAPI.version
            }
        }
    }

    private nonisolated func receive(_ t: URLSessionWebSocketTask, _ gen: Int) {
        t.receive { [weak self] result in
            switch result {
            case .success(let message):
                var data: Data?
                switch message {
                case .string(let s): data = s.data(using: .utf8)
                case .data(let d): data = d
                @unknown default: data = nil
                }
                let snap = data.flatMap { try? JSONDecoder.box.decode(Snapshot.self, from: $0) }
                Task { @MainActor [weak self] in
                    guard let self, gen == self.generation else { return }
                    self.status = .connected
                    let now = Date()
                    if let snap, now.timeIntervalSince(self.lastPublish) >= 1 / self.maxRate {
                        self.lastPublish = now
                        self.snapshot = snap
                    }
                }
                self?.receive(t, gen)
            case .failure(let error):
                Task { @MainActor [weak self] in
                    guard let self, gen == self.generation else { return }
                    self.status = .failed(error.localizedDescription)
                    try? await Task.sleep(nanoseconds: 2_000_000_000)
                    self.start(gen)
                }
            }
        }
    }
}
