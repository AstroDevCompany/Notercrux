// Flattens the square logo onto white (iOS app icons must be opaque) and writes a 1024 px PNG.
// Usage: swift scripts/make-ios-icon.swift <input.png> <output.png>
import AppKit

let args = CommandLine.arguments
guard args.count == 3, let src = NSImage(contentsOfFile: args[1]) else {
    FileHandle.standardError.write(Data("usage: make-ios-icon <in> <out>\n".utf8)); exit(1)
}
let size = 1024
let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: size, pixelsHigh: size, bitsPerSample: 8,
                           samplesPerPixel: 3, hasAlpha: false, isPlanar: false,
                           colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
NSColor.white.setFill()
NSRect(x: 0, y: 0, width: size, height: size).fill()
src.draw(in: NSRect(x: 0, y: 0, width: size, height: size))
NSGraphicsContext.restoreGraphicsState()
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: args[2]))
