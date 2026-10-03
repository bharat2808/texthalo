import CoreGraphics
import Foundation
import FoundationModels

enum AssistantAction: String, CaseIterable, Equatable {
    case text
    case speak
    case readSelection = "read-selection"
    case clarify

    var marker: String {
        switch self {
        case .text: "0|"
        case .speak: "1|"
        case .readSelection: "2|"
        case .clarify: "3|"
        }
    }
}

struct AssistantReply: Equatable {
    let action: AssistantAction
    let text: String
}

enum AssistantProtocolError: LocalizedError, Equatable {
    case invalidMarker(String)
    case missingSelection
    case emptyResponse

    var errorDescription: String? {
        switch self {
        case .invalidMarker:
            return "The local model returned an invalid Assistant response. Try again."
        case .missingSelection:
            return "Select some text before asking TextHalo to read it aloud."
        case .emptyResponse:
            return "The local model returned an empty Assistant response. Try again."
        }
    }
}

/// Decodes the model-owned delivery decision without inferring intent. Foundation
/// Models streams cumulative snapshots, so the decoder accepts partial markers,
/// strips the completed marker once, and publishes only changed marker-free text.
struct AssistantMarkerDecoder {
    private(set) var action: AssistantAction?
    private var last: AssistantReply?

    mutating func update(snapshot: String, selection: String) throws -> AssistantReply? {
        if action == nil {
            if let match = AssistantAction.allCases.first(where: { snapshot.hasPrefix($0.marker) }) {
                action = match
            } else if AssistantAction.allCases.contains(where: { $0.marker.hasPrefix(snapshot) }) {
                return nil
            } else {
                throw AssistantProtocolError.invalidMarker(String(snapshot.prefix(80)))
            }
        }
        guard let action else { return nil }
        let body = Self.body(in: snapshot, after: action.marker)
        let text: String
        if action == .readSelection {
            guard !selection.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
                throw AssistantProtocolError.missingSelection
            }
            text = selection
        } else {
            text = body
        }
        let reply = AssistantReply(action: action, text: text)
        guard reply != last, action == .readSelection || !text.isEmpty else { return nil }
        last = reply
        return reply
    }

    mutating func finish(snapshot: String, selection: String) throws -> AssistantReply {
        _ = try update(snapshot: snapshot, selection: selection)
        guard let action else { throw AssistantProtocolError.invalidMarker(String(snapshot.prefix(80))) }
        let text = action == .readSelection ? selection : Self.body(in: snapshot, after: action.marker)
        guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            throw AssistantProtocolError.emptyResponse
        }
        return AssistantReply(action: action, text: text)
    }

    private static func body(in snapshot: String, after marker: String) -> String {
        guard snapshot.hasPrefix(marker) else { return "" }
        var body = String(snapshot.dropFirst(marker.count))
        if body.hasPrefix("\r\n") { body.removeFirst() }
        else if body.hasPrefix("\n") { body.removeFirst() }
        return body
    }
}

enum AssistantImageSupport {
    static var isAvailable: Bool {
        #if FOUNDATION_MODELS_IMAGE_ATTACHMENTS
        if #available(macOS 27.0, *) { return true }
        #endif
        return false
    }
}

@available(macOS 26.0, *)
private func consumeAssistantStream(
    _ stream: LanguageModelSession.ResponseStream<String>,
    selection: String,
    onRaw: @escaping @MainActor (String) -> Void,
    onSnapshot: @escaping @MainActor (AssistantReply) -> Void
) async throws -> AssistantReply {
    var decoder = AssistantMarkerDecoder()
    var final = ""
    for try await snapshot in stream {
        try Task.checkCancellation()
        final = snapshot.content
        await onRaw(final)
        if let reply = try decoder.update(snapshot: final, selection: selection) {
            await onSnapshot(reply)
        }
    }
    try Task.checkCancellation()
    return try decoder.finish(snapshot: final, selection: selection)
}

/// One local model request owns both the delivery marker and the generated text.
/// There is no classifier, deterministic command parser, validation model, or
/// retry generation around this call.
@available(macOS 26.0, *)
func generateAssistantReply(
    request: String,
    preference: String,
    reference: String,
    previous: String,
    selection: String,
    screenImage: CGImage? = nil,
    onRaw: @escaping @MainActor (String) -> Void = { _ in },
    onSnapshot: @escaping @MainActor (AssistantReply) -> Void = { _ in }
) async throws -> AssistantReply {
    let instructions = AssistantPrompt.systemInstructions(preference: preference,
                                                          hasSelection: !selection.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
                                                          hasImage: screenImage != nil)
    let promptText = AssistantPrompt.userPrompt(request: request, reference: reference, previous: previous)
    let session = LanguageModelSession(instructions: instructions)

    if let screenImage {
        #if FOUNDATION_MODELS_IMAGE_ATTACHMENTS
        if #available(macOS 27.0, *) {
            let prompt = Prompt {
                Attachment(screenImage).label("original-window")
                promptText
            }
            return try await consumeAssistantStream(session.streamResponse(to: prompt),
                                                    selection: selection,
                                                    onRaw: onRaw,
                                                    onSnapshot: onSnapshot)
        }
        #endif
        _ = screenImage
        throw NSError(domain: "TextHalo.ImageContext", code: 1,
                      userInfo: [NSLocalizedDescriptionKey:
                        "Visual context requires a TextHalo build made with the macOS 27 SDK."])
    }

    return try await consumeAssistantStream(session.streamResponse(to: promptText),
                                            selection: selection,
                                            onRaw: onRaw,
                                            onSnapshot: onSnapshot)
}
