import AppKit
import Foundation

/// A private, request-owned audit record. One file includes its screenshot so
/// retention cannot leave orphaned images behind. Writes replace files atomically.
@MainActor final class GenerationHistory {
    private let file: URL
    private var record: [String: Any]
    private var finished = false
    private var lastWrite = Date.distantPast
    private(set) var error: String?

    init?(requestID: UInt64, source: String, directory: String? = ProcessInfo.processInfo.environment["TEXTHALO_AUDIT_DIR"]) {
        guard let directory else { return nil }
        let root = URL(fileURLWithPath: directory, isDirectory: true)
        let now = Date().timeIntervalSince1970 * 1000
        let key = String(format: "%013.0f", now) + "-" + UUID().uuidString
        file = root.appendingPathComponent(key + ".json")
        record = ["id": key, "requestId": requestID, "createdAt": now,
                  "source": source, "status": "started", "events": [[String: Any]](),
                  "model": "Apple Foundation Models (on-device)",
                  "osVersion": ProcessInfo.processInfo.operatingSystemVersionString]
        do {
            try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true,
                                                   attributes: [.posixPermissions: 0o700])
            persist(force: true)
            guard error == nil else { return }
            let files = try FileManager.default.contentsOfDirectory(at: root, includingPropertiesForKeys: nil)
                .filter { $0.pathExtension == "json" }.sorted { $0.lastPathComponent > $1.lastPathComponent }
            for old in files.dropFirst(50) { try FileManager.default.removeItem(at: old) }
        } catch { self.error = error.localizedDescription }
    }
    func input(_ values: [String: Any]) {
        guard !finished else { return }
        record.merge(values) { _, new in new }
        persist(force: true)
    }
    func screenshot(_ image: CGImage) {
        guard let png = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:]) else {
            error = "Could not encode the captured screenshot."; return
        }
        input(["screenshot": "data:image/png;base64," + png.base64EncodedString()])
    }
    func raw(_ text: String) {
        guard !finished else { return }
        record["rawOutput"] = text
        persist(force: false)
    }
    func update(phase: String, message: String, text: String, action: String) {
        guard !finished else { return }
        record["status"] = phase
        record["message"] = message
        record["action"] = action
        if phase == "recording", !text.isEmpty { record["transcript"] = text }
        if phase == "streaming" || phase == "result" || phase == "error", !text.isEmpty { record["output"] = text }
        var events = record["events"] as? [[String: Any]] ?? []
        if events.last?["phase"] as? String != phase {
            events.append(["phase": phase, "at": Date().timeIntervalSince1970 * 1000, "message": message])
            record["events"] = events
        }
        let terminal = ["result", "error", "canceled", "interrupted"].contains(phase)
        if terminal { record["finishedAt"] = Date().timeIntervalSince1970 * 1000 }
        persist(force: terminal)
        finished = terminal
    }
    private func persist(force: Bool) {
        guard force || Date().timeIntervalSince(lastWrite) >= 1 else { return }
        do {
            let data = try JSONSerialization.data(withJSONObject: record, options: [.sortedKeys])
            try data.write(to: file, options: [.atomic])
            try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file.path)
            lastWrite = Date()
        } catch { self.error = error.localizedDescription }
    }
}
