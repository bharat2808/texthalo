import AppKit

@main struct ScreenTextTests {
    @MainActor static func main() async throws {
        let image = NSImage(size: NSSize(width: 1000, height: 300))
        image.lockFocus()
        NSColor.white.setFill()
        NSBezierPath(rect: NSRect(x: 0, y: 0, width: 1000, height: 300)).fill()
        let attributes: [NSAttributedString.Key: Any] = [.font: NSFont.systemFont(ofSize: 32), .foregroundColor: NSColor.black]
        ("TextHalo screen context" as NSString).draw(at: NSPoint(x: 30, y: 210), withAttributes: attributes)
        ("Order 12345 is ready" as NSString).draw(at: NSPoint(x: 30, y: 140), withAttributes: attributes)
        image.unlockFocus()
        var rect = NSRect(origin: .zero, size: image.size)
        let cgImage = image.cgImage(forProposedRect: &rect, context: nil, hints: nil)!
        let text = try await ScreenText.recognize(cgImage)
        assert(text.contains("TextHalo screen context"), "Missing heading: \(text)")
        assert(text.contains("Order 12345 is ready"), "Missing body: \(text)")
        let canceled = Task {
            withUnsafeCurrentTask { $0?.cancel() }
            return try await ScreenText.recognize(cgImage)
        }
        do { _ = try await canceled.value; assertionFailure("Canceled OCR must not return text") }
        catch is CancellationError {}
        let blank = NSImage(size: NSSize(width: 300, height: 100))
        blank.lockFocus()
        NSColor.white.setFill()
        NSBezierPath(rect: NSRect(x: 0, y: 0, width: 300, height: 100)).fill()
        blank.unlockFocus()
        var blankRect = NSRect(origin: .zero, size: blank.size)
        let blankImage = blank.cgImage(forProposedRect: &blankRect, context: nil, hints: nil)!
        do { _ = try await ScreenText.recognize(blankImage); assertionFailure("Blank screen OCR must report unreadable text instead of generating from metadata") }
        catch { assert(error.localizedDescription.contains("No readable text found")) }
        print("Screen text recognition and cancellation passed")
    }
}
