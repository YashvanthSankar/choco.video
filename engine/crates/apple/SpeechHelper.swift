// choco-apple-speech: a tiny helper that runs Apple's SpeechAnalyzer on a WAV file.
// Usage:
//   choco-apple-speech status
//   choco-apple-speech transcribe <file.wav> <locale>
// Progress and errors go to stderr as JSON lines. The transcript goes to stdout as JSON
// matching choco-engine's `Transcript` type. Built by choco-engine's build.rs.

import AVFoundation
import Foundation
import Speech

struct WordOut: Encodable { let start_ms: UInt64; let end_ms: UInt64; let text: String; let confidence: Float }
struct SegmentOut: Encodable { let start_ms: UInt64; let end_ms: UInt64; let text: String; let words: [WordOut] }
struct TranscriptOut: Encodable { let language: String; let engine: String; let segments: [SegmentOut] }
struct StatusOut: Encodable { let available: Bool; let os: String; let locales: [String]; let installed_locales: [String] }

let stderr = FileHandle.standardError

func emit(_ obj: [String: Any]) {
    if let data = try? JSONSerialization.data(withJSONObject: obj), let s = String(data: data, encoding: .utf8) {
        stderr.write((s + "\n").data(using: .utf8)!)
    }
}

func fail(_ message: String) -> Never {
    emit(["error": message])
    exit(1)
}

func ms(_ t: CMTime) -> UInt64 {
    let s = CMTimeGetSeconds(t)
    return s.isFinite && s > 0 ? UInt64((s * 1000).rounded()) : 0
}

func osVersion() -> String {
    let v = ProcessInfo.processInfo.operatingSystemVersion
    return "\(v.majorVersion).\(v.minorVersion).\(v.patchVersion)"
}

@available(macOS 26.0, *)
func status() async -> StatusOut {
    let supported = await SpeechTranscriber.supportedLocales.map { $0.identifier(.bcp47) }
    let installed = await SpeechTranscriber.installedLocales.map { $0.identifier(.bcp47) }
    return StatusOut(available: SpeechTranscriber.isAvailable, os: osVersion(), locales: supported.sorted(), installed_locales: installed.sorted())
}

@available(macOS 26.0, *)
func convert(_ result: SpeechTranscriber.Result) -> SegmentOut? {
    var words: [WordOut] = []
    for run in result.text.runs {
        let piece = String(result.text[run.range].characters)
        let trimmed = piece.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty { continue }
        let conf = Float(run.transcriptionConfidence ?? 1.0)
        guard let range = run.audioTimeRange else {
            // Punctuation without its own timing attaches to the previous word.
            if let last = words.popLast() {
                words.append(WordOut(start_ms: last.start_ms, end_ms: last.end_ms, text: last.text + trimmed, confidence: last.confidence))
            }
            continue
        }
        let start = ms(range.start)
        let end = max(ms(range.end), start + 1)
        // A run can hold several words; split evenly by character count when it does.
        let parts = trimmed.split(whereSeparator: { $0.isWhitespace }).map(String.init)
        if parts.count <= 1 {
            words.append(WordOut(start_ms: start, end_ms: end, text: trimmed, confidence: conf))
        } else {
            let total = parts.reduce(0) { $0 + $1.count }
            var t = start
            for (i, p) in parts.enumerated() {
                let share = UInt64(Double(end - start) * Double(p.count) / Double(max(total, 1)))
                let e = i == parts.count - 1 ? end : t + max(share, 1)
                words.append(WordOut(start_ms: t, end_ms: e, text: p, confidence: conf))
                t = e
            }
        }
    }
    guard let first = words.first, let last = words.last else { return nil }
    let text = String(result.text.characters).trimmingCharacters(in: .whitespacesAndNewlines)
    return SegmentOut(start_ms: first.start_ms, end_ms: last.end_ms, text: text, words: words)
}

@available(macOS 26.0, *)
func transcribe(path: String, localeId: String) async throws -> TranscriptOut {
    guard SpeechTranscriber.isAvailable else { fail("SpeechTranscriber is not available on this Mac") }
    guard let locale = await SpeechTranscriber.supportedLocale(equivalentTo: Locale(identifier: localeId)) else {
        fail("locale \(localeId) is not supported by Apple speech")
    }
    let transcriber = SpeechTranscriber(
        locale: locale,
        transcriptionOptions: [],
        reportingOptions: [],
        attributeOptions: [.audioTimeRange, .transcriptionConfidence]
    )

    if let request = try await AssetInventory.assetInstallationRequest(supporting: [transcriber]) {
        emit(["fraction": 0.0, "message": "downloading Apple speech assets for \(locale.identifier(.bcp47))"])
        let watcher = Task {
            while !Task.isCancelled {
                emit(["fraction": request.progress.fractionCompleted * 0.1, "message": "downloading Apple speech assets"])
                try? await Task.sleep(nanoseconds: 500_000_000)
            }
        }
        try await request.downloadAndInstall()
        watcher.cancel()
    }

    let file = try AVAudioFile(forReading: URL(fileURLWithPath: path))
    let totalMs = Double(file.length) / file.processingFormat.sampleRate * 1000
    let analyzer = SpeechAnalyzer(modules: [transcriber])

    let collector = Task { () throws -> [SegmentOut] in
        var segments: [SegmentOut] = []
        var lastReport = Date.distantPast
        for try await result in transcriber.results {
            guard let seg = convert(result) else { continue }
            segments.append(seg)
            if Date().timeIntervalSince(lastReport) > 0.5 {
                let f = totalMs > 0 ? min(0.1 + 0.9 * Double(seg.end_ms) / totalMs, 1.0) : -1
                emit(["fraction": f, "message": "transcribing"])
                lastReport = Date()
            }
        }
        return segments
    }

    if let last = try await analyzer.analyzeSequence(from: file) {
        try await analyzer.finalizeAndFinish(through: last)
    } else {
        await analyzer.cancelAndFinishNow()
    }
    let segments = try await collector.value
    emit(["fraction": 1.0, "message": "transcribed"])
    return TranscriptOut(language: locale.identifier(.bcp47), engine: "apple-speech", segments: segments)
}

@main
struct Main {
    static func main() async {
        let args = CommandLine.arguments
        guard args.count >= 2 else { fail("usage: choco-apple-speech status | transcribe <wav> <locale>") }
        guard #available(macOS 26.0, *) else {
            if args[1] == "status" {
                let out = StatusOut(available: false, os: osVersion(), locales: [], installed_locales: [])
                FileHandle.standardOutput.write(try! JSONEncoder().encode(out))
                exit(0)
            }
            fail("Apple speech needs macOS 26 or later; this Mac runs \(osVersion())")
        }
        do {
            let encoder = JSONEncoder()
            switch args[1] {
            case "status":
                FileHandle.standardOutput.write(try encoder.encode(await status()))
            case "transcribe" where args.count >= 4:
                let out = try await transcribe(path: args[2], localeId: args[3])
                FileHandle.standardOutput.write(try encoder.encode(out))
            default:
                fail("usage: choco-apple-speech status | transcribe <wav> <locale>")
            }
        } catch {
            fail("\(error)")
        }
    }
}
