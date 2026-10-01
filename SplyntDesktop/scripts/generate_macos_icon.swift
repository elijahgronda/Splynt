#!/usr/bin/env swift

import AppKit
import Foundation

let fileManager = FileManager.default
let project = URL(fileURLWithPath: fileManager.currentDirectoryPath, isDirectory: true)
let icons = project.appendingPathComponent("src-tauri/icons", isDirectory: true)
let sourceURL = icons.appendingPathComponent("icon.png")
let masterURL = icons.appendingPathComponent("icon-macos.png")
let outputURL = icons.appendingPathComponent("icon.icns")

guard let source = NSImage(contentsOf: sourceURL) else {
  fatalError("Could not read \(sourceURL.path)")
}

func bitmap(size: Int) -> NSBitmapImageRep {
  guard let representation = NSBitmapImageRep(
    bitmapDataPlanes: nil,
    pixelsWide: size,
    pixelsHigh: size,
    bitsPerSample: 8,
    samplesPerPixel: 4,
    hasAlpha: true,
    isPlanar: false,
    colorSpaceName: .deviceRGB,
    bytesPerRow: 0,
    bitsPerPixel: 0
  ) else {
    fatalError("Could not allocate a \(size)px icon bitmap")
  }
  return representation
}

func withContext<T>(_ representation: NSBitmapImageRep, draw: () -> T) -> T {
  guard let context = NSGraphicsContext(bitmapImageRep: representation) else {
    fatalError("Could not create an icon graphics context")
  }
  NSGraphicsContext.saveGraphicsState()
  NSGraphicsContext.current = context
  context.imageInterpolation = .high
  context.cgContext.setShouldAntialias(true)
  context.cgContext.clear(NSRect(x: 0, y: 0, width: representation.pixelsWide, height: representation.pixelsHigh))
  defer { NSGraphicsContext.restoreGraphicsState() }
  return draw()
}

func squircle(in rect: CGRect, exponent: Double = 5) -> CGPath {
  let path = CGMutablePath()
  let center = CGPoint(x: rect.midX, y: rect.midY)
  let horizontal = rect.width / 2
  let vertical = rect.height / 2
  let power = 2 / exponent
  for degree in 0...360 {
    let angle = Double(degree) * .pi / 180
    let cosine = cos(angle)
    let sine = sin(angle)
    let point = CGPoint(
      x: center.x + horizontal * (cosine < 0 ? -1 : 1) * pow(abs(cosine), power),
      y: center.y + vertical * (sine < 0 ? -1 : 1) * pow(abs(sine), power)
    )
    if degree == 0 { path.move(to: point) } else { path.addLine(to: point) }
  }
  path.closeSubpath()
  return path
}

// A modern macOS icon occupies roughly 80% of its 1024px canvas. The former
// asset painted to every edge, so Dock rendered it visibly larger than native
// and third-party icons. The transparent safe area and continuous-corner mask
// preserve the existing Splynt mark while matching the platform's optical size.
let masterSize = 1024
let visibleSize = 824
let visibleOrigin = (masterSize - visibleSize) / 2
let master = bitmap(size: masterSize)
withContext(master) {
  let frame = CGRect(x: visibleOrigin, y: visibleOrigin, width: visibleSize, height: visibleSize)
  NSGraphicsContext.current?.cgContext.addPath(squircle(in: frame))
  NSGraphicsContext.current?.cgContext.clip()
  source.draw(in: frame, from: .zero, operation: .sourceOver, fraction: 1)
}

guard let masterPNG = master.representation(using: .png, properties: [:]) else {
  fatalError("Could not encode the macOS icon master")
}
try masterPNG.write(to: masterURL, options: .atomic)

let masterImage = NSImage(size: NSSize(width: masterSize, height: masterSize))
masterImage.addRepresentation(master)

func png(size: Int) -> Data {
  let representation = bitmap(size: size)
  withContext(representation) {
    masterImage.draw(
      in: NSRect(x: 0, y: 0, width: size, height: size),
      from: NSRect(x: 0, y: 0, width: masterSize, height: masterSize),
      operation: .sourceOver,
      fraction: 1
    )
  }
  guard let data = representation.representation(using: .png, properties: [:]) else {
    fatalError("Could not encode the \(size)px icon")
  }
  return data
}

let iconset = fileManager.temporaryDirectory
  .appendingPathComponent("splice-\(UUID().uuidString).iconset", isDirectory: true)
try fileManager.createDirectory(at: iconset, withIntermediateDirectories: true)
defer { try? fileManager.removeItem(at: iconset) }

let members: [(String, Int)] = [
  ("icon_16x16.png", 16),
  ("icon_16x16@2x.png", 32),
  ("icon_32x32.png", 32),
  ("icon_32x32@2x.png", 64),
  ("icon_128x128.png", 128),
  ("icon_128x128@2x.png", 256),
  ("icon_256x256.png", 256),
  ("icon_256x256@2x.png", 512),
  ("icon_512x512.png", 512),
  ("icon_512x512@2x.png", 1024),
]

for (name, size) in members {
  try png(size: size).write(to: iconset.appendingPathComponent(name), options: .atomic)
}

let iconutil = Process()
iconutil.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
iconutil.arguments = ["-c", "icns", "-o", outputURL.path, iconset.path]
try iconutil.run()
iconutil.waitUntilExit()
guard iconutil.terminationStatus == 0 else {
  fatalError("iconutil failed with status \(iconutil.terminationStatus)")
}

print("Wrote \(masterURL.path)")
print("Wrote \(outputURL.path)")
