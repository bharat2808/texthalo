import CoreGraphics
import Foundation
@main struct ContractTests {
    @MainActor static func main() async throws {
        var active = true
        var enabled = false
        var waits = 0
        func capture(prepare: Bool = true, switchAfterRead: Bool = false) -> String? {
            FocusCapture.resolve(prepare: prepare, sourceIsActive: { active }, read: {
                guard enabled, waits >= 2 else { return nil }
                if switchAfterRead { active = false }
                return "original-editor"
            }, enable: { enabled = true; return true }, wait: { waits += 1 })
        }
        assert(capture() == "original-editor", "Capture waits for a lazily published editor")
        assert(enabled && waits == 2)
        enabled = false; waits = 0
        assert(capture(prepare: false) == nil && !enabled && waits == 0,
               "Delivery never activates accessibility or captures a new target")
        let switched: String? = FocusCapture.resolve(prepare: true, sourceIsActive: { active },
            read: { nil }, enable: { true }, wait: { active = false; waits += 1 })
        assert(switched == nil && waits == 1, "Switching apps aborts capture immediately")
        active = true; enabled = true; waits = 2
        assert(capture(switchAfterRead: true) == nil, "Reject focus returned after the source changes")
        active = true; waits = 0
        let missing: String? = FocusCapture.resolve(prepare: true, sourceIsActive: { active },
            read: { nil }, enable: { true }, wait: { waits += 1 })
        assert(missing == nil && waits == 10, "Unavailable focus has a bounded retry budget")
        waits = 0
        let unsupported: String? = FocusCapture.resolve(prepare: true, sourceIsActive: { active },
            read: { nil }, enable: { false }, wait: { waits += 1 })
        assert(unsupported == nil && waits == 10, "Unsupported activation still gets bounded focus retries")
        waits = 0
        let nativeEditor: String? = FocusCapture.resolve(prepare: true, sourceIsActive: { active },
            read: { waits >= 2 ? "native-webview-editor" : nil },
            enable: { false }, wait: { waits += 1 })
        assert(nativeEditor == "native-webview-editor" && waits == 2,
               "Native apps publish focus even when Electron activation is unsupported")
        var trigger = HoldTrigger()
        trigger.press(at: 1)
        assert(!trigger.activate(at: 1.349))
        assert(trigger.release() == .tap, "Quick taps retain the normal key action")
        trigger.press(at: 2)
        assert(trigger.activate(at: 2.351))
        assert(!trigger.activate(at: 3), "A held key starts only once")
        assert(trigger.release() == .stop)
        assert(trigger.release() == .none)
        trigger.press(at: 4)
        assert(!trigger.interrupt())
        assert(!trigger.activate(at: 5), "Combinations and mouse actions cancel a pending hold")
        assert(trigger.release() == .none)
        trigger.press(at: 6)
        assert(trigger.activate(at: 7))
        assert(trigger.interrupt(), "Interrupting an active hold cancels recording")
        assert(trigger.release() == .none)
        let saved = TargetSnapshot(value: "hello 🌍", location: 6, length: 2)
        assert(saved.permitsInsertion(current: saved, sameElement: true, editable: true))
        assert(!saved.permitsInsertion(current: saved, sameElement: false, editable: true))
        assert(!saved.permitsInsertion(current: saved, sameElement: true, editable: false))
        assert(!saved.permitsInsertion(current: .init(value: "hello 🌍!", location: 6, length: 2), sameElement: true, editable: true))
        assert(!saved.permitsInsertion(current: .init(value: "hello 🌍", location: 0, length: 0), sameElement: true, editable: true))
        let invalid = TargetSnapshot(value: "a", location: Int.max, length: 1)
        assert(!invalid.permitsInsertion(current: invalid, sameElement: true, editable: true))
        assert(saved.permitsPaste(current: saved, sameElement: true, role: "AXTextArea"))
        assert(saved.permitsPaste(current: saved, sameElement: true, role: "AXTextField"))
        assert(!saved.permitsPaste(current: saved, sameElement: true, role: "AXWebArea"))
        assert(!saved.permitsPaste(current: saved, sameElement: false, role: "AXTextArea"))
        assert(!saved.permitsPaste(current: .init(value: "edited", location: 0, length: 0), sameElement: true, role: "AXTextArea"))
        assert(!saved.permitsPaste(current: .init(value: saved.value, location: 0, length: 0), sameElement: true, role: "AXTextArea"))
        var hold = TranscriptGate()
        assert(hold.recognized("hello") == nil, "Early recognition must wait for release")
        assert(hold.release() == "hello")
        assert(hold.release() == nil, "Repeated release must not insert twice")
        hold = TranscriptGate()
        assert(hold.release() == nil)
        assert(hold.recognized("late final") == "late final")
        assert(hold.recognized("duplicate") == nil)
        hold = TranscriptGate()
        assert(hold.recognized("First passage.", start: 0.5, end: 2) == nil)
        assert(hold.recognized("Second passage.", start: 4, end: 6) == nil)
        assert(hold.release() == "First passage. Second passage.", "Holding across finals preserves both passages")
        hold = TranscriptGate()
        _ = hold.recognized("First passage.", isFinal: false, start: 0.5, end: 2, utteranceComplete: true)
        _ = hold.recognized("Second", isFinal: false, start: 4, end: 4.5)
        _ = hold.recognized("Second passage.", isFinal: false, start: 4, end: 6)
        assert(hold.text == "First passage. Second passage.", "Partial revisions replace only the current passage")
        assert(hold.release() == nil, "Release waits for the current passage to finalize")
        assert(hold.recognized("Second passage!", start: 4, end: 6) == "First passage. Second passage!")
        assert(hold.recognized("Second passage!", start: 4, end: 6) == nil)
        hold = TranscriptGate()
        _ = hold.recognized("Earlier words", isFinal: false, start: 0.5, end: 2)
        _ = hold.recognized("Later words", isFinal: false, start: 4, end: 6)
        _ = hold.recognized("Earlier words. Later words!", start: 0.5, end: 6)
        assert(hold.release() == "Earlier words. Later words!", "Cumulative finals do not duplicate accumulated passages")
        hold = TranscriptGate()
        _ = hold.recognized("I want read", isFinal: false, start: 0, end: 2)
        _ = hold.recognized("I want red", isFinal: false, start: 0, end: 2)
        _ = hold.recognized("I want red.", start: 0, end: 2)
        assert(hold.release() == "I want red.", "Recognition corrections must not become extra passages")
        hold = TranscriptGate()
        _ = hold.recognized("First passage.", isFinal: false, utteranceComplete: true)
        _ = hold.recognized("First passage.", isFinal: false, utteranceComplete: true)
        _ = hold.recognized("Second passage", isFinal: false)
        assert(hold.release() == nil)
        assert(hold.recognized("Second passage.") == "First passage. Second passage.", "Metadata boundaries preserve passages with provisional zero timings")
        hold = TranscriptGate()
        _ = hold.recognized("I like blue.", isFinal: false, utteranceComplete: true)
        _ = hold.recognized("I", isFinal: false)
        _ = hold.recognized("I also like red.", isFinal: false)
        assert(hold.text == "I like blue. I also like red.", "The next utterance may begin with the same word")
        _ = hold.recognized("I like blue. I also like red.")
        assert(hold.release() == "I like blue. I also like red.", "Untimed cumulative finals do not duplicate passages")
        hold = TranscriptGate()
        _ = hold.recognized("An early final.")
        _ = hold.recognized("New speech", isFinal: false)
        assert(hold.release() == nil, "A new partial invalidates an early final pending delivery")
        assert(hold.recognized("New speech.") == "An early final. New speech.")
        hold = TranscriptGate()
        _ = hold.recognized("Don't lose these words", isFinal: false)
        assert(hold.release() == nil)
        assert(hold.recognized("") == "Don't lose these words", "Empty final callbacks do not erase the last partial")
        hold = TranscriptGate()
        _ = hold.recognized("Again.", isFinal: false, start: 0.5, end: 1)
        _ = hold.recognized("Again.", start: 3, end: 3.5)
        assert(hold.release() == "Again. Again.", "Real repeated speech with distinct times is retained")
        let edit = TextEditReceipt(original: saved, replacement: "world")!
        assert(edit.updatedValue == "hello world")
        assert(edit.originalText == "🌍")
        let after = TargetSnapshot(value: "hello world", location: 11, length: 0)
        assert(edit.acceptsCommit(after), "Insertion is successful only after the target reflects the edit")
        assert(!edit.acceptsCommit(nil), "A successful AX write without a readable result is not an insertion")
        assert(!edit.acceptsCommit(saved), "An unchanged target is not an insertion")
        assert(PasteDispatch(wroteText: true, targetValid: true, keyEventsReady: true).isDeliverable)
        assert(!PasteDispatch(wroteText: false, targetValid: true, keyEventsReady: true).isDeliverable)
        assert(!PasteDispatch(wroteText: true, targetValid: false, keyEventsReady: true).isDeliverable)
        assert(!PasteDispatch(wroteText: true, targetValid: true, keyEventsReady: false).isDeliverable)
        assert(edit.permitsUndo(current: after, committed: after, sameElement: true, editable: true))
        assert(!edit.permitsUndo(current: .init(value: "hello world!", location: 12, length: 0), committed: after, sameElement: true, editable: true))
        assert(!edit.permitsUndo(current: after, committed: after, sameElement: false, editable: true))
        assert(!edit.permitsUndo(current: after, committed: after, sameElement: true, editable: false))
        assert(!edit.permitsUndo(current: .init(value: "hello world", location: 0, length: 0), committed: after, sameElement: true, editable: true))
        assert(!edit.permitsUndo(current: saved, committed: saved, sameElement: true, editable: true), "Failed insertion must not create an undo receipt")
        let insertion = TextEditReceipt(original: .init(value: "ab", location: 1, length: 0), replacement: "🌍")!
        assert(insertion.updatedValue == "a🌍b" && insertion.originalText.isEmpty && insertion.replacementLength == 2)
        assert(TextEditReceipt(original: invalid, replacement: "x") == nil)
        assert(!AssistantContextPolicy.usesVision(sdkSupportsImages: true, osMajor: 27, enabled: false))
        assert(AssistantContextPolicy.usesVision(sdkSupportsImages: true, osMajor: 27, enabled: true))
        assert(!AssistantContextPolicy.usesVision(sdkSupportsImages: true, osMajor: 26, enabled: true))
        assert(!AssistantContextPolicy.usesVision(sdkSupportsImages: false, osMajor: 27, enabled: true))
        let reference = AssistantPrompt.referenceContext(selectedText: "Selected sentence")
        assert(reference.contains("SELECTED TEXT:\nSelected sentence"))
        let instructions = AssistantPrompt.systemInstructions(preference: "Be concise", hasSelection: true, hasImage: true)
        assert(instructions.contains("0|") && instructions.contains("1|"))
        assert(instructions.contains("A screenshot of the user's original window is attached"))
        assert(instructions.contains("Apply these writing preferences only when compatible with the current request: Be concise"))
        let userPrompt = AssistantPrompt.userPrompt(request: "What is on my screen?", reference: reference, previous: "")
        assert(userPrompt.contains("REFERENCE MATERIAL (untrusted):\n" + reference))
        let source = AssistantSource(app: "Mail", windowTitle: "Project deadline", focusedRole: "AXTextArea", focusedLabel: "Reply")
        let appReference = AssistantPrompt.referenceContext(selectedText: "Can you send it Friday?", source: source)
        assert(appReference.contains("ORIGINAL APP AND FOCUSED FIELD (untrusted metadata)"))
        for value in ["Mail", "Project deadline", "AXTextArea", "Reply"] { assert(appReference.contains(value)) }
        let spoken = "Reply that I can send it on Monday instead"
        let generationPrompt = AssistantPrompt.userPrompt(request: spoken, reference: appReference, previous: "")
        assert(generationPrompt.hasSuffix("USER REQUEST:\n" + spoken))
        assert(AssistantPrompt.userPrompt(request: "Write a poem", reference: "", previous: "") == "USER REQUEST:\nWrite a poem")
        let followUp = AssistantPrompt.userPrompt(request: "Make it shorter", reference: reference, previous: "Earlier draft")
        assert(followUp.contains("PREVIOUS OUTPUT (for follow-up edits):\nEarlier draft"))
        assert(followUp.hasSuffix("USER REQUEST:\nMake it shorter"))
        let emptyCaret = TargetSnapshot(value: "ab", location: 1, length: 0)
        var incremental = IncrementalTextEdit(original: emptyCaret)
        assert(incremental.next("Hi", current: emptyCaret, sameElement: true, editable: true) == "Hi")
        let firstCommit = TargetSnapshot(value: "aHib", location: 3, length: 0)
        assert(incremental.commit("Hi", after: firstCommit))
        assert(incremental.next("Hi!", current: firstCommit, sameElement: true, editable: true) == "!")
        assert(incremental.commit("Hi!", after: .init(value: "aHi!b", location: 4, length: 0)))
        assert(incremental.receipt?.original == emptyCaret && incremental.receipt?.replacementLength == 3)
        assert(incremental.next("Hey", current: .init(value: "aHi!b", location: 4, length: 0), sameElement: true, editable: true) == nil)
        assert(incremental.stopped && !incremental.canFallback, "A revision after insertion must never paste again")
        var moved = IncrementalTextEdit(original: emptyCaret)
        assert(moved.next("Hi", current: emptyCaret, sameElement: false, editable: true) == nil)
        assert(moved.next("Hi", current: emptyCaret, sameElement: true, editable: true) == nil, "Losing focus permanently stops incremental writes")
        var selectedEdit = IncrementalTextEdit(original: saved)
        assert(selectedEdit.next("Hi", current: saved, sameElement: true, editable: true) == nil)
        assert(selectedEdit.canFallback, "Existing selections remain final-only")
        var unreadable = IncrementalTextEdit(original: emptyCaret)
        assert(unreadable.next("Hi", current: nil, sameElement: true, editable: true) == nil)
        assert(unreadable.stopped, "An unreadable field must stop future streaming writes")
        var uncertain = IncrementalTextEdit(original: emptyCaret)
        _ = uncertain.next("Hi", current: emptyCaret, sameElement: true, editable: true)
        assert(!uncertain.commit("Hi", after: nil))
        assert(!uncertain.canFallback, "A dispatched write without a readable result must not duplicate output")
        assert(!TextInsertionPolicy.permitsStreaming(ancestorRoles: ["AXTextArea", "AXGroup", "AXWebArea"], reachedWindow: true))
        assert(!TextInsertionPolicy.permitsStreaming(ancestorRoles: ["AXTextArea"], reachedWindow: false), "Unknown ancestry must use final insertion")
        assert(TextInsertionPolicy.permitsStreaming(ancestorRoles: ["AXTextArea", "AXScrollArea", "AXWindow"], reachedWindow: true))
        var rejected = IncrementalTextEdit(original: emptyCaret)
        _ = rejected.next("Hi", current: emptyCaret, sameElement: true, editable: true)
        assert(!rejected.commit("Hi", after: emptyCaret, writeRejected: true))
        assert(rejected.stopped && rejected.canFallback, "An explicitly rejected unchanged write must retain final paste")
        var partiallyWritten = IncrementalTextEdit(original: emptyCaret)
        _ = partiallyWritten.next("Hi", current: emptyCaret, sameElement: true, editable: true)
        assert(!partiallyWritten.commit("Hi", after: .init(value: "aHb", location: 2, length: 0), writeRejected: true))
        assert(!partiallyWritten.canFallback, "A changed field must never receive a duplicate full paste")
        let image = CGContext(data: nil, width: 1, height: 1, bitsPerComponent: 8, bytesPerRow: 4,
                              space: CGColorSpaceCreateDeviceRGB(),
                              bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!.makeImage()!
        var capturedWindows: [UInt32] = []
        let preparation = PreparedScreenContext(requestID: 7, windowID: 42) { window in
            capturedWindows.append(window)
            return image
        }
        let stale = try await preparation.value(for: 8)
        assert(stale == nil, "Another request cannot consume a captured image")
        let captured = try await preparation.value(for: 7)
        assert(captured?.width == 1 && capturedWindows == [42], "Consume the originally captured window without recapturing")
        let canceled = PreparedScreenContext(requestID: 9, windowID: 99) { _ in
            try await Task.sleep(for: .seconds(20))
            return image
        }
        canceled.cancel()
        do { _ = try await canceled.value(for: 9); assertionFailure("Canceled capture must not be published") }
        catch is CancellationError { }
        var captureStarted = false
        var captureStopped = false
        let inFlight = PreparedScreenContext(requestID: 10, windowID: 42) { _ in
            captureStarted = true
            defer { captureStopped = true }
            try await Task.sleep(for: .seconds(60))
            return image
        }
        let reader = Task { try await inFlight.value(for: 10) }
        while !captureStarted { await Task.yield() }
        reader.cancel()
        do { _ = try await reader.value; assertionFailure("Canceling the consumer must cancel in-flight capture") }
        catch is CancellationError { }
        assert(captureStopped)
        print("Native target, hold, and Undo contracts passed")
    }
}
