import Foundation

@main struct HostedAssistantTests {
    static func command(_ json: String) throws -> Request {
        try JSONDecoder().decode(Request.self, from: Data(json.utf8))
    }
    @MainActor static func main() async throws {
        assert(AssistantContextPolicy.supportsCloudScreenText(osMajor: 14))
        assert(!AssistantContextPolicy.supportsScreenText(osMajor: 14))
        var frozen = HostedAssistantResponse(id: 1, selection: "Frozen selection")
        let read = try frozen.receive(id: 1, action: "read-selection", text: "Model invented text", complete: true)
        assert(read == AssistantReply(action: .readSelection, text: "Frozen selection"))
        let late = try frozen.receive(id: 1, action: "text", text: "Late", complete: false)
        assert(late == nil)
        var monotonic = HostedAssistantResponse(id: 2, selection: "")
        _ = try monotonic.receive(id: 2, action: "text", text: "Hello 👋", complete: false)
        let duplicate = try monotonic.receive(id: 2, action: "text", text: "Hello 👋", complete: false)
        assert(duplicate == nil)
        do {
            _ = try monotonic.receive(id: 2, action: "text", text: "Hello", complete: false)
            assertionFailure("Regressing snapshots must fail")
        } catch { assert(monotonic.terminal) }
        for (action, text) in [("read-selection", ""), ("speak", ""), ("clarify", "  ")] {
            var invalid = HostedAssistantResponse(id: 3, selection: "")
            do {
                _ = try invalid.receive(id: 3, action: action, text: text, complete: true)
                assertionFailure("Missing selection or empty completion must fail")
            } catch { assert(invalid.terminal) }
        }
        var emptyAction = HostedAssistantResponse(id: 4, selection: "")
        _ = try emptyAction.receive(id: 4, action: "speak", text: "", complete: false)
        do {
            _ = try emptyAction.receive(id: 4, action: "text", text: "Wrong", complete: true)
            assertionFailure("Even empty snapshots must freeze the action")
        } catch { assert(emptyAction.terminal) }
        let assistant = Assistant()
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        assistant.audit = GenerationHistory(requestID: 31, source: "", directory: directory.path)
        assistant.id = 31
        assistant.request = try command(#"{"command":"start","id":31,"mode":"assistant","provider":"cloud","instruction":"Brief"}"#)
        assistant.result = "Earlier answer"
        assistant.generate("Write a greeting")
        await assistant.generation?.value
        assistant.handle(try command(#"{"command":"hosted_snapshot","id":30,"action":"clarify","text":"Wrong"}"#))
        assistant.handle(try command(#"{"command":"hosted_snapshot","id":31,"action":"clarify","text":"Which"}"#))
        assert(assistant.result == "Which", "Cloud snapshot must reach common delivery")
        assistant.handle(try command(#"{"command":"hosted_complete","id":31,"action":"clarify","text":"Which greeting?"}"#))
        assert(assistant.result == "Which greeting?")
        let files = try FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil)
        let record = try JSONSerialization.jsonObject(with: Data(contentsOf: files[0])) as! [String: Any]
        assert(record["provider"] as? String == "cloud")
        assert(record["model"] as? String == "assistant-standard")
        assistant.handle(try command(#"{"command":"hosted_snapshot","id":31,"action":"clarify","text":"Late"}"#))
        assert(assistant.result == "Which greeting?")
        assistant.generate("Another greeting")
        await assistant.generation?.value
        assistant.handle(try command(#"{"command":"hosted_snapshot","id":31,"action":"speak","text":"Hello"}"#))
        assistant.handle(try command(#"{"command":"hosted_complete","id":31,"action":"text","text":"Wrong"}"#))
        assert(assistant.result == "Hello", "Changing action must fail before insertion")
        assistant.handle(try command(#"{"command":"hosted_complete","id":31,"action":"speak","text":"Late"}"#))
        assert(assistant.result == "Hello")
        assistant.generate("One more")
        await assistant.generation?.value
        assistant.handle(try command(#"{"command":"hosted_complete","id":31,"action":"invalid","text":"Wrong"}"#))
        assistant.generate("Cancel this")
        await assistant.generation?.value
        assistant.cancel()
        assistant.handle(try command(#"{"command":"hosted_complete","id":31,"action":"text","text":"Late"}"#))
        assert(assistant.result == "Hello")
        assistant.generate("Network failure")
        await assistant.generation?.value
        assistant.handle(try command(#"{"command":"hosted_snapshot","id":31,"action":"speak","text":"Partial"}"#))
        assistant.handle(try command(#"{"command":"hosted_error","id":31,"text":"Wrong","message":"Connection lost"}"#))
        assert(assistant.result == "Partial")
        assistant.handle(try command(#"{"command":"hosted_complete","id":31,"action":"speak","text":"Late"}"#))
        assert(assistant.result == "Partial")
        assistant.id = 32
        assistant.handle(try command(#"{"command":"hosted_snapshot","id":31,"action":"text","text":"Late"}"#))
        assert(assistant.result == "Partial")
    }
}
