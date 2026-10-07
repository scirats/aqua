import CoreGraphics
import Foundation

/// Conversion from Wayland `wl_shm` pixel formats to a CoreGraphics image.
///
/// Wayland `WL_SHM_FORMAT_ARGB8888` / `XRGB8888` are packed 32-bit values
/// (`0xAARRGGBB` / `0xXXRRGGBB`) stored in **little-endian** memory, so the byte
/// order in the buffer is `B, G, R, A` (BGRA / BGRX) on all supported machines.
/// They are **not** `R, G, B, A`.
///
/// This is validated by `PixelFormatTests` with known colour patterns.
enum ShmPixelFormat: Equatable {
    case argb8888
    case xrgb8888

    init?(protocolValue: UInt32) {
        switch protocolValue {
        case AquaProtocol.SurfaceFormat.argb8888: self = .argb8888
        case AquaProtocol.SurfaceFormat.xrgb8888: self = .xrgb8888
        default: return nil
        }
    }

    /// CoreGraphics bitmap layout for the little-endian BGRA/BGRX buffer.
    var bitmapInfo: CGBitmapInfo {
        switch self {
        case .argb8888:
            // BGRA with alpha; Wayland convention is premultiplied.
            return CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedFirst.rawValue
                | CGBitmapInfo.byteOrder32Little.rawValue)
        case .xrgb8888:
            // BGRX: the high byte is ignored.
            return CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipFirst.rawValue
                | CGBitmapInfo.byteOrder32Little.rawValue)
        }
    }
}

enum SurfacePixelFormat {
    /// Build a `CGImage` from an owned SHM frame.
    ///
    /// Returns `nil` for unsupported formats, zero dimensions, or a buffer that
    /// is too short for `stride * height`. All arithmetic is checked.
    static func makeImage(
        width: UInt32,
        height: UInt32,
        stride: UInt32,
        format: UInt32,
        data: Data
    ) -> CGImage? {
        guard let pixelFormat = ShmPixelFormat(protocolValue: format) else { return nil }
        guard width > 0, height > 0, width <= 8192, height <= 8192 else { return nil }

        let expected = stride.multipliedReportingOverflow(by: height)
        guard !expected.overflow else { return nil }
        guard Int(expected.partialValue) <= data.count else { return nil }

        guard
            let provider = CGDataProvider(data: data as CFData),
            let image = CGImage(
                width: Int(width),
                height: Int(height),
                bitsPerComponent: 8,
                bitsPerPixel: 32,
                bytesPerRow: Int(stride),
                space: CGColorSpaceCreateDeviceRGB(),
                bitmapInfo: pixelFormat.bitmapInfo,
                provider: provider,
                decode: nil,
                shouldInterpolate: false,
                intent: .defaultIntent
            )
        else {
            return nil
        }
        return image
    }

    /// Reads the top-left pixel as `(r, g, b, a)` by drawing the image into a
    /// canonical RGBA context. Used by tests to detect swapped channels.
    static func sampleTopLeft(_ image: CGImage) -> (r: UInt8, g: UInt8, b: UInt8, a: UInt8)? {
        var bytes = [UInt8](repeating: 0, count: 4)
        let colorSpace = CGColorSpaceCreateDeviceRGB()
        let info = CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue
        guard let context = CGContext(
            data: &bytes,
            width: 1,
            height: 1,
            bitsPerComponent: 8,
            bytesPerRow: 4,
            space: colorSpace,
            bitmapInfo: info
        ) else {
            return nil
        }
        context.draw(image, in: CGRect(x: 0, y: 0, width: 1, height: 1))
        return (bytes[0], bytes[1], bytes[2], bytes[3])
    }
}
