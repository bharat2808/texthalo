import CoreGraphics
import Foundation
import ScreenCaptureKit

enum ScreenCapture {
    /// Capture the original target window entirely in memory. The image is sent
    /// used for OCR, or sent to Foundation Models when image input is enabled.
    @available(macOS 14.0, *)
    static func window(_ windowID: CGWindowID) async throws -> CGImage {
        try Task.checkCancellation()
        let content = try await SCShareableContent.excludingDesktopWindows(true, onScreenWindowsOnly: true)
        try Task.checkCancellation()
        guard let window = content.windows.first(where: { $0.windowID == windowID }) else {
            throw NSError(domain: "TextHalo.ScreenCapture", code: 1,
                          userInfo: [NSLocalizedDescriptionKey: "The original window is no longer available."])
        }
        let filter = SCContentFilter(desktopIndependentWindow: window)
        let config = SCStreamConfiguration()
        config.width = max(1, Int(filter.contentRect.width * CGFloat(filter.pointPixelScale)))
        config.height = max(1, Int(filter.contentRect.height * CGFloat(filter.pointPixelScale)))
        config.showsCursor = false
        config.ignoreShadowsSingleWindow = true
        let image = try await SCScreenshotManager.captureImage(contentFilter: filter, configuration: config)
        try Task.checkCancellation()
        return image
    }
}
