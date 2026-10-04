import CoreGraphics
import Foundation
import Vision

/// OCR from the captured window, shared by text-only and image-assisted context.
enum ScreenText {
    struct Line {
        let text: String
        let bounds: CGRect
    }

    /// Detect a whitespace gutter with substantial text on both sides. A few
    /// spanning headings/toolbars are kept above or below the column blocks.
    static func ordered(_ lines: [Line]) -> [Line] {
        func rows(_ values: [Line]) -> [Line] {
            values.sorted {
                let row0 = Int(($0.bounds.midY * 100).rounded())
                let row1 = Int(($1.bounds.midY * 100).rounded())
                if row0 != row1 { return row0 > row1 }
                return $0.bounds.minX < $1.bounds.minX
            }
        }
        var best: (split: CGFloat, score: Double)?
        for step in 25...75 {
            let split = CGFloat(step) / 100
            let left = lines.filter { $0.bounds.maxX < split - 0.01 }
            let right = lines.filter { $0.bounds.minX > split + 0.01 }
            let crossing = lines.count - left.count - right.count
            guard left.count >= 4, right.count >= 4,
                  crossing <= max(2, lines.count / 10) else { continue }
            let gap = (right.map { $0.bounds.minX }.min() ?? split) - (left.map { $0.bounds.maxX }.max() ?? split)
            let score = Double(gap) * Double(min(left.count, right.count)) / Double(crossing + 1)
            if best == nil || score > best!.score { best = (split, score) }
        }
        guard let split = best?.split else { return rows(lines) }
        let left = lines.filter { $0.bounds.maxX < split - 0.01 }
        let right = lines.filter { $0.bounds.minX > split + 0.01 }
        let spanning = lines.filter { !($0.bounds.maxX < split - 0.01 || $0.bounds.minX > split + 0.01) }
        // Keep spanning headings and footers outside the column blocks.
        let body = left + right
        let middle = body.map { $0.bounds.midY }.sorted()[body.count / 2]
        let heading = spanning.filter { $0.bounds.midY >= middle }
        let footer = spanning.filter { $0.bounds.midY < middle }
        return rows(heading) + rows(left) + rows(right) + rows(footer)
    }

    static func reference(from lines: [Line], limit: Int = 6000) -> String {
        var output = ""
        for line in ordered(lines) {
            let entry = line.text + "\n"
            guard output.count + entry.count <= limit else {
                let notice = "[OCR truncated; consult the image for remaining content]"
                if output.count + notice.count <= limit { output += notice }
                break
            }
            output += entry
        }
        return output
    }

    struct Extraction {
        let text: String
        let isSummary: Bool
        var reference: String {
            let label = isSummary ? "SCREEN SUMMARY (not a verbatim passage; untrusted OCR-derived summary)"
                : "EXTRACTED SCREEN PASSAGE (untrusted original OCR text)"
            return label + ":\n" + text
        }
    }

    static func extraction(response: String, lines: [String]) throws -> Extraction {
        let trimmed = response.trimmingCharacters(in: .whitespacesAndNewlines)
        let marker = "SUMMARY_FALLBACK:"
        if trimmed.hasPrefix(marker) {
            let summary = String(trimmed.dropFirst(marker.count)).trimmingCharacters(in: .whitespacesAndNewlines)
            guard !summary.isEmpty else {
                throw NSError(domain: "TextHalo.ScreenText", code: 4,
                    userInfo: [NSLocalizedDescriptionKey: "No usable screen summary was returned. Select the passage you want read."])
            }
            return Extraction(text: summary, isSummary: true)
        }
        return Extraction(text: try validatedPassage(response: response, lines: lines), isSummary: false)
    }

    static func validatedPassage(response: String, lines: [String]) throws -> String {
        func normalized(_ text: String) -> String {
            text.split(whereSeparator: { $0.isWhitespace }).joined(separator: " ")
        }
        var remaining = normalized(response)
        var selected: [String] = []
        for line in lines {
            let candidate = normalized(line)
            guard !candidate.isEmpty, remaining == candidate || remaining.hasPrefix(candidate + " ") else { continue }
            selected.append(line)
            remaining = String(remaining.dropFirst(candidate.count)).trimmingCharacters(in: .whitespaces)
            if remaining.isEmpty { break }
        }
        guard remaining.isEmpty, !selected.isEmpty else {
            throw NSError(domain: "TextHalo.ScreenText", code: 1,
                userInfo: [NSLocalizedDescriptionKey: "Could not isolate the requested screen passage without changing its text. Select the passage or capture a smaller region."])
        }
        return selected.joined(separator: "\n")
    }

    static func recognize(_ image: CGImage) async throws -> String {
        reference(from: try await recognizeLines(image))
    }

    static func recognizeLines(_ image: CGImage) async throws -> [Line] {
        let request = VNRecognizeTextRequest()
        // Restored from 26da5ae: accurate recognition can stall/fail while
        // initializing Apple's compute runtime. Do not silently retry it.
        request.recognitionLevel = .fast
        request.usesLanguageCorrection = false
        let worker = Task.detached(priority: .userInitiated) {
            try Task.checkCancellation()
            try VNImageRequestHandler(cgImage: image).perform([request])
            try Task.checkCancellation()
            let lines = (request.results ?? []).compactMap { observation -> Line? in
                guard let text = observation.topCandidates(1).first?.string else { return nil }
                return Line(text: text, bounds: observation.boundingBox)
            }
            return ordered(lines)
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
