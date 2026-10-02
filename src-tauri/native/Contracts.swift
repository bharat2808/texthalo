import Foundation

struct TargetSnapshot: Equatable {
    let value: String
    let location: Int
    let length: Int
    func permitsInsertion(current: TargetSnapshot, sameElement: Bool, editable: Bool) -> Bool {
        sameElement && editable && self == current && location >= 0 && length >= 0
            && location <= value.utf16.count && length <= value.utf16.count - location
    }
}

struct Request: Decodable {
    var command: String
    var id: UInt64 = 0
    var mode: String?
    var language: String?
    var instruction: String?
    var context: String?
    var output: String?
    var text: String?
    var key: String?
    var modifiers: String?
    var restore: Bool?
}

// A recognizer may finalize before key-up; only release authorizes delivering text.
struct TranscriptGate {
    private var held = true
    private var delivered = false
    private var pending: String?
    mutating func recognized(_ text: String) -> String? {
        guard !delivered else { return nil }
        if held { pending = text; return nil }
        delivered = true
        return text
    }
    mutating func release() -> String? {
        guard held, !delivered else { return nil }
        held = false
        if let pending { delivered = true; return pending }
        return nil
    }
}
