import XCTest
@testable import AquaClient

/// Pure NAL-unit parsing/conversion tests. These are the fragile parts of the
/// decoder, so they are tested without AVFoundation.
final class VideoBitstreamTests: XCTestCase {
    // SPS (type 7), PPS (type 8), IDR slice (type 5), Annex-B framed.
    private let h264AnnexB = Data([
        0x00, 0x00, 0x00, 0x01, 0x67, 0x42, 0x00, 0x1e,
        0x00, 0x00, 0x01, 0x68, 0xce, 0x3c, 0x80,
        0x00, 0x00, 0x01, 0x65, 0x88, 0x84,
    ])

    // VPS (32), SPS (33), PPS (34), Annex-B framed.
    private let hevcAnnexB = Data([
        0x00, 0x00, 0x00, 0x01, 0x40, 0x01, 0x0c,
        0x00, 0x00, 0x01, 0x42, 0x01, 0x01,
        0x00, 0x00, 0x01, 0x44, 0x01, 0xc0,
    ])

    func testIsAnnexB() {
        XCTAssertTrue(VideoBitstream.isAnnexB(h264AnnexB))
        XCTAssertTrue(VideoBitstream.isAnnexB(Data([0x00, 0x00, 0x01, 0x65])))
        XCTAssertFalse(VideoBitstream.isAnnexB(Data([0x00, 0x00, 0x00, 0x02, 0x65])))
        XCTAssertFalse(VideoBitstream.isAnnexB(Data()))
    }

    func testAnnexBUnitsStripsStartCodes() {
        let units = VideoBitstream.annexBUnits(h264AnnexB)
        XCTAssertEqual(units.count, 3)
        XCTAssertEqual(units[0], Data([0x67, 0x42, 0x00, 0x1e]))
        XCTAssertEqual(units[1], Data([0x68, 0xce, 0x3c, 0x80]))
        XCTAssertEqual(units[2], Data([0x65, 0x88, 0x84]))
    }

    func testLengthPrefixedEncoding() {
        let units = VideoBitstream.annexBUnits(hevcAnnexB)
        let avcc = VideoBitstream.lengthPrefixed(units)
        // First NAL is 3 bytes: 00 00 00 03 40 01 0c
        XCTAssertEqual(avcc.prefix(7), Data([0x00, 0x00, 0x00, 0x03, 0x40, 0x01, 0x0c]))
        XCTAssertEqual(VideoBitstream.avccFromAnnexB(hevcAnnexB), avcc)
    }

    func testH264ParameterSetExtraction() {
        let units = VideoBitstream.annexBUnits(h264AnnexB)
        let sets = VideoBitstream.h264ParameterSets(from: units)
        XCTAssertEqual(sets.count, 2)
        XCTAssertEqual(sets[0].first, 0x67) // SPS
        XCTAssertEqual(sets[1].first, 0x68) // PPS
    }

    func testHEVCParameterSetExtraction() {
        let units = VideoBitstream.annexBUnits(hevcAnnexB)
        let sets = VideoBitstream.hevcParameterSets(from: units)
        XCTAssertEqual(sets.count, 3)
        XCTAssertEqual(sets[0].first, 0x40) // VPS
        XCTAssertEqual(sets[1].first, 0x42) // SPS
        XCTAssertEqual(sets[2].first, 0x44) // PPS
    }
}
