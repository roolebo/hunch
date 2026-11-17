// Draws Hunch's icon, an h in a ring, into an iconset: icon DIR writes
// DIR/icon_SIZE.png and DIR/icon_SIZE@2x.png. It's drawn here rather than
// taken from SF Symbols, whose license keeps them out of app icons.

import AppKit

let accent = NSColor(red: 0.45, green: 0.75, blue: 1.0, alpha: 1)
let tileTop = NSColor(red: 0.10, green: 0.12, blue: 0.20, alpha: 1)
let tileBottom = NSColor(red: 0.18, green: 0.22, blue: 0.40, alpha: 1)

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
        colorsSpace: CGColorSpaceCreateDeviceRGB(), colors: [tileTop.cgColor, tileBottom.cgColor] as CFArray,
        locations: [0, 1])!
    ctx.drawLinearGradient(gradient, start: CGPoint(x: 0, y: 512), end: CGPoint(x: 512, y: 0), options: [])

    // The ring
    ctx.setStrokeColor(accent.cgColor)
    ctx.setLineWidth(26)
    ctx.strokeEllipse(in: CGRect(x: 256 - 165, y: 256 - 165, width: 330, height: 330))

    // The h: a stem, and an arch into its leg; ends cut square
    ctx.setStrokeColor(NSColor.white.cgColor)
    ctx.setLineWidth(40)
    ctx.setLineCap(.butt)
    let left = 256.0 - 58, right = 256.0 + 58, bottom = 256.0 - 95, shoulder = 256.0 + 5
    ctx.move(to: CGPoint(x: left, y: 256 + 105))
    ctx.addLine(to: CGPoint(x: left, y: bottom))
    ctx.strokePath()
    ctx.move(to: CGPoint(x: left, y: shoulder))
    ctx.addArc(center: CGPoint(x: 256, y: shoulder), radius: 58, startAngle: .pi, endAngle: 0, clockwise: true)
    ctx.addLine(to: CGPoint(x: right, y: bottom))
    ctx.strokePath()

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
