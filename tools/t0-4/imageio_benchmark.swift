import CoreGraphics
import Foundation
import ImageIO

struct Arguments {
    let inputRoot: URL
    let output: URL
    let iterations: Int
    let warmups: Int
    let maxPixel: Int
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

    guard iterations > 0, warmups >= 0, maxPixel > 0 else {
        throw BenchmarkError.invalidValue("iterations, warmups, and max-pixel must be positive")
    }

    return Arguments(
        inputRoot: inputRoot,
        output: output,
        iterations: iterations,
        warmups: warmups,
        maxPixel: maxPixel
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

func decode(_ url: URL, mode: String, maxPixel: Int) -> CGImage? {
    guard let imageSource = source(for: url) else { return nil }

    let image: CGImage?
    switch mode {
    case "full-decode-scale":
        image = CGImageSourceCreateImageAtIndex(imageSource, 0, [
            kCGImageSourceShouldCache: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
        ] as CFDictionary)
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
    let resized = mode == "full-decode-scale" ? scaledImage(image, maxPixel: maxPixel) : image
    guard let resized else { return nil }
    return makeSrgbImage(resized)
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

func measure(url: URL, mode: String, arguments: Arguments) -> Observation {
    for _ in 0..<arguments.warmups {
        _ = decode(url, mode: mode, maxPixel: arguments.maxPixel)
    }

    var durations: [Double] = []
    var lastImage: CGImage?
    var peakResident: UInt64?

    for _ in 0..<arguments.iterations {
        let start = DispatchTime.now().uptimeNanoseconds
        let image = decode(url, mode: mode, maxPixel: arguments.maxPixel)
        let end = DispatchTime.now().uptimeNanoseconds
        guard let image else {
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
                error: "ImageIO could not decode or normalize the image"
            )
        }
        durations.append(elapsedMilliseconds(start, end))
        lastImage = image
        if let current = residentBytes() {
            peakResident = max(peakResident ?? current, current)
        }
    }

    return Observation(
        mode: mode,
        path: url.path,
        success: true,
        durationP50Milliseconds: percentile(durations, 0.50),
        durationP95Milliseconds: percentile(durations, 0.95),
        residentBytes: peakResident,
        width: lastImage?.width,
        height: lastImage?.height,
        colorSpace: lastImage.flatMap(colorSpaceName),
        error: nil
    )
}

do {
    let arguments = try parseArguments()
    let urls = try imageURLs(in: arguments.inputRoot)
    let modes = ["full-decode-scale", "imageio-thumbnail", "embedded-preferred"]
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
