import CoreGraphics
import Foundation
import ImageIO

enum FixtureError: Error { case usage, context, image, unsupported(String), write(String) }

guard CommandLine.arguments.count == 2 else { throw FixtureError.usage }
let root = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)

let width = 4032
let height = 3024
guard let colorSpace = CGColorSpace(name: CGColorSpace.displayP3),
      let context = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8,
                              bytesPerRow: 0, space: colorSpace,
                              bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else {
    throw FixtureError.context
}
context.setFillColor(CGColor(red: 0.91, green: 0.12, blue: 0.08, alpha: 1))
context.fill(CGRect(x: 0, y: 0, width: width / 2, height: height / 2))
context.setFillColor(CGColor(red: 0.08, green: 0.72, blue: 0.18, alpha: 1))
context.fill(CGRect(x: width / 2, y: 0, width: width / 2, height: height / 2))
context.setFillColor(CGColor(red: 0.08, green: 0.18, blue: 0.91, alpha: 1))
context.fill(CGRect(x: 0, y: height / 2, width: width / 2, height: height / 2))
context.setFillColor(CGColor(gray: 0.85, alpha: 1))
context.fill(CGRect(x: width / 2, y: height / 2, width: width / 2, height: height / 2))
guard let image = context.makeImage() else { throw FixtureError.image }

let formats = [
    ("fixture.heic", "public.heic"),
    ("fixture.jpg", "public.jpeg"),
    ("fixture.png", "public.png"),
]
let supported = Set(CGImageDestinationCopyTypeIdentifiers() as! [String])
for (name, type) in formats {
    guard supported.contains(type) else {
        print("skipping unavailable ImageIO destination")
        continue
    }
    let url = root.appendingPathComponent(name)
    guard let destination = CGImageDestinationCreateWithURL(url as CFURL, type as CFString, 1, nil) else {
        print("skipping unavailable ImageIO destination")
        continue
    }
    var properties: [CFString: Any] = [
        kCGImagePropertyOrientation: 6,
        kCGImageDestinationLossyCompressionQuality: 0.9,
    ]
    if type == "public.jpeg" { properties[kCGImageDestinationEmbedThumbnail] = true }
    CGImageDestinationAddImage(destination, image, properties as CFDictionary)
    guard CGImageDestinationFinalize(destination) else {
        print("skipping ImageIO destination that could not be finalized")
        try? FileManager.default.removeItem(at: url)
        continue
    }
}
// ImageIO decodes WebP on macOS but does not expose a WebP destination. Keep a
// tiny deterministic fixture in source form so the repository contains no
// binary test asset. Large WebP performance fixtures are generated with Pillow
// by the documented benchmark procedure.
let webp = Data(base64Encoded: "UklGRiIAAABXRUJQVlA4IBYAAAAwAQCdASoBAAEALmk0mk0iIiIiIgBoSygABc6zbAAA")!
try webp.write(to: root.appendingPathComponent("fixture.webp"))
try Data().write(to: root.appendingPathComponent("empty.jpg"))
try Data("not an image".utf8).write(to: root.appendingPathComponent("corrupt.webp"))
print("generated deterministic fixtures in \(root.path)")
