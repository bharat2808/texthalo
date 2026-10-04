import AppKit

func emit(_ value: [String: Any]) {}

@main struct HoldShortcutTests {
    static func main() {
        func window(_ pid: Int, _ layer: Int) -> [String: Any] {
            [kCGWindowOwnerPID as String: pid, kCGWindowLayer as String: layer,
             kCGWindowAlpha as String: 1, kCGWindowBounds as String: ["X": 0, "Y": 0, "Width": 100, "Height": 100]]
        }
        let point = CGPoint(x: 50, y: 50)
        let widget = window(42, 1000)
        assert(HoldShortcuts.isWidgetInteraction(at: point, windows: [widget], ownerPID: 42))
        assert(!HoldShortcuts.isWidgetInteraction(at: point, windows: [window(42, 0)], ownerPID: 42), "Main app clicks still cancel a hold")
        assert(!HoldShortcuts.isWidgetInteraction(at: point, windows: [window(99, 1000)], ownerPID: 42), "Another app's overlay is not ours")
        assert(!HoldShortcuts.isWidgetInteraction(at: point, windows: [window(99, 2000), widget], ownerPID: 42), "A covering window owns the click")
        assert(!HoldShortcuts.isWidgetInteraction(at: .init(x: 150, y: 50), windows: [widget], ownerPID: 42))
        var events: [[String: Any]] = []
        var replayed: [CGEvent] = []
        var widgetInteraction = false
        let listener = HoldShortcuts(sendEvent: { events.append($0) }, postEvent: { replayed.append($0) }, allowsWidgetInteraction: { _ in widgetInteraction })
        listener.configure(["transcribe": "Hold:Escape", "assistant": "Hold:RightOption"])
        func event(_ type: CGEventType, _ code: CGKeyCode, _ flags: CGEventFlags = []) -> CGEvent {
            let event = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: type != .keyUp)!
            event.type = type; event.flags = flags
            return event
        }
        func wait() { RunLoop.main.run(until: Date().addingTimeInterval(0.4)) }
        // Quick Escape taps are delivered exactly once, in down/up order.
        assert(listener.handle(.keyDown, event(.keyDown, 53)) == nil)
        assert(listener.handle(.keyUp, event(.keyUp, 53)) == nil)
        assert(replayed.map(\.type) == [.keyDown, .keyUp])
        wait(); assert(events.isEmpty)
        // Replayed events bypass the hold listener.
        for replay in replayed { assert(listener.handle(replay.type, replay) != nil) }
        replayed.removeAll()
        // Long Escape starts and finishes once, without dismissing anything.
        assert(listener.handle(.keyDown, event(.keyDown, 53)) == nil)
        wait()
        assert(listener.handle(.keyDown, event(.keyDown, 53)) == nil)
        assert(listener.handle(.keyUp, event(.keyUp, 53)) == nil)
        assert(events.compactMap { $0["phase"] as? String } == ["hold_pressed", "hold_released"])
        assert(replayed.isEmpty)
        events.removeAll()
        // The global key-state query can lag the modifier event at a tap.
        // Press/release must be determined from the event being handled.
        _ = listener.handle(.flagsChanged, event(.flagsChanged, 61, .maskAlternate))
        _ = listener.handle(.scrollWheel, event(.scrollWheel, 0))
        wait()
        _ = listener.handle(.scrollWheel, event(.scrollWheel, 0))
        _ = listener.handle(.flagsChanged, event(.flagsChanged, 61))
        assert(events.compactMap { $0["phase"] as? String } == ["hold_pressed", "hold_released"])
        events.removeAll()
        // Option combinations pass through and never activate a hold.
        assert(listener.handle(.flagsChanged, event(.flagsChanged, 61, .maskAlternate)) != nil)
        assert(listener.handle(.keyDown, event(.keyDown, 0, .maskAlternate)) != nil)
        wait(); assert(events.isEmpty)
        _ = listener.handle(.keyUp, event(.keyUp, 0, .maskAlternate))
        _ = listener.handle(.flagsChanged, event(.flagsChanged, 61))
        // A mouse action cancels an active Option hold.
        _ = listener.handle(.flagsChanged, event(.flagsChanged, 61, .maskAlternate))
        wait()
        _ = listener.handle(.leftMouseDown, event(.leftMouseDown, 0))
        _ = listener.handle(.flagsChanged, event(.flagsChanged, 61))
        assert(events.compactMap { $0["phase"] as? String } == ["hold_pressed", "hold_cancel"])
        events.removeAll()
        // Expanding, selecting text, or dragging our widget must not end a hold.
        widgetInteraction = true
        _ = listener.handle(.flagsChanged, event(.flagsChanged, 61, .maskAlternate))
        wait()
        _ = listener.handle(.leftMouseDown, event(.leftMouseDown, 0))
        _ = listener.handle(.flagsChanged, event(.flagsChanged, 61))
        assert(events.compactMap { $0["phase"] as? String } == ["hold_pressed", "hold_released"])
        events.removeAll()
        widgetInteraction = false
        // Changing a binding while waiting must not start the old action.
        _ = listener.handle(.keyDown, event(.keyDown, 53))
        listener.configure(["assistant": "Hold:Fn"])
        _ = listener.handle(.keyUp, event(.keyUp, 53))
        wait(); assert(events.isEmpty)
        // Fn uses modifier flags rather than ordinary key-down events.
        _ = listener.handle(.flagsChanged, event(.flagsChanged, 63, .maskSecondaryFn))
        wait()
        _ = listener.handle(.flagsChanged, event(.flagsChanged, 63))
        assert(events.compactMap { $0["phase"] as? String } == ["hold_pressed", "hold_released"])
        events.removeAll()
        listener.suspend(true)
        _ = listener.handle(.flagsChanged, event(.flagsChanged, 63, .maskSecondaryFn))
        wait(); assert(events.isEmpty)
    }
}
