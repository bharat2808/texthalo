import Foundation
import FoundationModels

@main struct AssistantGenerationTests {
    @MainActor static func main() async throws {
        guard #available(macOS 26.0, *) else { return }
        let encoder = JSONEncoder()
        let screenSchema = String(data: try encoder.encode(ScreenAnswer.generationSchema), encoding: .utf8)!
        let standaloneSchema = String(data: try encoder.encode(StandaloneAnswer.generationSchema), encoding: .utf8)!
        let selectionSchema = String(data: try encoder.encode(AssistantReply.generationSchema), encoding: .utf8)!
        let deliverySchema = String(data: try encoder.encode(AssistantAudibleDecision.generationSchema), encoding: .utf8)!
        let referenceSchema = String(data: try encoder.encode(AssistantReferenceDecision.generationSchema), encoding: .utf8)!
        assert(!screenSchema.contains("readSelection"), "Screen-only questions must not produce a readSelection action")
        assert(!standaloneSchema.contains("readSelection") && !standaloneSchema.contains("relevantContext"), "Standalone requests must not be forced through a reference schema")
        assert(selectionSchema.contains("readSelection"), "Explicit selection reading remains supported")
        assert(deliverySchema.contains("shouldProduceAudibleSpeech") && referenceSchema.contains("referenceUse"))
        assert(screenSchema.contains("useScreenTextVerbatim"), "The screen generation contract can request exact source reading")
        assert(!standaloneSchema.contains("useScreenTextVerbatim"))
        assert(explicitAssistantCommand(for: "Read the book aloud to me") == .readSourceAloud)
        assert(explicitAssistantCommand(for: "Transcribe the screen text exactly") == .returnSourceVerbatim)
        assert(explicitAssistantCommand(for: "Summarize the book aloud") == .generateSpokenAnswer)
        assert(explicitAssistantCommand(for: "Generate audio describing my screen") == .generateSpokenAnswer)
        assert(explicitAssistantCommand(for: "Write an audio script") == .generateStandaloneText)
        assert(explicitAssistantCommand(for: "Write an email to my manager requesting two days of leave") == .generateStandaloneText)
        assert(explicitAssistantCommand(for: "Write an email based on this document") == nil)
        assert(explicitAssistantCommand(for: "Do not read it aloud; just write a summary") == .generateWrittenAnswer)
        for action in ["preview", "speak", "clarify"] { assert(screenSchema.contains(action)) }
        let written = AssistantReply(action: .preview, text: "Here is the screen summary.")
        assert(routeAssistantReply(written, delivery: .audio, selection: "").action == .speak)
        assert(routeAssistantReply(AssistantReply(action: .speak, text: "A written audio script"), delivery: .text, selection: "").action == .preview)
        let read = routeAssistantReply(AssistantReply(action: .readSelection, text: ""), delivery: .text, selection: "Exact text")
        assert(read.action == .preview && read.text == "Exact text")
        assert(routeAssistantReply(AssistantReply(action: .clarify, text: "Which passage?"), delivery: .audio, selection: "").action == .clarify)
        var streamed: [AssistantReply] = []
        var stream = AssistantSnapshotRouter(delivery: .audio, selection: "Exact selection")
        assert(stream.update(action: nil, text: "hidden") == nil)
        if let value = stream.update(action: .preview, text: "Hello") { streamed.append(value) }
        if let value = stream.update(action: .preview, text: "Hello world") { streamed.append(value) }
        assert(stream.update(action: .preview, text: "Hello world") == nil)
        assert(streamed.count == 2 && streamed[0].action == .speak && streamed[1].text == "Hello world")
        assert(stream.update(action: .preview, text: "Hello there")?.text == "Hello there", "Uncommitted model revisions reach the preview")
        var clarification = AssistantSnapshotRouter(delivery: .audio, selection: "")
        assert(clarification.update(action: .clarify, text: "Which passage?")?.action == .clarify)
        var selected = AssistantSnapshotRouter(delivery: .audio, selection: "Exact selection")
        assert(selected.update(action: .readSelection, text: "invented partial") == nil, "Exact selection is dispatched only at EOF")
        print("Generation action schemas passed")
        // Opt-in behavioral checks use Apple's real local model and synthetic
        // screen text. Ordinary contract tests don't require Apple Intelligence.
        if ProcessInfo.processInfo.environment["TEXTHALO_TEST_LOCAL_MODEL"] == "1" {
            guard SystemLanguageModel.default.isAvailable else { throw NSError(domain: "Model unavailable", code: 1) }
            let bookPage = """
                Chapter One
                It was a bright cold day in April, and the clocks were striking thirteen.
                Winston Smith slipped quickly through the glass doors of Victory Mansions.
                """
            let bookReading = try await generateAssistantReply(
                request: "Read the book aloud to me.", preference: "",
                reference: AssistantPrompt.referenceContext(selectedText: "", screenOCR: bookPage),
                previous: "", selection: "", screenText: bookPage)
            assert(bookReading.action == .speak && bookReading.text == bookPage,
                   "A book reading must use the captured words without a generated preamble")
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
            let screenPlan = try await assistantPlan(for: "What do you see on my screen?", hasSelection: false, hasScreen: true)
            print("SCREEN PLAN: \(screenPlan.delivery) / \(screenPlan.referenceUse)"); fflush(stdout)
            var snapshots: [AssistantReply] = []
            let summary = try await generateAssistantReply(request: "What do you see on my screen?", preference: "", reference: reference, previous: "", selection: "", screenText: ocr, onSnapshot: { snapshots.append($0) })
            assert(snapshots.count > 1, "A real model answer must produce live snapshots before EOF")
            assert(snapshots.last?.text == summary.text && snapshots.allSatisfy { $0.action == .preview })
            assert(snapshots.allSatisfy { !$0.text.contains("relevantContext") && !$0.text.contains("New chat\nProjects") }, "Never stream generated structure or incidental OCR")
            print("SCREEN SUMMARY: \(summary.text)"); fflush(stdout)
            assert(summary.action == .preview)
            assert(summary.text.contains("Codex"))
            assert(!summary.text.lowercased().contains("text editor") && !summary.text.lowercased().contains("message button"), "Do not invent UI types from focused-field metadata")
            assert(["code", "assistant", "screen", "development"].contains(where: { summary.text.lowercased().contains($0) }), "Explain the main development conversation")
            assert(summary.text != ocr && !summary.text.contains("New chat\nProjects"))
            assert(summary.text.split(whereSeparator: \.isWhitespace).count < 140)
            let exactText = "Invoice total: $42\nDue Friday"
            let transcription = try await generateAssistantReply(request: "Transcribe the screen text exactly, preserving its words and line breaks.", preference: "", reference: AssistantPrompt.referenceContext(selectedText: "", screenOCR: exactText), previous: "", selection: "", screenText: exactText)
            print("EXPLICIT TRANSCRIPTION: \(transcription.text)"); fflush(stdout)
            assert(transcription.action == .preview)
            assert(transcription.text.trimmingCharacters(in: .whitespacesAndNewlines) == exactText)
            let audio = try await generateAssistantReply(request: "Generate audio of what you see on my screen.", preference: "", reference: reference, previous: "", selection: "", screenText: ocr)
            print("AUDIO ROUTE: \(audio.action) — \(audio.text)"); fflush(stdout)
            assert(audio.action == .speak && !audio.text.isEmpty)
            let unrelatedNotes = AssistantPrompt.referenceContext(selectedText: "", screenOCR: """
                Notes
                Bug #1
                Production
                Personal
                Recently Deleted
                """, source: AssistantSource(app: "Notes", windowTitle: "Notes", focusedRole: "AXTextArea", focusedLabel: "Body"), mode: "screen")
            let leavePlan = try await assistantPlan(for: "Write an email to my manager requesting two days of leave.", hasSelection: false, hasScreen: true)
            if case .none = leavePlan.referenceUse {} else { assertionFailure("A self-contained writing request must not use screen context") }
            let leaveEmail = try await generateAssistantReply(
                request: "Write an email to my manager requesting two days of leave.",
                preference: "", reference: unrelatedNotes, previous: "", selection: "", screenText: "Notes\nBug #1\nProduction\nPersonal\nRecently Deleted")
            print("SELF-CONTAINED EMAIL: \(leaveEmail.text)"); fflush(stdout)
            let emailText = leaveEmail.text.lowercased()
            assert(leaveEmail.action == .preview)
            assert(emailText.contains("leave") && (emailText.contains("manager") || emailText.contains("dear")), "The result must be the requested leave email")
            assert(!emailText.contains("looking at") && !emailText.contains("notes app") && !emailText.contains("bug #1"), "Irrelevant screen OCR must not hijack a self-contained writing request")
            for request in ["Write an audio script about this screen", "Explain how audio generation works", "Do not read it aloud; just write a summary"] {
                let medium = try await assistantDelivery(for: request)
                assert(medium == .text, "A text request must not trigger audio: \(request)")
            }
        }
    }
}
