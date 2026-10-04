import Foundation
import AVFoundation

private let lock = NSLock()
private var events: [[String: Any]] = []
private let done = DispatchSemaphore(value: 0)
func emit(_ event: [String: Any]) {
    lock.lock(); events.append(event); lock.unlock()
    if event["phase"] as? String == "whisper_finish" { done.signal() }
}
@main struct WhisperCaptureTests {
    static func main() throws {
        let format = AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: 48_000, channels: 1, interleaved: false)!
        let capture = try WhisperAudioCapture(format: format, id: 42)
        let count = 48_000 * 3 + 17
        for offset in stride(from: 0, to: count, by: 1024) {
            let length = min(1024, count - offset)
            let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: AVAudioFrameCount(length))!
            buffer.frameLength = AVAudioFrameCount(length)
            for index in 0..<length { buffer.floatChannelData![0][index] = sin(Float(offset + index) * 0.02) * 0.2 }
            capture.append(buffer)
        }
        capture.finish()
        precondition(done.wait(timeout: .now() + 5) == .success, "Capture never finished")
        lock.lock(); let recorded = events; lock.unlock()
        precondition(recorded.last?["phase"] as? String == "whisper_finish")
        precondition(recorded.allSatisfy { $0["id"] as? UInt64 == 42 })
        var samples: [Float] = []
        for event in recorded.dropLast() {
            precondition(event["phase"] as? String == "whisper_audio")
            let data = Data(base64Encoded: event["audio"] as! String)!
            precondition(data.count % 4 == 0)
            samples += data.withUnsafeBytes { Array($0.bindMemory(to: Float.self)) }
        }
        precondition(abs(samples.count - count / 3) <= 16, "Resampling lost frames: \(samples.count)")
        precondition(samples.allSatisfy { $0.isFinite })
        precondition(samples.contains { abs($0) > 0.1 }, "Conversion lost the signal")
        let canceled = try WhisperAudioCapture(format: format, id: 43)
        canceled.cancel()
        canceled.append(AVAudioPCMBuffer(pcmFormat: format, frameCapacity: 1024)!)
        canceled.finish()
        Thread.sleep(forTimeInterval: 0.1)
        lock.lock(); let afterCancel = events; lock.unlock()
        precondition(afterCancel.count == recorded.count, "Canceled capture emitted late frames")
        print("whisper capture passed: \(samples.count) samples, final flush, cancellation")
    }
}
