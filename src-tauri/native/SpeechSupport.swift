import Foundation
import Speech

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

@MainActor final class FileRecognition {
    private var task: SFSpeechRecognitionTask?
    private var continuation: CheckedContinuation<String, Error>?
    private var timeout: Task<Void, Never>?
    func run(url: URL) async throws -> String {
        let recognizer = try localRecognizer(language: "en-US")
        let request = SFSpeechURLRecognitionRequest(url: url)
        request.requiresOnDeviceRecognition = true
        request.shouldReportPartialResults = false
        return try await withCheckedThrowingContinuation { continuation in
            self.continuation = continuation
            task = recognizer.recognitionTask(with: request) { [weak self] result, error in
                Task { @MainActor in
                    guard let self else { return }
                    if let result, result.isFinal { self.finish(.success(result.bestTranscription.formattedString)) }
                    else if let error { self.finish(.failure(error)) }
                }
            }
            timeout = Task {
                try? await Task.sleep(for: .seconds(25))
                guard !Task.isCancelled else { return }
                self.finish(.failure(NSError(domain: "TextHalo", code: 3, userInfo: [NSLocalizedDescriptionKey: "Local transcription timed out."])))
            }
        }
    }
    private func finish(_ result: Result<String, Error>) {
        guard let continuation else { return }
        self.continuation = nil
        timeout?.cancel(); timeout = nil
        task?.cancel(); task = nil
        continuation.resume(with: result)
    }
}
