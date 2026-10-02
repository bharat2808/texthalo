import Foundation
@main struct ContractTests {
    static func main() {
        let saved = TargetSnapshot(value: "hello 🌍", location: 6, length: 2)
        assert(saved.permitsInsertion(current: saved, sameElement: true, editable: true))
        assert(!saved.permitsInsertion(current: saved, sameElement: false, editable: true))
        assert(!saved.permitsInsertion(current: saved, sameElement: true, editable: false))
        assert(!saved.permitsInsertion(current: .init(value: "hello 🌍!", location: 6, length: 2), sameElement: true, editable: true))
        assert(!saved.permitsInsertion(current: .init(value: "hello 🌍", location: 0, length: 0), sameElement: true, editable: true))
        let invalid = TargetSnapshot(value: "a", location: Int.max, length: 1)
        assert(!invalid.permitsInsertion(current: invalid, sameElement: true, editable: true))
        var hold = TranscriptGate()
        assert(hold.recognized("hello") == nil, "Early recognition must wait for release")
        assert(hold.release() == "hello")
        assert(hold.release() == nil, "Repeated release must not insert twice")
        hold = TranscriptGate()
        assert(hold.release() == nil)
        assert(hold.recognized("late final") == "late final")
        assert(hold.recognized("duplicate") == nil)
        let edit = TextEditReceipt(original: saved, replacement: "world")!
        assert(edit.updatedValue == "hello world")
        assert(edit.originalText == "🌍")
        let after = TargetSnapshot(value: "hello world", location: 11, length: 0)
        assert(edit.permitsUndo(current: after, committed: after, sameElement: true, editable: true))
        assert(!edit.permitsUndo(current: .init(value: "hello world!", location: 12, length: 0), committed: after, sameElement: true, editable: true))
        assert(!edit.permitsUndo(current: after, committed: after, sameElement: false, editable: true))
        assert(!edit.permitsUndo(current: after, committed: after, sameElement: true, editable: false))
        assert(!edit.permitsUndo(current: .init(value: "hello world", location: 0, length: 0), committed: after, sameElement: true, editable: true))
        assert(!edit.permitsUndo(current: saved, committed: saved, sameElement: true, editable: true), "Failed insertion must not create an undo receipt")
        let insertion = TextEditReceipt(original: .init(value: "ab", location: 1, length: 0), replacement: "🌍")!
        assert(insertion.updatedValue == "a🌍b" && insertion.originalText.isEmpty && insertion.replacementLength == 2)
        assert(TextEditReceipt(original: invalid, replacement: "x") == nil)
        print("Native target, hold, and Undo contracts passed")
    }
}
