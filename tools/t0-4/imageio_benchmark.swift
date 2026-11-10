import CoreGraphics
import CoreImage
import Foundation
import ImageIO

let orientationContext = CIContext(options: [.useSoftwareRenderer: true])

struct Arguments {
    let inputRoot: URL
    let output: URL
    let iterations: Int
    let warmups: Int
    let maxPixel: Int
    let mode: String?
}

struct Observation: Codable {
    let mode: String
    let path: String
    let success: Bool
    let durationP50Milliseconds: Double?
    let durationP95Milliseconds: Double?
    let residentBytes: UInt64?
    let width: Int?
    let height: Int?
    let colorSpace: String?
    let orientation: Int?
    let orientationCorrect: Bool?
    let srgbCorrect: Bool?
    let sourceType: String?
    let error: String?
}

struct Report: Codable {
    let generatedAt: String
    let machine: String
    let osVersion: String
    let architecture: String
    let maxPixel: Int
    let iterations: Int
    let warmups: Int
    let mode: String?
    let observations: [Observation]
}

enum BenchmarkError: LocalizedError {
    case missingValue(String)
    case invalidValue(String)

    var errorDescription: String? {
        switch self {
        case let .missingValue(option):
            return "missing value for \(option)"
        case let .invalidValue(value):
            return "invalid value: \(value)"
        }
    }
}

func argumentValue(_ name: String, in arguments: [String]) throws -> String {
    guard let index = arguments.firstIndex(of: name), index + 1 < arguments.count else {
        throw BenchmarkError.missingValue(name)
    }
    return arguments[index + 1]
}

func parseArguments() throws -> Arguments {
    let arguments = Array(CommandLine.arguments.dropFirst())
    let inputRoot = URL(fileURLWithPath: try argumentValue("--input-root", in: arguments), isDirectory: true)
    let output = URL(fileURLWithPath: try argumentValue("--output", in: arguments))
    let iterations = Int((try? argumentValue("--iterations", in: arguments)) ?? "5") ?? 5
    let warmups = Int((try? argumentValue("--warmups", in: arguments)) ?? "2") ?? 2
    let maxPixel = Int((try? argumentValue("--max-pixel", in: arguments)) ?? "512") ?? 512
    let mode = try? argumentValue("--mode", in: arguments)
    let validModes = ["full-decode-scale", "imageio-thumbnail", "embedded-preferred"]

    guard iterations > 0, warmups >= 0, maxPixel > 0,
          mode == nil || validModes.contains(mode!) else {
        throw BenchmarkError.invalidValue("iterations, warmups, and max-pixel must be positive")
    }

    return Arguments(
        inputRoot: inputRoot,
        output: output,
        iterations: iterations,
        warmups: warmups,
        maxPixel: maxPixel,
        mode: mode
    )
}

func imageURLs(in root: URL) throws -> [URL] {
    let allowedExtensions = Set(["heic", "heif", "jpg", "jpeg", "png", "webp"])
    let keys: [URLResourceKey] = [.isRegularFileKey]
    guard let enumerator = FileManager.default.enumerator(
        at: root,
        includingPropertiesForKeys: keys,
        options: [.skipsHiddenFiles]
    ) else {
        throw BenchmarkError.invalidValue("cannot enumerate \(root.path)")
    }

    return enumerator.compactMap { item in
        guard let url = item as? URL,
              let values = try? url.resourceValues(forKeys: Set(keys)),
              values.isRegularFile == true,
              allowedExtensions.contains(url.pathExtension.lowercased()) else {
            return nil
        }
        return url
    }.sorted { $0.path < $1.path }
}

func source(for url: URL) -> CGImageSource? {
    CGImageSourceCreateWithURL(url as CFURL, nil)
}

func thumbnailOptions(maxPixel: Int, always: Bool, ifAbsent: Bool) -> CFDictionary {
    [
        kCGImageSourceCreateThumbnailFromImageAlways: always,
        kCGImageSourceCreateThumbnailFromImageIfAbsent: ifAbsent,
        kCGImageSourceCreateThumbnailWithTransform: true,
        kCGImageSourceThumbnailMaxPixelSize: maxPixel,
    ] as CFDictionary
}

func makeSrgbImage(_ image: CGImage) -> CGImage? {
    guard let colorSpace = CGColorSpace(name: CGColorSpace.sRGB),
          let context = CGContext(
              data: nil,
              width: image.width,
              height: image.height,
              bitsPerComponent: 8,
              bytesPerRow: 0,
              space: colorSpace,
              bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
          ) else {
        return nil
    }

    context.interpolationQuality = .high
    context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
    return context.makeImage()
}

func scaledImage(_ image: CGImage, maxPixel: Int) -> CGImage? {
    let scale = min(1.0, Double(maxPixel) / Double(max(image.width, image.height)))
    let width = max(1, Int((Double(image.width) * scale).rounded()))
    let height = max(1, Int((Double(image.height) * scale).rounded()))
    guard let colorSpace = image.colorSpace ?? CGColorSpace(name: CGColorSpace.sRGB),
          let context = CGContext(
              data: nil,
              width: width,
              height: height,
              bitsPerComponent: 8,
              bytesPerRow: 0,
              space: colorSpace,
              bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
          ) else {
        return nil
    }

    context.interpolationQuality = .high
    context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
    return context.makeImage()
}

func sourceOrientation(_ source: CGImageSource) -> Int {
    guard let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
          let orientation = properties[kCGImagePropertyOrientation] as? NSNumber else {
        return 1
    }
    return orientation.intValue
}

func orient(_ image: CGImage, orientation: Int) -> CGImage? {
    guard orientation != 1 else { return image }
    let oriented = CIImage(cgImage: image).oriented(forExifOrientation: Int32(orientation))
    return orientationContext.createCGImage(oriented, from: oriented.extent)
}

func decode(_ url: URL, mode: String, maxPixel: Int) -> (CGImage, Int, String?)? {
    guard let imageSource = source(for: url) else { return nil }
    let orientation = sourceOrientation(imageSource)
    let sourceType = CGImageSourceGetType(imageSource) as String?

    let image: CGImage?
    switch mode {
    case "full-decode-scale":
        image = CGImageSourceCreateImageAtIndex(imageSource, 0, [kCGImageSourceShouldCache: true] as CFDictionary)
    case "imageio-thumbnail":
        image = CGImageSourceCreateThumbnailAtIndex(
            imageSource,
            0,
            thumbnailOptions(maxPixel: maxPixel, always: true, ifAbsent: true)
        )
    case "embedded-preferred":
        let embedded = CGImageSourceCreateThumbnailAtIndex(
            imageSource,
            0,
            thumbnailOptions(maxPixel: maxPixel, always: false, ifAbsent: false)
        )
        image = embedded ?? CGImageSourceCreateThumbnailAtIndex(
            imageSource,
            0,
            thumbnailOptions(maxPixel: maxPixel, always: true, ifAbsent: true)
        )
    default:
        return nil
    }

    guard let image else { return nil }
    let transformed = mode == "full-decode-scale" ? orient(image, orientation: orientation) : image
    guard let transformed else { return nil }
    let resized = mode == "full-decode-scale" ? scaledImage(transformed, maxPixel: maxPixel) : transformed
    guard let resized else { return nil }
    guard let normalized = makeSrgbImage(resized) else { return nil }
    return (normalized, orientation, sourceType)
}

func residentBytes() -> UInt64? {
    var usage = rusage()
    guard getrusage(RUSAGE_SELF, &usage) == 0 else { return nil }
    return UInt64(usage.ru_maxrss)
}

func elapsedMilliseconds(_ start: UInt64, _ end: UInt64) -> Double {
    Double(end - start) / 1_000_000.0
}

func percentile(_ values: [Double], _ percentile: Double) -> Double {
    let sorted = values.sorted()
    let index = min(sorted.count - 1, Int(Double(sorted.count - 1) * percentile))
    return sorted[index]
}

func colorSpaceName(_ image: CGImage) -> String? {
    image.colorSpace?.name as String?
}

func expectedSize(source: CGImageSource, orientation: Int, maxPixel: Int) -> (Int, Int)? {
    guard let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
          let width = properties[kCGImagePropertyPixelWidth] as? NSNumber,
          let height = properties[kCGImagePropertyPixelHeight] as? NSNumber else { return nil }
    var w = width.intValue
    var h = height.intValue
    if [5, 6, 7, 8].contains(orientation) { swap(&w, &h) }
    let scale = min(1.0, Double(maxPixel) / Double(max(w, h)))
    return (max(1, Int((Double(w) * scale).rounded())), max(1, Int((Double(h) * scale).rounded())))
}

func measure(url: URL, mode: String, arguments: Arguments) -> Observation {
    for _ in 0..<arguments.warmups {
        _ = decode(url, mode: mode, maxPixel: arguments.maxPixel)
    }

    var durations: [Double] = []
    var lastResult: (CGImage, Int, String?)?
    var peakResident: UInt64?

    for _ in 0..<arguments.iterations {
        let start = DispatchTime.now().uptimeNanoseconds
        let result = decode(url, mode: mode, maxPixel: arguments.maxPixel)
        let end = DispatchTime.now().uptimeNanoseconds
        guard let result else {
            return Observation(
                mode: mode,
                path: url.path,
                success: false,
                durationP50Milliseconds: nil,
                durationP95Milliseconds: nil,
                residentBytes: residentBytes(),
                width: nil,
                height: nil,
                colorSpace: nil,
                orientation: nil,
                orientationCorrect: nil,
                srgbCorrect: nil,
                sourceType: nil,
                error: "ImageIO could not decode or normalize the image"
            )
        }
        durations.append(elapsedMilliseconds(start, end))
        lastResult = result
        if let current = residentBytes() {
            peakResident = max(peakResident ?? current, current)
        }
    }

    let expected = source(for: url).flatMap {
        expectedSize(source: $0, orientation: lastResult?.1 ?? 1, maxPixel: arguments.maxPixel)
    }
    let output = lastResult?.0
    return Observation(
        mode: mode,
        path: url.path,
        success: true,
        durationP50Milliseconds: percentile(durations, 0.50),
        durationP95Milliseconds: percentile(durations, 0.95),
        residentBytes: peakResident,
        width: output?.width,
        height: output?.height,
        colorSpace: output.flatMap(colorSpaceName),
        orientation: lastResult?.1,
        orientationCorrect: expected.map {
            guard let output else { return false }
            let ratioError = abs(output.width * $0.1 - $0.0 * output.height)
            return ratioError <= max($0.0, $0.1) && max(output.width, output.height) <= arguments.maxPixel
        },
        srgbCorrect: output.flatMap(colorSpaceName) == CGColorSpace.sRGB as String,
        sourceType: lastResult?.2,
        error: nil
    )
}

do {
    let arguments = try parseArguments()
    let urls = try imageURLs(in: arguments.inputRoot)
    let modes = arguments.mode.map { [$0] } ?? ["full-decode-scale", "imageio-thumbnail", "embedded-preferred"]
    let observations = modes.flatMap { mode in
        urls.map { measure(url: $0, mode: mode, arguments: arguments) }
    }

    let report = Report(
        generatedAt: ISO8601DateFormatter().string(from: Date()),
        machine: Host.current().localizedName ?? "unknown",
        osVersion: ProcessInfo.processInfo.operatingSystemVersionString,
        architecture: "arm64",
        maxPixel: arguments.maxPixel,
        iterations: arguments.iterations,
        warmups: arguments.warmups,
        mode: arguments.mode,
        observations: observations
    )
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
    try encoder.encode(report).write(to: arguments.output)
    print("wrote \(observations.count) observations to \(arguments.output.path)")
} catch {
    fputs("error: \(error.localizedDescription)\n", stderr)
    exit(1)
}
