import AVFoundation
import Foundation
import Speech

func emit(_ dict: [String: Any]) {
    if let data = try? JSONSerialization.data(withJSONObject: dict),
       let line = String(data: data, encoding: .utf8) {
        print(line)
        fflush(stdout)
    }
}

guard let recognizer = SFSpeechRecognizer(), recognizer.isAvailable else {
    emit(["event": "error", "message": "Speech recognizer is not available on this Mac."])
    exit(1)
}

let semaphore = DispatchSemaphore(value: 0)
var authStatus: SFSpeechRecognizerAuthorizationStatus = .notDetermined
SFSpeechRecognizer.requestAuthorization { status in
    authStatus = status
    semaphore.signal()
}
semaphore.wait()

guard authStatus == .authorized else {
    emit(["event": "error", "message": "Speech recognition permission denied. Enable it in System Settings → Privacy & Security → Speech Recognition."])
    exit(1)
}

let engine = AVAudioEngine()
let request = SFSpeechAudioBufferRecognitionRequest()
request.shouldReportPartialResults = true
if #available(macOS 13.0, *) {
    request.addsPunctuation = true
}

let inputNode = engine.inputNode
let format = inputNode.outputFormat(forBus: 0)
inputNode.installTap(onBus: 0, bufferSize: 1024, format: format) { buffer, _ in
    request.append(buffer)
}

engine.prepare()
do {
    try engine.start()
} catch {
    emit(["event": "error", "message": "Could not start microphone: \(error.localizedDescription)"])
    exit(1)
}

emit(["event": "ready"])

var latestText = ""
let task = recognizer.recognitionTask(with: request) { result, error in
    if let result = result {
        latestText = result.bestTranscription.formattedString
        emit([
            "event": result.isFinal ? "final" : "partial",
            "text": latestText
        ])
        if result.isFinal {
            engine.stop()
            inputNode.removeTap(onBus: 0)
            exit(0)
        }
    }
    if let error = error {
        if !latestText.isEmpty {
            emit(["event": "final", "text": latestText])
            exit(0)
        }
        emit(["event": "error", "message": error.localizedDescription])
        exit(1)
    }
}

// Stop gracefully when parent closes stdin.
DispatchQueue.global(qos: .utility).async {
    while let _ = readLine() {}
    engine.stop()
    inputNode.removeTap(onBus: 0)
    request.endAudio()
    DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) {
        if !latestText.isEmpty {
            emit(["event": "final", "text": latestText])
        }
        task.cancel()
        exit(0)
    }
}

RunLoop.main.run()
