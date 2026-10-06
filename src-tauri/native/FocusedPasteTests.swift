import AppKit
import Foundation

@main struct FocusedPasteTests {
    @MainActor static func main() async throws {
        let board = NSPasteboard(name: .init("TextHaloFocusedPasteTests-\(UUID().uuidString)"))
        defer { board.releaseGlobally() }
        board.setString("Synthetic original", forType: .string)
        let rich = Data([1, 2, 3, 4])
        board.setData(rich, forType: .rtf)
        var dispatched = 0
        var waits: [TimeInterval] = []
        var pressed = true
        var active = true
        let delivery = FocusedPaste(pasteboard: board, sleep: { interval in
            waits.append(interval)
            pressed = false
        }, dispatch: {
            assert(board.string(forType: .string) == "Synthetic transcript")
            dispatched += 1
            return true
        }, scheduleRestore: { $0() })
        // Delivery needs an active source app, not AXValue or AXSelectedTextRange.
        let delivered = await delivery.paste("Synthetic transcript", restoreClipboard: true,
            sourceIsActive: { active }, shortcutPressed: { pressed })
        assert(delivered)
        assert(dispatched == 1 && waits.contains(0.025) && waits.contains(0.03))
        assert(board.string(forType: .string) == "Synthetic original")

        assert(board.data(forType: .rtf) == rich)

        active = false
        let inactive = await delivery.paste("Synthetic transcript", restoreClipboard: true,
            sourceIsActive: { active }, shortcutPressed: { false })
        assert(!inactive)
        assert(dispatched == 1 && board.string(forType: .string) == "Synthetic original")

        active = true
        let switched = FocusedPaste(pasteboard: board, sleep: { _ in active = false }, dispatch: { fatalError("Pasted after switching apps") })
        let switchedResult = await switched.paste("Synthetic transcript", restoreClipboard: true,
            sourceIsActive: { active }, shortcutPressed: { false })
        assert(!switchedResult)
        assert(board.string(forType: .string) == "Synthetic original")

        active = true
        var heldWaits = 0
        let held = FocusedPaste(pasteboard: board, sleep: { _ in heldWaits += 1 }, dispatch: { fatalError("Pasted with shortcut held") })
        let heldResult = await held.paste("Synthetic transcript", restoreClipboard: true,
            sourceIsActive: { true }, shortcutPressed: { true })
        assert(!heldResult)
        assert(heldWaits == 24 && board.string(forType: .string) == "Synthetic original")

        let copied = FocusedPaste(pasteboard: board, sleep: { _ in }, dispatch: { true }, scheduleRestore: { restore in
            board.clearContents(); board.setString("Synthetic newer copy", forType: .string)
            restore()
        })
        let copiedResult = await copied.paste("Synthetic transcript", restoreClipboard: true,
            sourceIsActive: { true }, shortcutPressed: { false })
        assert(copiedResult)
        assert(board.string(forType: .string) == "Synthetic newer copy")

        let canceled = FocusedPaste(pasteboard: board, sleep: { _ in throw CancellationError() }, dispatch: { fatalError("Pasted after cancellation") })
        let canceledResult = await canceled.paste("Synthetic transcript", restoreClipboard: true,
            sourceIsActive: { true }, shortcutPressed: { false })
        assert(!canceledResult)
        assert(board.string(forType: .string) == "Synthetic newer copy")
        let kept = FocusedPaste(pasteboard: board, sleep: { _ in }, dispatch: { true },
            scheduleRestore: { _ in fatalError("Restored despite disabled restoration") })
        let keptResult = await kept.paste("Synthetic kept transcript", restoreClipboard: false,
            sourceIsActive: { true }, shortcutPressed: { false })
        assert(keptResult && board.string(forType: .string) == "Synthetic kept transcript")

        let rejected = FocusedPaste(pasteboard: board, sleep: { _ in }, dispatch: { false }, scheduleRestore: { $0() })
        let rejectedResult = await rejected.paste("Synthetic failed paste", restoreClipboard: true,
            sourceIsActive: { true }, shortcutPressed: { false })
        assert(!rejectedResult && board.string(forType: .string) == "Synthetic kept transcript")
        let writeFailed = FocusedPaste(pasteboard: board, sleep: { _ in },
            dispatch: { fatalError("Dispatched paste after clipboard write failure") },
            writeText: { _, _ in false }, scheduleRestore: { $0() })
        let writeFailedResult = await writeFailed.paste("Synthetic failed write", restoreClipboard: true,
            sourceIsActive: { true }, shortcutPressed: { false })
        assert(!writeFailedResult && board.string(forType: .string) == "Synthetic kept transcript")
        print("Focused paste contracts passed")
    }
}
