// Draws Hunch's icon, a bold slanted H on a yellow tile, into an iconset:
// icon DIR writes DIR/icon_SIZE.png and DIR/icon_SIZE@2x.png. It's drawn
// here rather than set in a font, so it looks the same on every Mac.

import AppKit

let tileTop = CGColor(red: 1.0, green: 0.80, blue: 0.15, alpha: 1)
let tileBottom = CGColor(red: 1.0, green: 0.42, blue: 0.10, alpha: 1)
let ink = CGColor(red: 0.07, green: 0.07, blue: 0.09, alpha: 1)
/// How far the H leans: x moves this much per unit of y
let slant = 0.14

/// The icon at side pixels, drawn on a 512-point grid
func icon(side: Int) -> NSBitmapImageRep {
    let rep = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: side, pixelsHigh: side, bitsPerSample: 8,
        samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
        bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    let ctx = NSGraphicsContext.current!.cgContext
    ctx.scaleBy(x: Double(side) / 512, y: Double(side) / 512)

    // The tile, as macOS app icons are shaped
    ctx.addPath(CGPath(roundedRect: CGRect(x: 40, y: 40, width: 432, height: 432), cornerWidth: 100, cornerHeight: 100, transform: nil))
    ctx.clip()
    let gradient = CGGradient(
        colorsSpace: CGColorSpaceCreateDeviceRGB(), colors: [tileTop, tileBottom] as CFArray, locations: [0, 1])!
    ctx.drawLinearGradient(gradient, start: CGPoint(x: 0, y: 512), end: CGPoint(x: 512, y: 0), options: [])

    // The H: two stems and a crossbar, leaning about the center
    ctx.translateBy(x: 256, y: 256)
    ctx.concatenate(CGAffineTransform(a: 1, b: 0, c: slant, d: 1, tx: 0, ty: 0))
    let width = 250.0, height = 300.0, stem = 82.0, bar = 60.0
    ctx.setFillColor(ink)
    ctx.fill(CGRect(x: -width / 2, y: -height / 2, width: stem, height: height))
    ctx.fill(CGRect(x: width / 2 - stem, y: -height / 2, width: stem, height: height))
    ctx.fill(CGRect(x: -width / 2 + stem - 1, y: -bar / 2, width: width - 2 * stem + 2, height: bar))

    NSGraphicsContext.restoreGraphicsState()
    return rep
}

let dir = URL(fileURLWithPath: CommandLine.arguments[1])
try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
for size in [16, 32, 128, 256, 512] {
    for scale in [1, 2] {
        let name = scale == 1 ? "icon_\(size)x\(size).png" : "icon_\(size)x\(size)@2x.png"
        let png = icon(side: size * scale).representation(using: .png, properties: [:])!
        try png.write(to: dir.appendingPathComponent(name))
    }
}
