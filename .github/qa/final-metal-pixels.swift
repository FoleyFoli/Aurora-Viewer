import Foundation
import CoreGraphics
import ImageIO

// Decode through the same native path used by the earlier Metal diagnostics.
guard CommandLine.arguments.count == 2,
      let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: CommandLine.arguments[1]) as CFURL, nil),
      let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else { exit(2) }
let width = image.width, height = image.height
guard width >= 128, height >= 96, width <= 4096, height <= 4096 else { exit(3) }
let format = CGBitmapInfo.byteOrder32Big.rawValue | CGImageAlphaInfo.premultipliedLast.rawValue
var bytes = [UInt8](repeating: 0, count: width * height * 4)
let stats: [String: Int]? = bytes.withUnsafeMutableBytes { raw in
    guard let context = CGContext(data: raw.baseAddress, width: width, height: height, bitsPerComponent: 8,
                                  bytesPerRow: width * 4, space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: format) else { return nil }
    context.draw(image, in: CGRect(x: 0, y: 0, width: CGFloat(width), height: CGFloat(height)))
    let rgb = raw.bindMemory(to: UInt8.self)
    var nonblack = 0, interior = 0, interiorPixels = 0
    var bins = Set<Int>()
    for y in 0..<height {
        for x in 0..<width {
            let p = (y * width + x) * 4
            let lit = (rgb[p] | rgb[p+1] | rgb[p+2]) != 0
            if lit { nonblack += 1 }
            if y >= 48 && y < height - 48 && x >= 64 && x < width - 64 {
                interiorPixels += 1
                if lit { interior += 1 }
                bins.insert((Int(rgb[p]) >> 4) << 8 | (Int(rgb[p+1]) >> 4) << 4 | (Int(rgb[p+2]) >> 4))
            }
        }
    }
    return ["width": width, "height": height, "pixels": width * height, "nonblack": nonblack,
            "interior_pixels": interiorPixels, "interior_nonblack": interior, "interior_rgb_bins": bins.count]
}
guard let stats = stats,
      let data = try? JSONSerialization.data(withJSONObject: stats, options: [.sortedKeys]),
      let text = String(data: data, encoding: .utf8) else { exit(4) }
print(text)
