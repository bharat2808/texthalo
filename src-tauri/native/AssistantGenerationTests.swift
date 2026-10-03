import CoreGraphics
import Foundation
import FoundationModels
import ImageIO

@main struct AssistantGenerationTests {
    @MainActor static func main() async throws {
        let lines = [ScreenText.Line(text: "Left text", bounds: CGRect(x: 0.1, y: 0.6, width: 0.3, height: 0.05)),
                     ScreenText.Line(text: "Right text", bounds: CGRect(x: 0.6, y: 0.7, width: 0.3, height: 0.05))]
        let positioned = ScreenText.reference(from: lines)
        assert(positioned.contains("Left text") && positioned.contains("Right text"))
        let bounded = ScreenText.reference(from: Array(repeating: lines[0], count: 1000), limit: 300)
        assert(bounded.count <= 300 && !bounded.isEmpty)
        assert(ScreenText.reference(from: []).isEmpty)
        let left = (0..<5).map { ScreenText.Line(text: "L\($0)", bounds: CGRect(x: 0.1, y: 0.7 - Double($0) * 0.05, width: 0.3, height: 0.02)) }
        let right = (0..<5).map { ScreenText.Line(text: "R\($0)", bounds: CGRect(x: 0.6, y: 0.8 - Double($0) * 0.05, width: 0.3, height: 0.02)) }
        assert(ScreenText.ordered(Array((right + left).reversed())).map(\.text) == (left + right).map(\.text))
        assert(ScreenText.ordered(Array(left.reversed())).map(\.text) == left.map(\.text))
        var decoder = AssistantMarkerDecoder()
        let partialMarker = try decoder.update(snapshot: "1", selection: "")
        assert(partialMarker == nil)
        let completedMarker = try decoder.update(snapshot: "1|", selection: "")
        assert(completedMarker == nil)
        let firstText = try decoder.update(snapshot: "1|\nHello", selection: "")
        assert(firstText == AssistantReply(action: .speak, text: "Hello"))
        let completedText = try decoder.update(snapshot: "1|\nHello world.", selection: "")
        assert(completedText == AssistantReply(action: .speak, text: "Hello world."))
        let duplicate = try decoder.update(snapshot: "1|\nHello world.", selection: "")
        assert(duplicate == nil)
        let final = try decoder.finish(snapshot: "1|\nHello world.", selection: "")
        assert(final == AssistantReply(action: .speak, text: "Hello world."))

        for action in [AssistantAction.text, .speak, .clarify] {
            var stream = AssistantMarkerDecoder()
            let response = action.marker + "\r\nVisible answer"
            let decoded = try stream.finish(snapshot: response, selection: "")
            assert(decoded == AssistantReply(action: action, text: "Visible answer"), "\(action): \(decoded)")
            assert(!decoded.text.contains(action.marker))
        }

        var selected = AssistantMarkerDecoder()
        let selectedSnapshot = try selected.update(snapshot: "2|", selection: "Exact selected text")
        assert(selectedSnapshot == AssistantReply(action: .readSelection, text: "Exact selected text"))
        let selectedFinal = try selected.finish(snapshot: "2|", selection: "Exact selected text")
        assert(selectedFinal.text == "Exact selected text")

        do {
            var invalid = AssistantMarkerDecoder()
            _ = try invalid.update(snapshot: "Here is an unmarked answer", selection: "")
            assertionFailure("Unmarked model output must fail instead of being routed by the app")
        } catch AssistantProtocolError.invalidMarker(_) { }
        do {
            var missing = AssistantMarkerDecoder()
            _ = try missing.finish(snapshot: "2|", selection: "")
            assertionFailure("Reading without a selection must fail clearly")
        } catch AssistantProtocolError.missingSelection { }
        do {
            var empty = AssistantMarkerDecoder()
            _ = try empty.finish(snapshot: "0|\n", selection: "")
            assertionFailure("An empty generated answer must fail without a second generation")
        } catch AssistantProtocolError.emptyResponse { }

        let instructions = AssistantPrompt.systemInstructions(preference: "Be concise", hasSelection: true, hasImage: false)
        for action in AssistantAction.allCases { assert(instructions.contains(action.marker)) }
        assert(instructions.contains("first two characters"))
        assert(instructions.contains("Selected text is available"))
        assert(instructions.contains("No screenshot is attached"))
        assert(!instructions.contains("Reading/transcription instructions"),
               "Image transcription rules must not apply to a text-only request")
        print("Single-stream Assistant marker contracts passed")

        if ProcessInfo.processInfo.environment["TEXTHALO_TEST_LOCAL_MODEL"] == "1" {
            guard #available(macOS 26.0, *), SystemLanguageModel.default.isAvailable else {
                throw NSError(domain: "Model unavailable", code: 1)
            }
            var snapshots: [AssistantReply] = []
            print("MODEL CASE: written greeting"); fflush(stdout)
            let reply = try await generateAssistantReply(
                request: "Write a friendly one-sentence greeting.",
                preference: "Keep it brief.",
                reference: AssistantPrompt.referenceContext(selectedText: "", source: nil),
                previous: "",
                selection: "",
                onSnapshot: { snapshots.append($0) })
            assert(reply.action == .text && !reply.text.isEmpty)
            assert(!snapshots.isEmpty && snapshots.last == reply)
            assert(snapshots.allSatisfy { !$0.text.hasPrefix("0|") && !$0.text.hasPrefix("1|") })
            print("LOCAL MODEL: \(reply.text)")
            print("MODEL CASE: spoken greeting"); fflush(stdout)
            let spoken = try await generateAssistantReply(
                request: "Say a friendly one-sentence greeting aloud.", preference: "",
                reference: "", previous: "", selection: "")
            assert(spoken.action == .speak && !spoken.text.isEmpty,
                   "An audible generated answer must use the speak marker: \(spoken)")
            print("MODEL CASE: exact selection"); fflush(stdout)
            let exactSelection = "This exact sentence should be spoken."
            let read = try await generateAssistantReply(
                request: "Read the selected text aloud exactly as written.", preference: "",
                reference: AssistantPrompt.referenceContext(selectedText: exactSelection),
                previous: "", selection: exactSelection)
            assert(read == AssistantReply(action: .readSelection, text: exactSelection),
                   "Exact selected-text reading must use the read-selection marker: \(read)")
            print("MODEL CASE: written audio script"); fflush(stdout)
            let script = try await generateAssistantReply(
                request: "Write a short audio script about a sunrise. Do not play it.", preference: "",
                reference: "", previous: "", selection: "")
            assert(script.action == .text && !script.text.isEmpty,
                   "A written audio script must remain text: \(script)")

            #if FOUNDATION_MODELS_IMAGE_ATTACHMENTS
            if #available(macOS 27.0, *) {
                let context = CGContext(data: nil, width: 64, height: 64, bitsPerComponent: 8,
                                        bytesPerRow: 64 * 4, space: CGColorSpaceCreateDeviceRGB(),
                                        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
                context.setFillColor(CGColor(red: 0.95, green: 0.05, blue: 0.05, alpha: 1))
                context.fill(CGRect(x: 0, y: 0, width: 64, height: 64))
                var poemImage = context.makeImage()!
                if let path = ProcessInfo.processInfo.environment["TEXTHALO_TEST_CONTEXT_IMAGE"] {
                    let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil)!
                    poemImage = CGImageSourceCreateImageAtIndex(source, 0, nil)!
                }
                for selection in ["", " I cannot generate a poem."] {
                    print("MODEL CASE: original poem, selected refusal: \(!selection.isEmpty)"); fflush(stdout)
                    let poem = try await generateAssistantReply(
                        request: selection.isEmpty ? "Can you write me a poem here please" : "Can you generate me a poem", preference: "",
                        reference: AssistantPrompt.referenceContext(selectedText: selection),
                        previous: "", selection: selection, screenImage: poemImage)
                    print("LOCAL POEM: \(poem.text)"); fflush(stdout)
                    assert(poem.action == .text && poem.text.split(separator: "\n").count >= 3,
                           "Original writing must produce a poem despite unrelated screen content or a selected refusal: \(poem)")
                }
                print("MODEL CASE: image attachment"); fflush(stdout)
                var visualSnapshots: [AssistantReply] = []
                let visual = try await generateAssistantReply(
                    request: "Describe the dominant color in the attached screenshot in three sentences.",
                    preference: "", reference: "", previous: "", selection: "",
                    screenImage: context.makeImage()!,
                    onSnapshot: { visualSnapshots.append($0) })
                assert(visual.action == .text && visual.text.lowercased().contains("red"),
                       "The native image attachment must reach the local model: \(visual)")
                print("LOCAL IMAGE MODEL: \(visual.text)")
                assert(visualSnapshots.count > 1 && visualSnapshots.last == visual,
                       "Image generation must publish intermediate marker-free snapshots")
            }
            #endif
        }
    }
}
