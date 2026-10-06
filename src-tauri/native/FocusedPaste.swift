import AppKit
import Carbon

/// Freeflow-style delivery: let the receiving app handle Cmd+V at its active cursor.
/// Accessibility snapshots remain useful for context and Undo, but are not a paste prerequisite.
@MainActor final class FocusedPaste {
    private let pasteboard: NSPasteboard
    private let sleep: (TimeInterval) async throws -> Void
    private let dispatch: () -> Bool
    private let writeText: (NSPasteboard, String) -> Bool
    private let scheduleRestore: (@escaping @MainActor () -> Void) -> Void

    init(pasteboard: NSPasteboard = .general,
         sleep: @escaping (TimeInterval) async throws -> Void = { try await Task.sleep(for: .seconds($0)) },
         dispatch: @escaping () -> Bool = FocusedPaste.postPaste,
         writeText: @escaping (NSPasteboard, String) -> Bool = { $0.setString($1, forType: .string) },
         scheduleRestore: @escaping (@escaping @MainActor () -> Void) -> Void = { restore in
             DispatchQueue.main.asyncAfter(deadline: .now() + 1) { restore() }
         }) {
        self.pasteboard = pasteboard
        self.sleep = sleep
        self.dispatch = dispatch
        self.writeText = writeText
        self.scheduleRestore = scheduleRestore
    }

    func paste(_ text: String, restoreClipboard: Bool, sourceIsActive: () -> Bool,
               shortcutPressed: () -> Bool) async -> Bool {
        guard !text.isEmpty, !Task.isCancelled, sourceIsActive() else { return false }
        do {
            // Same release polling and settling delay as Freeflow. Keep the wait bounded.
            for _ in 0..<24 {
                guard shortcutPressed() else { break }
                try await sleep(0.025)
                guard !Task.isCancelled, sourceIsActive() else { return false }
            }
            guard !shortcutPressed() else { return false }
            try await sleep(0.03)
        } catch { return false }
        guard !Task.isCancelled, sourceIsActive(), !shortcutPressed() else { return false }

        let saved = restoreClipboard ? (pasteboard.pasteboardItems ?? []).map { item in
            item.types.compactMap { type in item.data(forType: type).map { (type, $0) } }
        } : []
        pasteboard.clearContents()
        let wroteText = writeText(pasteboard, text)
        let changeCount = pasteboard.changeCount
        defer {
            if restoreClipboard {
                // Unlike Freeflow's text-equality fallback, a newer clipboard copy always wins.
                scheduleRestore { [pasteboard] in
                    guard pasteboard.changeCount == changeCount else { return }
                    pasteboard.clearContents()
                    let items = saved.map { entries in
                        let item = NSPasteboardItem()
                        for (type, data) in entries { item.setData(data, forType: type) }
                        return item
                    }
                    if !items.isEmpty { pasteboard.writeObjects(items) }
                }
            }
        }
        guard wroteText else { return false }
        return dispatch()
    }

    private static func postPaste() -> Bool {
        guard let source = CGEventSource(stateID: .hidSystemState) else { return false }
        let key = pasteKeyCode()
        guard let down = CGEvent(keyboardEventSource: source, virtualKey: key, keyDown: true),
              let up = CGEvent(keyboardEventSource: source, virtualKey: key, keyDown: false) else { return false }
        down.flags = .maskCommand; up.flags = .maskCommand
        down.post(tap: .cgSessionEventTap); up.post(tap: .cgSessionEventTap)
        return true
    }

    // Use the active keyboard layout, as Freeflow does, rather than assuming US keycode 9.
    private static func pasteKeyCode() -> CGKeyCode {
        let source = TISCopyCurrentKeyboardInputSource().takeRetainedValue()
        guard let raw = TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData) else { return 9 }
        let data = unsafeBitCast(raw, to: CFData.self) as Data
        return data.withUnsafeBytes { bytes in
            guard let layout = bytes.baseAddress?.assumingMemoryBound(to: UCKeyboardLayout.self) else { return 9 }
            for key in UInt16(0)..<UInt16(128) {
                var characters = [UniChar](repeating: 0, count: 4)
                var count = 0
                var deadKeyState: UInt32 = 0
                let status = UCKeyTranslate(layout, key, UInt16(kUCKeyActionDisplay), 0,
                    UInt32(LMGetKbdType()), OptionBits(kUCKeyTranslateNoDeadKeysBit),
                    &deadKeyState, 4, &count, &characters)
                if status == noErr, count > 0, characters[0] == 118 { return CGKeyCode(key) }
            }
            return 9
        }
    }
}
