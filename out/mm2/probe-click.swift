// Post a real left click at screen coordinates, for live input checks of
// the windowed macOS build (System Events' `click at` never reaches SDL).
// Needs Accessibility permission for the app running it.
//
//   xcrun swiftc -O out/mm2/probe-click.swift -o /tmp/probe-click
//   /tmp/probe-click 912 562
//
// Keys can be sent with:  osascript -e 'tell application "System Events" to key code 125'
import Foundation
import CoreGraphics
let a = CommandLine.arguments
guard a.count == 3, let x = Double(a[1]), let y = Double(a[2]) else { print("usage: click x y"); exit(2) }
let p = CGPoint(x: x, y: y)
func post(_ t: CGEventType) {
    guard let e = CGEvent(mouseEventSource: nil, mouseType: t, mouseCursorPosition: p, mouseButton: .left) else { print("event failed"); exit(1) }
    e.post(tap: .cghidEventTap)
    usleep(60000)
}
post(.mouseMoved); post(.leftMouseDown); post(.leftMouseUp)
print("clicked \(x),\(y)")
