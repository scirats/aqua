import Foundation

/// Pure byte-level helpers for H.264/HEVC access units.
///
/// NVENC typically emits Annex-B (start-code delimited) NAL units, while
/// `AVSampleBufferDisplayLayer` expects length-prefixed (AVCC/hvcC) samples once
/// a `CMVideoFormatDescription` was built from parameter sets. These conversions
/// are pure and unit-tested, so the fragile parsing is not buried inside the
/// AVFoundation plumbing.
enum VideoBitstream {
    /// Split an Annex-B byte stream into NAL units (start codes removed).
    static func annexBUnits(_ data: Data) -> [Data] {
        let bytes = [UInt8](data)
        var units: [Data] = []
        var index = 0
        var start: Int?

        while index < bytes.count {
            if index + 3 < bytes.count,
               bytes[index] == 0, bytes[index + 1] == 0,
               bytes[index + 2] == 0, bytes[index + 3] == 1 {
                if let start { units.append(Data(bytes[start..<index])) }
                start = index + 4
                index += 4
                continue
            }
            if index + 2 < bytes.count,
               bytes[index] == 0, bytes[index + 1] == 0, bytes[index + 2] == 1 {
                if let start { units.append(Data(bytes[start..<index])) }
                start = index + 3
                index += 3
                continue
            }
            index += 1
        }
        if let start, start < bytes.count {
            units.append(Data(bytes[start..<bytes.count]))
        }
        return units.filter { !$0.isEmpty }
    }

    /// True when `data` starts with an Annex-B start code.
    static func isAnnexB(_ data: Data) -> Bool {
        let bytes = [UInt8](data.prefix(4))
        if bytes.count >= 4, bytes[0] == 0, bytes[1] == 0, bytes[2] == 0, bytes[3] == 1 { return true }
        if bytes.count >= 3, bytes[0] == 0, bytes[1] == 0, bytes[2] == 1 { return true }
        return false
    }

    /// Encode NAL units as 4-byte big-endian length-prefixed samples (AVCC/hvcC).
    static func lengthPrefixed(_ units: [Data]) -> Data {
        var output = Data()
        for unit in units {
            let length = UInt32(unit.count).bigEndian
            withUnsafeBytes(of: length) { output.append(contentsOf: $0) }
            output.append(unit)
        }
        return output
    }

    /// Convert an Annex-B access unit to length-prefixed form (idempotent input
    /// must be Annex-B; use `isAnnexB` first).
    static func avccFromAnnexB(_ data: Data) -> Data {
        lengthPrefixed(annexBUnits(data))
    }

    /// H.264 SPS (type 7) then PPS (type 8) from Annex-B NAL units.
    static func h264ParameterSets(from units: [Data]) -> [Data] {
        var sps: Data?
        var pps: Data?
        for unit in units {
            guard let first = unit.first else { continue }
            switch first & 0x1f {
            case 7: sps = unit
            case 8: pps = unit
            default: break
            }
        }
        return [sps, pps].compactMap { $0 }
    }

    /// HEVC VPS (32), SPS (33), PPS (34) from Annex-B NAL units.
    static func hevcParameterSets(from units: [Data]) -> [Data] {
        var vps: Data?
        var sps: Data?
        var pps: Data?
        for unit in units {
            guard unit.count >= 2 else { continue }
            switch (unit[0] >> 1) & 0x3f {
            case 32: vps = unit
            case 33: sps = unit
            case 34: pps = unit
            default: break
            }
        }
        return [vps, sps, pps].compactMap { $0 }
    }
}
