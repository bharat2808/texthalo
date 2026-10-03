import Foundation
import ScreenCaptureKit
import Vision

enum ScreenText {
    // Keep capture in the signed helper and avoid a subprocess and PNG round trip.
    @available(macOS 14.0, *)
    static func capture(windowID: CGWindowID) async throws -> CGImage {
        try Task.checkCancellation()
        let content = try await SCShareableContent.excludingDesktopWindows(true, onScreenWindowsOnly: true)
        try Task.checkCancellation()
        guard let window = content.windows.first(where: { $0.windowID == windowID }) else {
            throw NSError(domain: "TextHalo", code: 1, userInfo: [NSLocalizedDescriptionKey: "The original window is no longer available."])
        }
        let filter = SCContentFilter(desktopIndependentWindow: window)
        let config = SCStreamConfiguration()
        config.width = max(1, Int(filter.contentRect.width * CGFloat(filter.pointPixelScale)))
        config.height = max(1, Int(filter.contentRect.height * CGFloat(filter.pointPixelScale)))
        config.showsCursor = false
        config.ignoreShadowsSingleWindow = true
        return try await SCScreenshotManager.captureImage(contentFilter: filter, configuration: config)
    }

    static func recognize(_ image: CGImage) async throws -> String {
        let request = VNRecognizeTextRequest()
        // UI text needs low latency. The accurate neural path can stall while
        // initializing Apple's compute runtime; do not retry that path silently.
        request.recognitionLevel = .fast
        request.usesLanguageCorrection = false
        let worker = Task.detached(priority: .userInitiated) {
            try Task.checkCancellation()
            try VNImageRequestHandler(cgImage: image).perform([request])
            try Task.checkCancellation()
            return (request.results ?? []).compactMap { $0.topCandidates(1).first?.string }.joined(separator: "\n")
        }
        return try await withTaskCancellationHandler {
            try Task.checkCancellation()
            return try await worker.value
        } onCancel: {
            worker.cancel()
            request.cancel()
        }
    }
}
