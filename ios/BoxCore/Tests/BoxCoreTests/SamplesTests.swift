import XCTest
@testable import BoxCore

/// The snapshots of the Spa race decode, and say what the dashboard said then.
final class SamplesTests: XCTestCase {
    func testAllSamplesDecode() {
        for name in Samples.names {
            XCTAssertNotNil(Samples.snapshot(name), name)
        }
    }

    func testAfterTheFirstStopTheCompoundIsAsked() throws {
        let s = try XCTUnwrap(Samples.snapshot("lap11"))
        XCTAssertEqual(s.lap, 11)
        XCTAssertEqual(s.totalLaps, 21)
        XCTAssertEqual(s.carName, "AMG GT3 '20")
        let p = try XCTUnwrap(s.strategy)
        XCTAssertEqual(p.compoundAsk, true)
        XCTAssertEqual(p.compound, "RS")
        XCTAssertEqual(p.targetLap, 15)
        XCTAssertEqual(p.calls?.first?.text, "Nasazeno RS? Potvrď směs (ťukni na ni).")
        XCTAssertNotNil(p.options?.best)
    }

    func testStandingInTheBoxSaysHowMuchToAdd() throws {
        let p = try XCTUnwrap(Samples.snapshot("stop10")?.strategy)
        XCTAssertEqual(p.stopAddLaps, 7)
        XCTAssertEqual(p.stopAdd ?? 0, 80.1, accuracy: 0.1)
        XCTAssertEqual(p.calls?.first?.level, "box")
    }

    func testTheFinish() throws {
        let p = try XCTUnwrap(Samples.snapshot("lap22")?.strategy)
        XCTAssertEqual(p.calls, [Call(level: "info", text: "Cíl – závod je dojetý.")])
    }

    func testAddressAndFormats() {
        let api = BoxAPI(base: BoxAPI.base(from: " 192.168.1.156:8099/ ")!)
        XCTAssertEqual(api.base.absoluteString, "http://192.168.1.156:8099")
        XCTAssertEqual(api.socket?.absoluteString, "ws://192.168.1.156:8099/ws")
        XCTAssertEqual(Fmt.lap(130159), "2:10.159")
        XCTAssertEqual(Fmt.num(11.087, 2), "11,09")
        XCTAssertEqual(Fmt.delta(-320), "−0,32")
    }
}
