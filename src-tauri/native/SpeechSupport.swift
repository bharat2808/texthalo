import Foundation
import Speech

func recognitionLanguages() -> [[String: Any]] {
    SFSpeechRecognizer.supportedLocales().map { locale in
        return ["code": locale.identifier.replacingOccurrences(of: "_", with: "-"),
                "name": Locale.current.localizedString(forIdentifier: locale.identifier) ?? locale.identifier] as [String: Any]
    }.sorted { ($0["name"] as! String).localizedStandardCompare($1["name"] as! String) == .orderedAscending }
}

func localRecognizer(language: String) throws -> SFSpeechRecognizer {
    guard SFSpeechRecognizer.authorizationStatus() == .authorized else {
        throw NSError(domain: "TextHalo", code: 1, userInfo: [NSLocalizedDescriptionKey: "Enable Speech Recognition permission first."])
    }
    guard let recognizer = SFSpeechRecognizer(locale: Locale(identifier: language)),
          recognizer.isAvailable, recognizer.supportsOnDeviceRecognition else {
        throw NSError(domain: "TextHalo", code: 2, userInfo: [NSLocalizedDescriptionKey: "On-device speech recognition is unavailable for this language. Check macOS speech language assets."])
    }
    return recognizer
}
