import Foundation
import CoreGraphics
import ImageIO

// Only select a window belonging to the viewer process. Never capture the desktop.
func windowID(pid: Int) -> Int? {
    let options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
    guard let rows = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] else { return nil }
    let windows: [(Int, Int)] = rows.compactMap { row in
        guard row[kCGWindowOwnerPID as String] as? Int == pid,
              row[kCGWindowLayer as String] as? Int == 0,
              let bounds = row[kCGWindowBounds as String] as? [String: Any],
              let w = bounds["Width"] as? NSNumber, let h = bounds["Height"] as? NSNumber,
              w.intValue >= 700, h.intValue >= 450,
              let id = row[kCGWindowNumber as String] as? Int else { return nil }
        return (w.intValue * h.intValue, id)
    }
    return windows.max(by: { $0.0 < $1.0 })?.1
}

func statistics(path: String) -> String? {
    guard let src = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil),
          let image = CGImageSourceCreateImageAtIndex(src, 0, nil) else { return nil }
    let w = image.width, h = image.height
    let format = CGBitmapInfo.byteOrder32Big.rawValue | CGImageAlphaInfo.premultipliedLast.rawValue
    var bytes = [UInt8](repeating: 0, count: w * h * 4)
    return bytes.withUnsafeMutableBytes { raw -> String? in
        guard let ctx = CGContext(data: raw.baseAddress, width: w, height: h, bitsPerComponent: 8,
                                  bytesPerRow: w * 4, space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: format) else { return nil }
        ctx.draw(image, in: CGRect(x: 0, y: 0, width: CGFloat(w), height: CGFloat(h)))
        let rgb = raw.bindMemory(to: UInt8.self)
        var coloured = 0, interior = 0, interiorPixels = 0
        for y in 0..<h {
            for x in 0..<w {
                let p = (y * w + x) * 4
                let lit = (rgb[p] | rgb[p+1] | rgb[p+2]) != 0
                if lit { coloured += 1 }
                if y >= 48 && y < h - 48 && x >= 64 && x < w - 64 {
                    interiorPixels += 1
                    if lit { interior += 1 }
                }
            }
        }
        return "\(w)\t\(h)\t\(coloured)\t\(w*h)\t\(interior)\t\(interiorPixels)"
    }
}

let args = CommandLine.arguments
guard args.count == 3 else { exit(2) }
if args[1] == "window", let pid = Int(args[2]), let window = windowID(pid: pid) {
    print(window)
} else if args[1] == "stats", let result = statistics(path: args[2]) {
    print(result)
} else {
    exit(1)
}
