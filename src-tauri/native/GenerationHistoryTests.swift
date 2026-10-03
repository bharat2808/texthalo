import AppKit
import Foundation

@main struct GenerationHistoryTests {
    @MainActor static func main() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let image = CGContext(data: nil, width: 2, height: 2, bitsPerComponent: 8, bytesPerRow: 8,
                              space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!.makeImage()!
        for n in 0..<55 {
            let run = GenerationHistory(requestID: UInt64(n), source: "Test", directory: root.path)!
            run.input(["request": "request \(n)", "transcript": "spoken \(n)", "systemInstructions": "instructions", "userPrompt": "context then request"])
            run.screenshot(image)
            run.raw("1|Answer \(n)")
            run.update(phase: "streaming", message: "Generating", text: "Answer \(n)", action: "speak")
            run.update(phase: n == 54 ? "error" : "result", message: "Final status", text: "Answer \(n)", action: "speak")
            run.update(phase: "canceled", message: "Must not overwrite terminal status", text: "", action: "")
            assert(run.error == nil)
            Thread.sleep(forTimeInterval: 0.002)
        }
        let files = try FileManager.default.contentsOfDirectory(at: root, includingPropertiesForKeys: nil).sorted { $0.lastPathComponent > $1.lastPathComponent }
        assert(files.count == 50, "Retention removes records and embedded screenshots together")
        let latest = try JSONSerialization.jsonObject(with: Data(contentsOf: files[0])) as! [String: Any]
        assert(latest["request"] as? String == "request 54")
        assert(latest["rawOutput"] as? String == "1|Answer 54")
        assert(latest["output"] as? String == "Answer 54")
        assert(latest["status"] as? String == "error")
        assert((latest["screenshot"] as? String)?.hasPrefix("data:image/png;base64,") == true)
        let attrs = try FileManager.default.attributesOfItem(atPath: files[0].path)
        assert((attrs[.posixPermissions] as? NSNumber)?.intValue == 0o600)
        let canceled = GenerationHistory(requestID: 56, source: "Test", directory: root.path)!
        canceled.update(phase: "canceled", message: "Canceled before generation", text: "", action: "")
        assert(canceled.error == nil)
        print("Generation audit retention, screenshots, raw output, errors, cancellation, and permissions passed")
    }
}
