// Renders the macOS-style app icon master: the artwork scaled to 824 pt on a transparent 1024 pt
// canvas (Apple's icon grid), with a soft drop shadow. Usage: swift macos-icon.swift in.png out.png
import Foundation
import CoreGraphics
import ImageIO
import UniformTypeIdentifiers

let args = CommandLine.arguments
guard args.count == 3,
      let src = CGImageSourceCreateWithURL(URL(fileURLWithPath: args[1]) as CFURL, nil),
      let art = CGImageSourceCreateImageAtIndex(src, 0, nil) else {
    FileHandle.standardError.write(Data("usage: macos-icon.swift in.png out.png\n".utf8))
    exit(1)
}

let canvas = 1024, body = 824, inset = (canvas - body) / 2
let ctx = CGContext(data: nil, width: canvas, height: canvas, bitsPerComponent: 8, bytesPerRow: 0,
                    space: CGColorSpace(name: CGColorSpace.sRGB)!,
                    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
ctx.interpolationQuality = .high
let rect = CGRect(x: inset, y: inset, width: body, height: body)
ctx.setShadow(offset: CGSize(width: 0, height: -12), blur: 24, color: CGColor(gray: 0, alpha: 0.28))
ctx.draw(art, in: rect)

let out = CGImageDestinationCreateWithURL(URL(fileURLWithPath: args[2]) as CFURL, UTType.png.identifier as CFString, 1, nil)!
CGImageDestinationAddImage(out, ctx.makeImage()!, nil)
guard CGImageDestinationFinalize(out) else { exit(2) }
