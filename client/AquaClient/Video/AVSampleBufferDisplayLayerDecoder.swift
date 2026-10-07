import AVFoundation
import CoreMedia
import UIKit

/// A `UIView` whose backing layer is an `AVSampleBufferDisplayLayer`.
final class AVSampleBufferDisplayLayerView: UIView {
    override class var layerClass: AnyClass { AVSampleBufferDisplayLayer.self }

    var displayLayer: AVSampleBufferDisplayLayer {
        // Safe: `layerClass` is `AVSampleBufferDisplayLayer`.
        layer as! AVSampleBufferDisplayLayer
    }
}

/// Bring-up hardware decoder: feeds `AVSampleBufferDisplayLayer` directly.
///
/// This is the first presentation strategy from `docs/VIDEO.md`. It is
/// intentionally conservative:
///
/// - it builds the `CMVideoFormatDescription` from the parameter sets in a
///   `CONFIG` message (H.264/HEVC);
/// - it converts Annex-B access units to length-prefixed samples;
/// - it flushes and waits for a keyframe if the layer reports `.failed`.
///
/// It is not yet physically validated (no Linux GPU / no stream here); the
/// `CVPixelBuffer`+Metal alternative remains the candidate if measurement shows
/// this is insufficient.
@MainActor
final class AVSampleBufferDisplayLayerDecoder: VideoDecoding {
    let view: UIView

    private let host: AVSampleBufferDisplayLayerView
    private var configuration: WindowVideoConfiguration?
    private var formatDescription: CMVideoFormatDescription?

    init() {
        let host = AVSampleBufferDisplayLayerView()
        host.backgroundColor = .black
        host.displayLayer.videoGravity = .resizeAspect
        self.host = host
        self.view = host
    }

    func configure(_ configuration: WindowVideoConfiguration) {
        self.configuration = configuration
        if let codecConfiguration = configuration.codecConfiguration, !codecConfiguration.isEmpty {
            let units = VideoBitstream.annexBUnits(codecConfiguration)
            formatDescription = Self.makeFormatDescription(
                configuration: configuration,
                units: units
            )
        }
        host.displayLayer.flush()
    }

    func decode(_ frame: EncodedVideoFrame) {
        guard let configuration,
              configuration.codec == frame.codec,
              let formatDescription else {
            // No parameter sets yet: the stream model keeps requesting a
            // keyframe / CONFIG until this becomes decodable.
            return
        }
        if host.displayLayer.status == .failed {
            host.displayLayer.flush()
        }
        guard let sample = Self.makeSampleBuffer(frame: frame, formatDescription: formatDescription) else {
            return
        }
        host.displayLayer.enqueue(sample)
    }

    func reset() {
        host.displayLayer.flush()
        formatDescription = nil
        configuration = nil
    }

    // MARK: - Format description

    private static func makeFormatDescription(
        configuration: WindowVideoConfiguration,
        units: [Data]
    ) -> CMVideoFormatDescription? {
        guard let codec = configuration.videoCodec else { return nil }
        switch codec {
        case .h264:
            let sets = VideoBitstream.h264ParameterSets(from: units)
            guard sets.count >= 2 else { return nil }
            return makeH264FormatDescription(sets)
        case .hevc:
            let sets = VideoBitstream.hevcParameterSets(from: units)
            guard sets.count >= 3 else { return nil }
            return makeHEVCFormatDescription(sets)
        case .av1:
            // AV1 is not presented through this path.
            return nil
        }
    }

    private static func makeH264FormatDescription(_ sets: [Data]) -> CMVideoFormatDescription? {
        withParameterSetPointers(sets) { pointers, sizes in
            var description: CMVideoFormatDescription?
            let status = CMVideoFormatDescriptionCreateFromH264ParameterSets(
                allocator: kCFAllocatorDefault,
                parameterSetCount: pointers.count,
                parameterSetPointers: pointers,
                parameterSetSizes: sizes,
                nalUnitHeaderLength: 4,
                formatDescriptionOut: &description
            )
            return status == noErr ? description : nil
        }
    }

    private static func makeHEVCFormatDescription(_ sets: [Data]) -> CMVideoFormatDescription? {
        withParameterSetPointers(sets) { pointers, sizes in
            var description: CMVideoFormatDescription?
            let status = CMVideoFormatDescriptionCreateFromHEVCParameterSets(
                allocator: kCFAllocatorDefault,
                parameterSetCount: pointers.count,
                parameterSetPointers: pointers,
                parameterSetSizes: sizes,
                nalUnitHeaderLength: 4,
                extensions: nil,
                formatDescriptionOut: &description
            )
            return status == noErr ? description : nil
        }
    }

    /// Copies each parameter set into stable storage and runs `body` while the
    /// pointers remain valid.
    private static func withParameterSetPointers<T>(
        _ sets: [Data],
        _ body: ([UnsafePointer<UInt8>], [Int]) -> T
    ) -> T {
        var buffers: [UnsafeMutableBufferPointer<UInt8>] = []
        defer { buffers.forEach { $0.deallocate() } }

        var pointers: [UnsafePointer<UInt8>] = []
        var sizes: [Int] = []
        for set in sets {
            let buffer = UnsafeMutableBufferPointer<UInt8>.allocate(capacity: max(set.count, 1))
            set.copyBytes(to: buffer)
            buffers.append(buffer)
            pointers.append(UnsafePointer(buffer.baseAddress!))
            sizes.append(set.count)
        }
        return body(pointers, sizes)
    }

    // MARK: - Sample buffer

    private static func makeSampleBuffer(
        frame: EncodedVideoFrame,
        formatDescription: CMVideoFormatDescription
    ) -> CMSampleBuffer? {
        let payload = VideoBitstream.isAnnexB(frame.data)
            ? VideoBitstream.avccFromAnnexB(frame.data)
            : frame.data
        guard !payload.isEmpty else { return nil }

        var blockBuffer: CMBlockBuffer?
        let flags = CMBlockBufferFlags(0)
        guard CMBlockBufferCreateWithMemoryBlock(
            allocator: kCFAllocatorDefault,
            memoryBlock: nil,
            blockLength: payload.count,
            blockAllocator: kCFAllocatorDefault,
            customBlockSource: nil,
            offsetToData: 0,
            dataLength: payload.count,
            flags: flags,
            blockBufferOut: &blockBuffer
        ) == kCMBlockBufferNoErr, let blockBuffer else {
            return nil
        }

        let replaceStatus = payload.withUnsafeBytes { raw -> OSStatus in
            guard let base = raw.baseAddress else { return -1 }
            return CMBlockBufferReplaceDataBytes(
                with: base,
                blockBuffer: blockBuffer,
                offsetIntoDestination: 0,
                dataLength: payload.count
            )
        }
        guard replaceStatus == kCMBlockBufferNoErr else { return nil }

        var timing = CMSampleTimingInfo(
            duration: .invalid,
            presentationTimeStamp: CMTime(value: CMTimeValue(frame.ptsUS), timescale: 1_000_000),
            decodeTimeStamp: .invalid
        )
        var sampleSize = payload.count
        var sampleBuffer: CMSampleBuffer?
        let createStatus = CMSampleBufferCreateReady(
            allocator: kCFAllocatorDefault,
            dataBuffer: blockBuffer,
            formatDescription: formatDescription,
            sampleCount: 1,
            sampleTimingEntryCount: 1,
            sampleTimingArray: &timing,
            sampleSizeEntryCount: 1,
            sampleSizeArray: &sampleSize,
            sampleBufferOut: &sampleBuffer
        )
        guard createStatus == noErr, let sampleBuffer else { return nil }

        if !frame.keyframe, let attachments = CMSampleBufferGetSampleAttachmentsArray(sampleBuffer, createIfNecessary: true) {
            let count = CFArrayGetCount(attachments)
            if count > 0, let raw = CFArrayGetValueAtIndex(attachments, 0) {
                let dictionary = unsafeBitCast(raw, to: CFMutableDictionary.self)
                CFDictionarySetValue(
                    dictionary,
                    Unmanaged.passUnretained(kCMSampleAttachmentKey_NotSync).toOpaque(),
                    Unmanaged.passUnretained(kCFBooleanTrue).toOpaque()
                )
            }
        }
        return sampleBuffer
    }
}
