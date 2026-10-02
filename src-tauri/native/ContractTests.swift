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
        print("Native target and hold contracts: 12 assertions passed")
    }
}
