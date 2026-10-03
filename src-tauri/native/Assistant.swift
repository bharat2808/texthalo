import AppKit
import AVFoundation
import Speech
import Vision
import ApplicationServices
import FoundationModels
import OSLog

func emit(_ value: [String: Any]) {
    guard let data = try? JSONSerialization.data(withJSONObject: value),
          let line = String(data: data, encoding: .utf8) else { return }
    print(line)
    fflush(stdout)
}

func attribute(_ element: AXUIElement, _ name: CFString) -> CFTypeRef? {
    var result: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, name, &result) == .success else { return nil }
    return result
}
func focusedElement() -> AXUIElement? {
    guard let value = attribute(AXUIElementCreateSystemWide(), kAXFocusedUIElementAttribute as CFString),
          CFGetTypeID(value) == AXUIElementGetTypeID() else { return nil }
    return unsafeBitCast(value, to: AXUIElement.self)
}
func snapshot(_ element: AXUIElement) -> TargetSnapshot? {
    guard let value = attribute(element, kAXValueAttribute as CFString) as? String,
          let raw = attribute(element, kAXSelectedTextRangeAttribute as CFString),
          CFGetTypeID(raw) == AXValueGetTypeID() else { return nil }
    let axValue = unsafeBitCast(raw, to: AXValue.self)
    guard AXValueGetType(axValue) == .cfRange else { return nil }
    var range = CFRange()
    guard AXValueGetValue(axValue, .cfRange, &range) else { return nil }
    return .init(value: value, location: range.location, length: range.length)
}

@MainActor final class Target {
    let application: NSRunningApplication?
    let element: AXUIElement?
    let original: TargetSnapshot?
    let selection: String
    let sourceContext: AssistantSource
    let windowID: CGWindowID?
    let bounds: CGRect?
    let caretBounds: CGRect?
    var receipt: TextEditReceipt?
    var committed: TargetSnapshot?
    init() {
        application = NSWorkspace.shared.frontmostApplication
        let current = AXIsProcessTrusted() ? focusedElement() : nil
        let secure = current.flatMap { attribute($0, kAXSubroleAttribute as CFString) as? String } == "AXSecureTextField"
        element = secure ? nil : current
        original = element.flatMap(snapshot)
        var anchor: CGRect?
        if let element, let original {
            var range = CFRange(location: original.location, length: original.length)
            if let rangeValue = AXValueCreate(.cfRange, &range) {
                var raw: CFTypeRef?
                if AXUIElementCopyParameterizedAttributeValue(element, kAXBoundsForRangeParameterizedAttribute as CFString, rangeValue, &raw) == .success,
                   let raw, CFGetTypeID(raw) == AXValueGetTypeID() {
                    var rect = CGRect.zero
                    if AXValueGetValue(unsafeBitCast(raw, to: AXValue.self), .cgRect, &rect), rect.height > 0 { anchor = rect }
                }
            }
        }
        caretBounds = anchor
        selection = secure ? "" : (element.flatMap { attribute($0, kAXSelectedTextAttribute as CFString) as? String } ?? "")
        let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
        let sourcePID = application?.processIdentifier
        let sourceWindows = windows.filter { ($0[kCGWindowOwnerPID as String] as? Int32) == sourcePID && ($0[kCGWindowLayer as String] as? Int) == 0 }
        var focusedWindowTitle = ""
        if let sourcePID, AXIsProcessTrusted(),
           let rawWindow = attribute(AXUIElementCreateApplication(sourcePID), kAXFocusedWindowAttribute as CFString),
           CFGetTypeID(rawWindow) == AXUIElementGetTypeID() {
            let focusedWindow = unsafeBitCast(rawWindow, to: AXUIElement.self)
            focusedWindowTitle = attribute(focusedWindow, kAXTitleAttribute as CFString) as? String ?? ""
        }
        let window = sourceWindows.first { !focusedWindowTitle.isEmpty && ($0[kCGWindowName as String] as? String) == focusedWindowTitle } ?? sourceWindows.first
        let fieldLabel = element.flatMap { field in
            [kAXTitleAttribute, kAXDescriptionAttribute, kAXPlaceholderValueAttribute].compactMap {
                attribute(field, $0 as CFString) as? String
            }.first(where: { !$0.isEmpty })
        } ?? ""
        sourceContext = AssistantSource(
            app: application?.localizedName ?? "Unknown app",
            windowTitle: String((focusedWindowTitle.isEmpty ? window?[kCGWindowName as String] as? String ?? "" : focusedWindowTitle).prefix(500)),
            focusedRole: element.flatMap { attribute($0, kAXRoleAttribute as CFString) as? String } ?? "",
            focusedLabel: String(fieldLabel.prefix(300)))
        windowID = (window?[kCGWindowNumber as String] as? NSNumber)?.uint32Value
        bounds = (window?[kCGWindowBounds as String] as? NSDictionary).flatMap { CGRect(dictionaryRepresentation: $0) }
    }
    var editable: Bool {
        guard let element, original != nil else { return false }
        var settable = DarwinBoolean(false)
        return AXUIElementIsAttributeSettable(element, kAXSelectedTextAttribute as CFString, &settable) == .success && settable.boolValue
    }
    func insert(_ text: String, restoreClipboard: Bool) async -> Bool {
        guard let application, !application.isTerminated,
              NSWorkspace.shared.frontmostApplication == application,
              let element, let focused = focusedElement(), let original, let current = snapshot(element),
              original.permitsInsertion(current: current, sameElement: CFEqual(element, focused), editable: true),
              let edit = TextEditReceipt(original: original, replacement: text) else { return false }
        if editable && AXUIElementSetAttributeValue(element, kAXSelectedTextAttribute as CFString, text as CFString) == .success {
            if let after = snapshot(element), edit.acceptsCommit(after) {
                receipt = edit; committed = after
                return true
            }
        }
        // Web and Electron editors often expose the caret but not a writable
        // AXSelectedText. Paste only into the same, unchanged editable field.
        let role = attribute(element, kAXRoleAttribute as CFString) as? String
        guard original.permitsPaste(current: current, sameElement: true, role: role) else { return false }
        let modifiers: CGEventFlags = [.maskCommand, .maskControl, .maskAlternate, .maskShift]
        for _ in 0..<40 {
            if CGEventSource.flagsState(.combinedSessionState).intersection(modifiers).isEmpty { break }
            do { try await Task.sleep(for: .milliseconds(25)) } catch { return false }
        }
        guard !Task.isCancelled,
              CGEventSource.flagsState(.combinedSessionState).intersection(modifiers).isEmpty,
              NSWorkspace.shared.frontmostApplication == application,
              let focused = focusedElement(), CFEqual(element, focused), snapshot(element) == original,
              let source = CGEventSource(stateID: .hidSystemState),
              let down = CGEvent(keyboardEventSource: source, virtualKey: 9, keyDown: true),
              let up = CGEvent(keyboardEventSource: source, virtualKey: 9, keyDown: false) else { return false }
        let pasteboard = NSPasteboard.general
        let saved = restoreClipboard ? (pasteboard.pasteboardItems ?? []).map { item in
            item.types.compactMap { type in item.data(forType: type).map { (type, $0) } }
        } : []
        pasteboard.clearContents()
        let wroteText = pasteboard.setString(text, forType: .string)
        let changeCount = pasteboard.changeCount
        defer {
            if restoreClipboard {
                // Let the receiving app consume the paste, and never overwrite a
                // newer clipboard change made by the user or another app.
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.8) {
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
        let dispatch = PasteDispatch(wroteText: wroteText, targetValid: true, keyEventsReady: true)
        guard dispatch.isDeliverable else { return false }
        down.flags = .maskCommand; up.flags = .maskCommand
        down.post(tap: .cgSessionEventTap); up.post(tap: .cgSessionEventTap)
        // Content-editable apps may accept paste without exposing their new
        // value through Accessibility. Dispatch is the reliable boundary.
        return true
    }
    var canUndo: Bool { receipt != nil && committed != nil }
    func undo() -> Bool {
        guard let application, !application.isTerminated, NSWorkspace.shared.frontmostApplication == application,
              let element, let focused = focusedElement(), let current = snapshot(element),
              let receipt, let committed,
              receipt.permitsUndo(current: current, committed: committed, sameElement: CFEqual(element, focused), editable: editable) else { return false }
        var range = CFRange(location: receipt.original.location, length: receipt.replacementLength)
        guard let rangeValue = AXValueCreate(.cfRange, &range),
              AXUIElementSetAttributeValue(element, kAXSelectedTextRangeAttribute as CFString, rangeValue) == .success else { return false }
        guard AXUIElementSetAttributeValue(element, kAXSelectedTextAttribute as CFString, receipt.originalText as CFString) == .success else {
            var restore = CFRange(location: committed.location, length: committed.length)
            if let value = AXValueCreate(.cfRange, &restore) { AXUIElementSetAttributeValue(element, kAXSelectedTextRangeAttribute as CFString, value) }
            return false
        }
        self.receipt = nil; self.committed = nil
        return true
    }

}

@MainActor final class Assistant {
    let holdShortcuts = HoldShortcuts()
    var id: UInt64 = 0
    var request: Request?
    var target: Target?
    var context = ""
    var result = ""
    var engine: AVAudioEngine?
    var audioRequest: SFSpeechAudioBufferRecognitionRequest?
    var recognition: SFSpeechRecognitionTask?
    var generation: Task<Void, Never>?
    var insertion: Task<Void, Never>?
    var deadline: Task<Void, Never>?
    var heldTimer: Timer?
    var transcriptGate = TranscriptGate()
    var screenProcess: Process?
    var recording = false
    var finishing = false
    var monitors: [Any] = []
    var diagnosticRunning = false
    var diagnostic = ""
    let screenLog = Logger(subsystem: "com.kiegen.app", category: "ScreenText")

    func event(_ phase: String, _ message: String = "", text: String = "", action: String = "preview") {
        var event: [String: Any] = ["id": id, "phase": phase, "message": message, "text": text,
            "action": action, "canInsert": target?.editable ?? false, "canUndo": target?.canUndo ?? false,
            "mode": request?.mode ?? "assistant",
            "source": target?.application?.localizedName ?? "", "context": context]
        if let bounds = target?.caretBounds ?? target?.bounds {
            event["bounds"] = ["x": bounds.origin.x, "y": bounds.origin.y, "width": bounds.width, "height": bounds.height]
        }
        emit(event)
    }
    func stopAudio() {
        heldTimer?.invalidate(); heldTimer = nil
        if let engine { engine.stop(); engine.inputNode.removeTap(onBus: 0) }
        engine = nil
        audioRequest?.endAudio()
        recording = false
    }
    func cancel() {
        insertion?.cancel(); insertion = nil
        generation?.cancel(); generation = nil
        deadline?.cancel(); deadline = nil
        stopAudio()
        recognition?.cancel(); recognition = nil
        audioRequest = nil
        finishing = false
        transcriptGate = TranscriptGate()
        if let process = screenProcess, process.isRunning { process.terminate() }
        screenProcess = nil
    }
    func availability() {
        var model = "Requires macOS 26 or later and Apple Intelligence."
        if #available(macOS 26.0, *) { model = String(describing: SystemLanguageModel.default.availability) }
        emit(["id": 0, "phase": "availability", "model": model,
              "microphone": AVCaptureDevice.authorizationStatus(for: .audio).rawValue,
              "recognition": SFSpeechRecognizer.authorizationStatus().rawValue,
              "accessibility": AXIsProcessTrusted(), "screen": CGPreflightScreenCaptureAccess(),
              "imageInput": false, "diagnostic": diagnostic])
    }
    func permissions() async {
        // Only an explicit Settings button invokes this; never resume an old hold afterward.
        _ = await AVCaptureDevice.requestAccess(for: .audio)
        _ = await withCheckedContinuation { continuation in
            SFSpeechRecognizer.requestAuthorization { continuation.resume(returning: $0) }
        }
        availability()
    }
    func diagnose() async {
        guard !diagnosticRunning, !recording, !finishing else { return }
        diagnosticRunning = true
        diagnostic = "Testing local transcription with generated English audio…"
        availability()
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { diagnosticRunning = false; try? FileManager.default.removeItem(at: directory); availability() }
        do {
            _ = try localRecognizer(language: "en-US")
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
            let audio = directory.appendingPathComponent("fixture.aiff")
            let process = Process()
            process.executableURL = URL(fileURLWithPath: "/usr/bin/say")
            process.arguments = ["-v", "Samantha", "-o", audio.path, "The quick brown fox jumps over the lazy dog."]
            try process.run()
            await Task.detached { process.waitUntilExit() }.value
            guard process.terminationStatus == 0 else { throw NSError(domain: "Could not generate test speech", code: 1) }
            let recognition = FileRecognition()
            let text = try await recognition.run(url: audio)
            guard text.lowercased().contains("fox"), text.lowercased().contains("dog") else {
                throw NSError(domain: "TextHalo", code: 4, userInfo: [NSLocalizedDescriptionKey: "The test audio was not transcribed correctly: \(text)"])
            }
            diagnostic = "Local transcription passed: \(text)"
        } catch { diagnostic = "Local transcription test failed: \(error.localizedDescription)" }
    }
    func handle(_ command: Request) {
        let startsAssistant = command.command == "open" || command.command == "ask"
            || (command.command == "start" && command.mode == "assistant")
        if startsAssistant && !CGPreflightScreenCaptureAccess() {
            cancel(); id = command.id
            event("error", "Enable Screen Recording in TextHalo’s Assistant section, then try again.")
            return
        }
        switch command.command {
        case "hold_configure": holdShortcuts.configure(command.holdBindings ?? [:]); holdShortcuts.install()
        case "hold_suspend": holdShortcuts.suspend(true)
        case "hold_resume": holdShortcuts.suspend(false); holdShortcuts.install()
        case "status": availability()
        case "permissions": Task { await permissions() }
        case "permission_microphone": Task { _ = await AVCaptureDevice.requestAccess(for: .audio); availability() }
        case "permission_speech": Task {
            _ = await withCheckedContinuation { continuation in SFSpeechRecognizer.requestAuthorization { continuation.resume(returning: $0) } }
            availability()
        }
        case "permission_screen":
            if !CGPreflightScreenCaptureAccess() { _ = CGRequestScreenCaptureAccess() }
            availability()
        case "settings_microphone", "settings_speech", "settings_screen":
            let pane = command.command == "settings_microphone" ? "Privacy_Microphone" : command.command == "settings_speech" ? "Privacy_SpeechRecognition" : "Privacy_ScreenCapture"
            if let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?" + pane) { NSWorkspace.shared.open(url) }
        case "diagnose": Task { await diagnose() }
        case "cancel":
            if command.restore == true { restoreSourceIfAssistantFocused(explicit: true) }
            cancel(); id = command.id; target = nil; context = ""; result = ""; event("idle")
        case "open":
            cancel(); id = command.id; request = command; result = ""
            let source = Target()
            target = source.application?.processIdentifier == getppid() ? nil : source
            context = sourceReference(mode: command.context)
            event("result", "Type a request, or hold your Assistant shortcut.")
        case "start": start(command)
        case "stop": if command.id == id { finishRecording() }
        case "ask":
            guard let text = command.text, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
                id = command.id; event("error", "Enter a request first."); return
            }
            cancel(); id = command.id; request = command
            context = sourceReference(mode: command.context)
            if command.usesScreenContext { captureScreen(region: false, prompt: text) }
            else { generate(text) }
        case "undo":
            guard command.id == id, target?.canUndo == true else { return }
            let currentID = id
            if command.restore == true { restoreSourceIfAssistantFocused(explicit: true) }
            Task {
                if command.restore == true { try? await Task.sleep(for: .milliseconds(180)) }
                guard self.id == currentID else { return }
                if self.target?.undo() == true { self.event("result", "Change undone", action: "undone") }
                else { self.target?.receipt = nil; self.target?.committed = nil; self.event("result", "The field changed. Undo is no longer available.", text: self.result, action: "undo_unavailable") }
            }
        case "copy":
            guard command.id == id, !result.isEmpty else { return }
            NSPasteboard.general.clearContents(); NSPasteboard.general.setString(result, forType: .string)
            event("result", "Copied.", text: result)
        case "screen":
            guard command.id == id else { return }
            captureScreen(region: command.context == "region")
        default: emit(["id": command.id, "phase": "error", "message": "Unknown assistant command."])
        }
    }
    func start(_ command: Request) {
        cancel(); id = command.id; request = command; result = ""; context = ""; target = Target()
        if command.mode == "assistant", command.context != "none" {
            context = sourceReference(mode: command.context)
        }
        guard AVCaptureDevice.authorizationStatus(for: .audio) == .authorized,
              SFSpeechRecognizer.authorizationStatus() == .authorized else {
            event("error", "Enable Microphone and Speech Recognition in TextHalo’s Permissions section, then hold the shortcut again."); return
        }
        if command.output != "copy" || command.mode == "assistant" {
            guard AXIsProcessTrusted() else {
                event("error", "Enable Accessibility for TextHalo to insert text into your app."); return
            }
        }
        event("processing", "Starting microphone…")
        let recognizer: SFSpeechRecognizer
        do { recognizer = try localRecognizer(language: command.language ?? "en-US") }
        catch { event("error", error.localizedDescription); return }
        if command.mode == "assistant" {
            guard #available(macOS 26.0, *) else { event("error", "Assistant requires macOS 26 or later."); return }
            guard SystemLanguageModel.default.isAvailable else { event("error", "Apple Intelligence is unavailable: \(SystemLanguageModel.default.availability)"); return }
            context = sourceReference(mode: command.context)
        }
        let currentID = id
        let bufferRequest = SFSpeechAudioBufferRecognitionRequest()
        bufferRequest.requiresOnDeviceRecognition = true
        bufferRequest.shouldReportPartialResults = true
        audioRequest = bufferRequest
        recognition = recognizer.recognitionTask(with: bufferRequest) { [weak self] response, error in
            Task { @MainActor in
                guard let self, self.id == currentID, self.recording || self.finishing else { return }
                if let response, response.isFinal {
                    let text = response.bestTranscription.formattedString
                    if let transcript = self.transcriptGate.recognized(text) { self.completeTranscript(transcript) }
                } else if let error {
                    self.cancel(); self.event("error", "Transcription failed: \(error.localizedDescription)")
                }
            }
        }
        let audioEngine = AVAudioEngine()
        let input = audioEngine.inputNode
        let format = input.outputFormat(forBus: 0)
        guard format.sampleRate > 0, format.channelCount > 0 else { cancel(); event("error", "No microphone input is available."); return }
        input.installTap(onBus: 0, bufferSize: 1024, format: format) { buffer, _ in bufferRequest.append(buffer) }
        engine = audioEngine
        do {
            try audioEngine.start(); recording = true
            event("recording", command.mode == "assistant" ? "Listening to your request… Release to ask." : "Listening… Release to transcribe.")
            // Carbon hotkeys report release; this checks physical state too, including modifiers released first.
            let code = keyCode(command.key ?? "")
            let required = requiredFlags(command.modifiers ?? "")
            heldTimer = Timer.scheduledTimer(withTimeInterval: 0.04, repeats: true) { [weak self] _ in
                Task { @MainActor in
                    guard let self, self.id == currentID, self.recording else { return }
                    if command.key?.hasPrefix("Hold:") == true { return }
                    let flags = CGEventSource.flagsState(.combinedSessionState)
                    if (code.map { !CGEventSource.keyState(.combinedSessionState, key: $0) } ?? false)
                        || !flags.contains(required) { self.finishRecording() }
                }
            }
            deadline = Task { try? await Task.sleep(for: .seconds(90)); guard !Task.isCancelled, self.id == currentID else { return }; self.cancel(); self.event("error", "Recording reached 90 seconds. Release and try a shorter recording.") }
        } catch { cancel(); event("error", "Could not start microphone: \(error.localizedDescription)") }
    }
    func finishRecording() {
        guard recording, !finishing else { return }
        finishing = true; stopAudio(); event("processing", "Transcribing on this Mac…")
        if let transcript = transcriptGate.release() { completeTranscript(transcript); return }
        deadline?.cancel()
        let currentID = id
        deadline = Task {
            try? await Task.sleep(for: .seconds(12))
            guard !Task.isCancelled, self.id == currentID, self.finishing else { return }
            self.cancel(); self.event("error", "Transcription did not finish. Try a shorter recording.")
        }
    }
    func completeTranscript(_ text: String) {
        let text = text.trimmingCharacters(in: .whitespacesAndNewlines)
        let mode = request?.mode
        cancel()
        guard !text.isEmpty else { event("error", "No speech detected. Hold the shortcut and try again."); return }
        if mode == "assistant" {
            if request?.usesScreenContext == true { captureScreen(region: false, prompt: text) }
            else { generate(text) }
        }
        else {
            event("transcript", text: text)
            result = text
            if request?.output == "copy" {
                NSPasteboard.general.clearContents(); NSPasteboard.general.setString(text, forType: .string)
                event("result", "Transcript copied.", action: "copied")
            } else { insertResult() }
        }
    }
    func insertResult() {
        let currentID = id
        let text = result
        let source = target
        let restoreClipboard = request?.restoreClipboard ?? true
        event("processing", "Inserting text…")
        insertion = Task {
            let inserted = await source?.insert(text, restoreClipboard: restoreClipboard) == true
            guard !Task.isCancelled, self.id == currentID else { return }
            self.insertion = nil
            if inserted { self.event("result", source?.original?.length == 0 ? "Text inserted" : "Text replaced", action: "applied") }
            else { self.event("result", "Could not insert into the original field. Keep your cursor in an editable text field while dictating, or use Copy.", text: text) }
        }
    }
    func sourceReference(mode: String?, screenOCR: String? = nil) -> String {
        AssistantPrompt.referenceContext(selectedText: target?.selection ?? "", screenOCR: screenOCR,
                                         source: target?.sourceContext, mode: mode)
    }
    func generate(_ prompt: String) {
        guard #available(macOS 26.0, *) else { event("error", "Assistant requires macOS 26 or later."); return }
        guard SystemLanguageModel.default.isAvailable else { event("error", "Apple Intelligence is unavailable: \(SystemLanguageModel.default.availability)"); return }
        guard prompt.count <= 4000, context.count <= 10000 else { event("error", "Select a shorter passage or use a shorter request."); return }
        let currentID = id
        let source = context
        let previous = String(result.prefix(6000))
        let preference = String((request?.instruction ?? "").prefix(2000))
        let selection = request?.context == "none" ? "" : target?.selection ?? ""
        event("processing", "Thinking on this Mac…")
        deadline?.cancel()
        deadline = Task {
            try? await Task.sleep(for: .seconds(90))
            guard !Task.isCancelled, self.id == currentID else { return }
            self.cancel(); self.event("error", "Generation timed out. Try a shorter request.")
        }
        generation = Task {
            defer { if self.id == currentID { self.deadline?.cancel(); self.deadline = nil; self.generation = nil } }
            do {
                let reply = try await generateAssistantReply(request: prompt, preference: preference,
                    reference: source, previous: previous, selection: selection)
                guard !Task.isCancelled, self.id == currentID else { return }
                let readOriginal = reply.action == .readSelection
                let text = readOriginal ? selection : reply.text
                guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { self.event("error", "Assistant returned an empty answer. Please try your request again."); return }
                self.result = text
                if reply.action == .speak || readOriginal { self.event("result", "Preparing narration…", text: text, action: "speak") }
                else if reply.action == .clarify { self.event("result", "Hold Assistant again with more detail", text: text, action: "clarify") }
                else { self.insertResult() }
            } catch {
                guard !Task.isCancelled, self.id == currentID else { return }
                self.event("error", "Assistant could not complete this request: \(error.localizedDescription)")
            }
        }
    }
    func captureScreen(region: Bool, prompt: String? = nil) {
        guard CGPreflightScreenCaptureAccess() else {
            event("error", "Enable Screen Recording in TextHalo’s Permissions section, then hold Assistant again."); return
        }
        let currentID = id
        let window = target?.windowID
        guard region || window != nil else { event("error", "No original window is available. Use selected-text context in Assistant settings."); return }
        event("capturing_screen", "Capturing your window…")
        generation = Task {
            defer { if self.id == currentID { self.screenProcess = nil } }
            do {
                let started = ProcessInfo.processInfo.systemUptime
                let image: CGImage
                if region {
                    // Preserve explicit region selection; normal window capture
                    // never launches the screenshot command or writes an image.
                    let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
                    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
                    defer { try? FileManager.default.removeItem(at: directory) }
                    let file = directory.appendingPathComponent("screen.png")
                    let process = Process()
                    self.screenProcess = process
                    process.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
                    process.arguments = ["-x", "-i", "-s", "-T", "0", file.path]
                    try Task.checkCancellation()
                    try process.run()
                    image = try await Task.detached {
                        process.waitUntilExit()
                        guard process.terminationStatus == 0,
                              let source = CGImageSourceCreateWithURL(file as CFURL, nil),
                              let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else { throw NSError(domain: "Capture canceled", code: 1) }
                        return image
                    }.value
                } else {
                    image = try await ScreenText.capture(windowID: window!)
                }
                guard !Task.isCancelled, self.id == currentID else { return }
                let captured = ProcessInfo.processInfo.systemUptime
                self.screenLog.info("Capture completed in \(Int((captured - started) * 1000)) ms")
                self.event("capturing_screen", "Reading screen text locally…")
                let text = try await ScreenText.recognize(image)
                self.screenLog.info("OCR completed in \(Int((ProcessInfo.processInfo.systemUptime - captured) * 1000)) ms")
                guard !Task.isCancelled, self.id == currentID else { return }
                guard !text.isEmpty else { self.event("error", "No readable text found. This build extracts text, not visual descriptions."); return }
                self.context = String(self.sourceReference(mode: self.request?.context, screenOCR: text).prefix(10000))
                if let prompt { self.generate(prompt) }
                else { self.event("result", "Screen text captured", text: self.result) }
            } catch { guard !Task.isCancelled, self.id == currentID else { return }; self.event("error", "Could not read screen text: \(error.localizedDescription)") }
        }
    }
    func restoreSourceIfAssistantFocused(explicit: Bool = false) {
        guard let front = NSWorkspace.shared.frontmostApplication,
              front.processIdentifier == getppid(), let source = target?.application,
              source != front, !source.isTerminated else { return }
        if !explicit {
            let appElement = AXUIElementCreateApplication(front.processIdentifier)
            guard let window = attribute(appElement, kAXFocusedWindowAttribute as CFString), CFGetTypeID(window) == AXUIElementGetTypeID(),
                  attribute(unsafeBitCast(window, to: AXUIElement.self), kAXTitleAttribute as CFString) as? String == "TextHalo Assistant" else { return }
        }
        source.activate(options: [])
    }
    func installEscape() {
        if let monitor = NSEvent.addGlobalMonitorForEvents(matching: .keyDown, handler: { [weak self] event in
            if event.keyCode == 53 { Task { @MainActor in
                guard let self, !self.holdShortcuts.reservesEscape,
                      self.recording || self.finishing || self.generation != nil || self.target != nil || !self.result.isEmpty else { return }
                self.restoreSourceIfAssistantFocused()
                self.cancel(); self.target = nil; self.context = ""; self.result = ""; self.event("canceled", "Canceled")
            } }
        }) { monitors.append(monitor) }
    }
}

func requiredFlags(_ names: String) -> CGEventFlags {
    var flags: CGEventFlags = []
    let names = names.lowercased()
    if names.contains("super") || names.contains("meta") { flags.insert(.maskCommand) }
    if names.contains("control") { flags.insert(.maskControl) }
    if names.contains("alt") { flags.insert(.maskAlternate) }
    if names.contains("shift") { flags.insert(.maskShift) }
    return flags
}
func keyCode(_ name: String) -> CGKeyCode? {
    let names = ["KeyA":0,"KeyS":1,"KeyD":2,"KeyF":3,"KeyH":4,"KeyG":5,"KeyZ":6,"KeyX":7,"KeyC":8,"KeyV":9,"KeyB":11,"KeyQ":12,"KeyW":13,"KeyE":14,"KeyR":15,"KeyY":16,"KeyT":17,"Digit1":18,"Digit2":19,"Digit3":20,"Digit4":21,"Digit6":22,"Digit5":23,"Equal":24,"Digit9":25,"Digit7":26,"Minus":27,"Digit8":28,"Digit0":29,"BracketRight":30,"KeyO":31,"KeyU":32,"BracketLeft":33,"KeyI":34,"KeyP":35,"Enter":36,"KeyL":37,"KeyJ":38,"Quote":39,"KeyK":40,"Semicolon":41,"Backslash":42,"Comma":43,"Slash":44,"KeyN":45,"KeyM":46,"Period":47,"Tab":48,"Space":49,"Backquote":50,"Backspace":51,"Escape":53,"F1":122,"F2":120,"F3":99,"F4":118,"F5":96,"F6":97,"F7":98,"F8":100,"F9":101,"F10":109,"F11":103,"F12":111,"ArrowLeft":123,"ArrowRight":124,"ArrowDown":125,"ArrowUp":126]
    return names[name].map(CGKeyCode.init)
}

@main struct Main {
    @MainActor static func main() {
        let app = NSApplication.shared
        app.setActivationPolicy(.prohibited)
        let assistant = Assistant()
        assistant.installEscape()
        DispatchQueue.global().async {
            while let line = readLine() {
                guard line.utf8.count < 100_000, let data = line.data(using: .utf8), let request = try? JSONDecoder().decode(Request.self, from: data) else {
                    emit(["id": 0, "phase": "error", "message": "Invalid native request."]); continue
                }
                DispatchQueue.main.async { assistant.handle(request) }
            }
            DispatchQueue.main.async { assistant.cancel(); exit(0) }
        }
        app.run()
    }
}
