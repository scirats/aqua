import Foundation

/// Minimal Protocol Buffers wire-format reader/writer.
///
/// The Aqua schema is `protocol/aqua.proto`. We hand-roll the codec instead of
/// pulling a code-generation toolchain into both builds; the wire format is
/// standard protobuf, so it stays interoperable and debuggable with `protoc`.
enum WireType {
    static let varint = 0
    static let fixed64 = 1
    static let lengthDelimited = 2
    static let fixed32 = 5
}

struct ProtobufWriter {
    private(set) var data = Data()

    mutating func writeVarint(_ value: UInt64) {
        var value = value
        while value >= 0x80 {
            data.append(UInt8((value & 0x7f) | 0x80))
            value >>= 7
        }
        data.append(UInt8(value))
    }

    mutating func writeTag(_ field: Int, _ wire: Int) {
        writeVarint(UInt64((field << 3) | wire))
    }

    mutating func writeUInt32(_ field: Int, _ value: UInt32) {
        guard value != 0 else { return } // proto3 omits defaults
        writeTag(field, WireType.varint)
        writeVarint(UInt64(value))
    }

    mutating func writeUInt64(_ field: Int, _ value: UInt64) {
        guard value != 0 else { return }
        writeTag(field, WireType.varint)
        writeVarint(value)
    }

    mutating func writeBool(_ field: Int, _ value: Bool) {
        guard value else { return }
        writeTag(field, WireType.varint)
        writeVarint(1)
    }

    /// int32 uses sign-extended varint encoding.
    mutating func writeInt32(_ field: Int, _ value: Int32) {
        guard value != 0 else { return }
        writeTag(field, WireType.varint)
        writeVarint(UInt64(bitPattern: Int64(value)))
    }

    mutating func writeString(_ field: Int, _ value: String) {
        guard !value.isEmpty else { return }
        let bytes = Data(value.utf8)
        writeTag(field, WireType.lengthDelimited)
        writeVarint(UInt64(bytes.count))
        data.append(bytes)
    }

    mutating func writeBytes(_ field: Int, _ value: Data) {
        writeTag(field, WireType.lengthDelimited)
        writeVarint(UInt64(value.count))
        data.append(value)
    }

    mutating func writeDouble(_ field: Int, _ value: Double) {
        guard value != 0 else { return }
        writeTag(field, WireType.fixed64)
        var bits = value.bitPattern.littleEndian
        withUnsafeBytes(of: &bits) { data.append(contentsOf: $0) }
    }

    mutating func writeMessage(_ field: Int, _ body: Data) {
        writeBytes(field, body)
    }
}

struct ProtobufReader {
    private let data: Data
    private var index: Int

    init(_ data: Data) {
        self.data = data
        self.index = data.startIndex
    }

    var isAtEnd: Bool { index >= data.endIndex }

    mutating func readVarint() throws -> UInt64 {
        var result: UInt64 = 0
        var shift: UInt64 = 0
        while true {
            guard index < data.endIndex else { throw ProtobufError.truncated }
            let byte = data[index]
            index += 1
            result |= UInt64(byte & 0x7f) << shift
            if byte & 0x80 == 0 { break }
            shift += 7
            if shift > 63 { throw ProtobufError.malformed }
        }
        return result
    }

    mutating func readTag() throws -> (field: Int, wire: Int)? {
        if isAtEnd { return nil }
        let tag = try readVarint()
        return (Int(tag >> 3), Int(tag & 0x7))
    }

    mutating func readLengthDelimited() throws -> Data {
        let length = Int(try readVarint())
        guard length >= 0, index + length <= data.endIndex else {
            throw ProtobufError.truncated
        }
        let start = index
        index += length
        return data.subdata(in: start..<index)
    }

    mutating func readDouble() throws -> Double {
        guard index + 8 <= data.endIndex else { throw ProtobufError.truncated }
        var bits: UInt64 = 0
        for offset in 0..<8 {
            bits |= UInt64(data[index + offset]) << (8 * UInt64(offset))
        }
        index += 8
        return Double(bitPattern: bits)
    }

    mutating func skip(_ wire: Int) throws {
        switch wire {
        case WireType.varint:
            _ = try readVarint()
        case WireType.fixed64:
            guard index + 8 <= data.endIndex else { throw ProtobufError.truncated }
            index += 8
        case WireType.lengthDelimited:
            _ = try readLengthDelimited()
        case WireType.fixed32:
            guard index + 4 <= data.endIndex else { throw ProtobufError.truncated }
            index += 4
        default:
            throw ProtobufError.malformed
        }
    }
}

enum ProtobufError: Error, Equatable {
    case truncated
    case malformed
}
