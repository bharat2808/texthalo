import Foundation
import Speech
import AVFoundation

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

/// Capture adapter only: conversion and bounded frame transport stay off the audio callback.
/// The Rust worker owns recognition, model residency, and transcript assembly.
final class WhisperAudioCapture {
    private let queue = DispatchQueue(label: "com.kiegen.whisper-audio")
    private let converter: AVAudioConverter
    private let outputFormat: AVAudioFormat
    private let id: UInt64
    private var active = true

    init(format: AVAudioFormat, id: UInt64) throws {
        let target = AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: 16_000, channels: 1, interleaved: false)!
        guard let converter = AVAudioConverter(from: format, to: target) else {
            throw NSError(domain: "TextHalo", code: 1, userInfo: [NSLocalizedDescriptionKey: "Could not prepare the microphone for Whisper."])
        }
        self.converter = converter; self.outputFormat = target; self.id = id
    }
    func append(_ buffer: AVAudioPCMBuffer) {
        guard let copy = AVAudioPCMBuffer(pcmFormat: buffer.format, frameCapacity: buffer.frameLength) else { return }
        copy.frameLength = buffer.frameLength
        let source = UnsafeMutableAudioBufferListPointer(buffer.mutableAudioBufferList)
        let destination = UnsafeMutableAudioBufferListPointer(copy.mutableAudioBufferList)
        for (from, to) in zip(source, destination) {
            if let input = from.mData, let output = to.mData { memcpy(output, input, Int(from.mDataByteSize)) }
        }
        queue.async { [self] in
            guard active else { return }
            let capacity = AVAudioFrameCount(ceil(Double(copy.frameLength) * 16_000 / copy.format.sampleRate)) + 128
            guard let output = AVAudioPCMBuffer(pcmFormat: outputFormat, frameCapacity: capacity) else { return }
            var supplied = false
            var error: NSError?
            let status = converter.convert(to: output, error: &error) { _, state in
                if supplied { state.pointee = .noDataNow; return nil }
                supplied = true; state.pointee = .haveData; return copy
            }
            if status == .error {
                active = false
                emit(["id": id, "phase": "whisper_capture_error", "message": "Could not convert microphone audio."])
            } else { send(output) }
        }
    }
    private func send(_ output: AVAudioPCMBuffer) {
        guard output.frameLength > 0, let samples = output.floatChannelData?[0] else { return }
        let data = Data(bytes: samples, count: Int(output.frameLength) * MemoryLayout<Float>.size)
        emit(["id": id, "phase": "whisper_audio", "audio": data.base64EncodedString()])
    }
    func finish() {
        queue.async { [self] in
            guard active else { return }
            // Flush the resampler before finalizing, including the last fractional frame.
            if let output = AVAudioPCMBuffer(pcmFormat: outputFormat, frameCapacity: 1024) {
                var error: NSError?
                _ = converter.convert(to: output, error: &error) { _, state in state.pointee = .endOfStream; return nil }
                send(output)
            }
            active = false
            emit(["id": id, "phase": "whisper_finish"])
        }
    }
    func cancel() { queue.async { [self] in active = false } }
}
