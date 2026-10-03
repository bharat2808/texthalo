import Foundation
import FoundationModels

@main struct AssistantGenerationTests {
    static func main() async throws {
        guard #available(macOS 26.0, *) else { return }
        let encoder = JSONEncoder()
        let screenSchema = String(data: try encoder.encode(ScreenAnswer.generationSchema), encoding: .utf8)!
        let selectionSchema = String(data: try encoder.encode(AssistantReply.generationSchema), encoding: .utf8)!
        assert(!screenSchema.contains("readSelection"), "Screen-only questions must not produce a readSelection action")
        assert(selectionSchema.contains("readSelection"), "Explicit selection reading remains supported")
        for action in ["preview", "speak", "clarify"] { assert(screenSchema.contains(action)) }
        let written = AssistantReply(action: .preview, text: "Here is the screen summary.")
        assert(routeAssistantReply(written, delivery: .audio, selection: "").action == .speak)
        assert(routeAssistantReply(AssistantReply(action: .speak, text: "A written audio script"), delivery: .text, selection: "").action == .preview)
        let read = routeAssistantReply(AssistantReply(action: .readSelection, text: ""), delivery: .text, selection: "Exact text")
        assert(read.action == .preview && read.text == "Exact text")
        assert(routeAssistantReply(AssistantReply(action: .clarify, text: "Which passage?"), delivery: .audio, selection: "").action == .clarify)
        print("Generation action schemas passed")
        // Opt-in behavioral checks use Apple's real local model and synthetic
        // screen text. Ordinary contract tests don't require Apple Intelligence.
        if ProcessInfo.processInfo.environment["TEXTHALO_TEST_LOCAL_MODEL"] == "1" {
            guard SystemLanguageModel.default.isAvailable else { throw NSError(domain: "Model unavailable", code: 1) }
            let ocr = """
            Codex
            New chat
            Projects
            Calendar app
            Fix navigation
            TextHalo
            Redesign the user flow
            Ran xcrun swiftc
            Read Contracts.swift
            The response schema now makes read selected text impossible when no text is selected.
            Screen questions can still use the app/window and OCR context.
            Fixed in the running development build.
            Empty model answers get one retry instead of reporting missing context.
            All five native tests pass. No release was built.
            Edited 3 files
            Undo
            Review
            Full access
            """
            let reference = AssistantPrompt.referenceContext(selectedText: "", screenOCR: ocr,
                source: AssistantSource(app: "Codex", windowTitle: "Redesign the user flow", focusedRole: "AXTextArea", focusedLabel: "Message"), mode: "screen")
            let summary = try await generateAssistantReply(request: "What do you see on my screen?", preference: "", reference: reference, previous: "", selection: "")
            print("SCREEN SUMMARY: \(summary.text)"); fflush(stdout)
            assert(summary.action == .preview)
            assert(summary.text.contains("Codex"))
            assert(!summary.text.lowercased().contains("text editor") && !summary.text.lowercased().contains("message button"), "Do not invent UI types from focused-field metadata")
            assert(["code", "assistant", "screen", "development"].contains(where: { summary.text.lowercased().contains($0) }), "Explain the main development conversation")
            assert(summary.text != ocr && !summary.text.contains("New chat\nProjects"))
            assert(summary.text.split(whereSeparator: \.isWhitespace).count < 140)
            let exactText = "Invoice total: $42\nDue Friday"
            let transcription = try await generateAssistantReply(request: "Transcribe the screen text exactly, preserving its words and line breaks.", preference: "", reference: AssistantPrompt.referenceContext(selectedText: "", screenOCR: exactText), previous: "", selection: "")
            print("EXPLICIT TRANSCRIPTION: \(transcription.text)"); fflush(stdout)
            assert(transcription.action == .preview)
            assert(transcription.text.trimmingCharacters(in: .whitespacesAndNewlines) == exactText)
            let audio = try await generateAssistantReply(request: "Generate audio of what you see on my screen.", preference: "", reference: reference, previous: "", selection: "")
            print("AUDIO ROUTE: \(audio.action) — \(audio.text)"); fflush(stdout)
            assert(audio.action == .speak && !audio.text.isEmpty)
            for request in ["Write an audio script about this screen", "Explain how audio generation works", "Do not read it aloud; just write a summary"] {
                let medium = try await assistantDelivery(for: request)
                assert(medium == .text, "A text request must not trigger audio: \(request)")
            }
        }
    }
}
