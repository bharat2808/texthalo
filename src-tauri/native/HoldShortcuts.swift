import AppKit
import ApplicationServices
import IOKit.hidsystem

// Owned by the main run loop, including the event-tap callback and timer.
final class HoldShortcuts {
    private let sendEvent: ([String: Any]) -> Void
    private let postEvent: (CGEvent) -> Void
    private let allowsWidgetInteraction: (CGEvent) -> Bool
    init(sendEvent: @escaping ([String: Any]) -> Void = emit,
         postEvent: @escaping (CGEvent) -> Void = { $0.post(tap: .cgSessionEventTap) },
         allowsWidgetInteraction: @escaping (CGEvent) -> Bool = { HoldShortcuts.isWidgetInteraction(at: $0.location) }) {
        self.sendEvent = sendEvent; self.postEvent = postEvent
        self.allowsWidgetInteraction = allowsWidgetInteraction
    }
    static func isWidgetInteraction(at point: CGPoint) -> Bool {
        guard let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] else { return false }
        return isWidgetInteraction(at: point, windows: windows, ownerPID: getppid())
    }
    static func isWidgetInteraction(at point: CGPoint, windows: [[String: Any]], ownerPID: pid_t) -> Bool {
        // Only the topmost window under the pointer may exempt a click. The
        // parent app owns our floating windows at NSScreenSaverWindowLevel.
        for window in windows {
            guard let bounds = window[kCGWindowBounds as String] as? [String: Any],
                  let rect = CGRect(dictionaryRepresentation: bounds as CFDictionary), rect.contains(point),
                  (window[kCGWindowAlpha as String] as? NSNumber)?.doubleValue != 0 else { continue }
            return (window[kCGWindowOwnerPID as String] as? NSNumber)?.int32Value == ownerPID
                && (window[kCGWindowLayer as String] as? NSNumber)?.intValue == 1000
        }
        return false
    }
    static let codes: [String: CGKeyCode] = ["Hold:Fn": 63, "Hold:LeftOption": 58, "Hold:RightOption": 61, "Hold:Escape": 53]
    private static let replayTag: Int64 = 0x54455854484F4C44
    private var bindings: [String: String] = [:]
    private var tap: CFMachPort?
    private var source: CFRunLoopSource?
    private var timer: Timer?
    private var trigger = HoldTrigger()
    private var candidate: String?
    private var escapeDown: CGEvent?
    private var suspended = false
    private var pressedKeys = Set<CGKeyCode>()
    var reservesEscape: Bool { !suspended && bindings.values.contains("Hold:Escape") }

    static func modifierDown(_ code: CGKeyCode, flags: CGEventFlags) -> Bool {
        if code == 63 { return flags.contains(.maskSecondaryFn) }
        guard flags.contains(.maskAlternate) else { return false }
        let left = UInt64(NX_DEVICELALTKEYMASK), right = UInt64(NX_DEVICERALTKEYMASK)
        let sides = flags.rawValue & (left | right)
        // Hardware events distinguish left/right Option; synthetic events can
        // carry only the aggregate flag. Never query the previous global state.
        return sides == 0 || (sides & (code == 58 ? left : right)) != 0
    }

    func configure(_ bindings: [String: String]) {
        if self.bindings != bindings { interrupt(); candidate = nil; self.bindings = bindings }
    }
    func suspend(_ value: Bool) { interrupt(); candidate = nil; pressedKeys.removeAll(); suspended = value }
    private func report(_ phase: String, key: String) {
        guard let mode = bindings.first(where: { $0.value == key })?.key else { return }
        sendEvent(["phase": phase, "mode": mode, "key": key])
    }
    private func replayEscape(up: CGEvent? = nil) {
        guard let down = escapeDown else { return }
        escapeDown = nil
        down.setIntegerValueField(.eventSourceUserData, value: Self.replayTag)
        postEvent(down)
        if let up {
            up.setIntegerValueField(.eventSourceUserData, value: Self.replayTag)
            postEvent(up)
        }
    }
    private func interrupt() {
        timer?.invalidate(); timer = nil
        let active = trigger.interrupt()
        if active, let candidate { report("hold_cancel", key: candidate); escapeDown = nil }
        // If Escape became part of another action, deliver its deferred down.
        replayEscape()
    }
    func install() {
        guard tap == nil, !bindings.isEmpty else { return }
        let mask = [CGEventType.keyDown, .keyUp, .flagsChanged, .leftMouseDown, .rightMouseDown, .otherMouseDown]
            .reduce(CGEventMask(0)) { $0 | (1 << $1.rawValue) }
        guard let tap = CGEvent.tapCreate(tap: .cgSessionEventTap, place: .headInsertEventTap,
            options: .defaultTap, eventsOfInterest: mask, callback: { _, type, event, context in
                guard let context else { return Unmanaged.passUnretained(event) }
                return Unmanaged<HoldShortcuts>.fromOpaque(context).takeUnretainedValue().handle(type, event)
            }, userInfo: Unmanaged.passUnretained(self).toOpaque()) else {
            sendEvent(["phase": "hold_status", "message": "Allow Accessibility for TextHalo to use hold keys, then select the shortcut again."])
            return
        }
        self.tap = tap
        source = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, tap, 0)
        CFRunLoopAddSource(CFRunLoopGetMain(), source, .commonModes)
        CGEvent.tapEnable(tap: tap, enable: true)
        sendEvent(["phase": "hold_status", "message": ""])
    }
    func handle(_ type: CGEventType, _ event: CGEvent) -> Unmanaged<CGEvent>? {
        let pass = Unmanaged.passUnretained(event)
        if type == .scrollWheel { return pass }
        if type == .tapDisabledByTimeout || type == .tapDisabledByUserInput {
            interrupt(); candidate = nil; pressedKeys.removeAll()
            if let tap { CGEvent.tapEnable(tap: tap, enable: true) }
            return pass
        }
        guard !suspended, event.getIntegerValueField(.eventSourceUserData) != Self.replayTag else { return pass }
        let code = CGKeyCode(event.getIntegerValueField(.keyboardEventKeycode))
        if type == .keyDown { pressedKeys.insert(code) }
        if type == .keyUp { pressedKeys.remove(code) }
        let keyboard = type == .keyDown || type == .keyUp || type == .flagsChanged
        let key = keyboard ? bindings.values.first(where: { Self.codes[$0] == code }) : nil
        if let candidate, key == candidate {
            let down = type == .flagsChanged
                ? Self.modifierDown(code, flags: event.flags)
                : type == .keyDown
            if !down {
                timer?.invalidate(); timer = nil
                let outcome = trigger.release()
                self.candidate = nil
                if outcome == .stop { report("hold_released", key: candidate) }
                if code == 53, let _ = escapeDown {
                    if outcome == .tap { replayEscape(up: event.copy()) } else { escapeDown = nil }
                    return nil
                }
            }
            // Repeated Escape down must not dismiss the user's target window.
            return code == 53 && escapeDown != nil ? nil : pass
        }
        if candidate != nil {
            if [.leftMouseDown, .rightMouseDown, .otherMouseDown].contains(type), allowsWidgetInteraction(event) { return pass }
            interrupt(); return pass
        }
        guard let key, type != .keyUp else { return pass }
        let allowed: CGEventFlags = code == 63 ? .maskSecondaryFn : code == 53 ? [] : .maskAlternate
        let modifiers: CGEventFlags = [.maskCommand, .maskControl, .maskAlternate, .maskShift, .maskSecondaryFn]
        guard event.flags.intersection(modifiers).subtracting(allowed).isEmpty,
              pressedKeys.subtracting([code]).isEmpty,
              type == .keyDown || Self.modifierDown(code, flags: event.flags),
              event.getIntegerValueField(.keyboardEventAutorepeat) == 0 else { return pass }
        candidate = key
        trigger.press(at: ProcessInfo.processInfo.systemUptime)
        if code == 53 { escapeDown = event.copy() }
        timer = Timer.scheduledTimer(withTimeInterval: 0.35, repeats: false) { [weak self] _ in
            guard let self, self.candidate == key,
                  self.trigger.activate(at: ProcessInfo.processInfo.systemUptime) else { return }
            self.report("hold_pressed", key: key)
        }
        // Modifier events remain available for normal Option/Fn combinations.
        return code == 53 ? nil : pass
    }
}
