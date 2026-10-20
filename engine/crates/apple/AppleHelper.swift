// choco-apple: a tiny helper exposing Apple's on-device frameworks to choco-engine.
// Usage:
//   choco-apple status
//   choco-apple transcribe <file.wav> <locale>        SpeechAnalyzer, word-timed transcript
//   choco-apple generate < request.json                Foundation Models (Apple Intelligence)
// Progress and errors go to stderr as JSON lines; results go to stdout as JSON.
// Built by choco-engine's build.rs. Runs as a child process so a crash here cannot take the app down.

import AVFoundation
import Foundation
import FoundationModels
import Speech

struct WordOut: Encodable { let start_ms: UInt64; let end_ms: UInt64; let text: String; let confidence: Float }
struct SegmentOut: Encodable { let start_ms: UInt64; let end_ms: UInt64; let text: String; let words: [WordOut] }
struct TranscriptOut: Encodable { let language: String; let engine: String; let segments: [SegmentOut] }
struct StatusOut: Encodable { let available: Bool; let os: String; let locales: [String]; let installed_locales: [String]; let llm_available: Bool; let llm_status: String }
struct GenerateRequest: Decodable { let mode: String; let instructions: String; let prompt: String }

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
    let llm = SystemLanguageModel.default.availability
    var llmOk = false
    if case .available = llm { llmOk = true }
    return StatusOut(available: SpeechTranscriber.isAvailable, os: osVersion(), locales: supported.sorted(), installed_locales: installed.sorted(), llm_available: llmOk, llm_status: "\(llm)")
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
    // The OS keeps at most five locales' assets reserved per user, and reservations persist across
    // processes. A sixth language fails with "Too many allocated locales"; release the others first
    // (their assets re-download on demand, usually from cache).
    let reserved = await AssetInventory.reservedLocales
    if reserved.count >= 5, !reserved.contains(where: { $0.identifier(.bcp47) == locale.identifier(.bcp47) }) {
        for old in reserved where old.identifier(.bcp47) != locale.identifier(.bcp47) {
            await AssetInventory.release(reservedLocale: old)
        }
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

// MARK: Foundation Models

@available(macOS 26.0, *)
@Generable
struct GenScore: Encodable {
    @Guide(description: "The candidate id exactly as given, for example c12")
    var id: String
    @Guide(description: "How likely the clip is to hold a viewer, 0 to 100", .range(0...100))
    var score: Int
    @Guide(description: "One to three short phrases explaining the score")
    var reasons: [String]
    @Guide(description: "Hook title for the clip, at most 60 characters, no emojis")
    var title: String
    @Guide(description: "One sentence for the post caption")
    var description: String
    @Guide(description: "Three to five hashtags, each starting with #")
    var hashtags: [String]
}

@available(macOS 26.0, *)
@Generable
struct GenScores: Encodable {
    @Guide(description: "Exactly one entry per candidate")
    var scores: [GenScore]
}

@available(macOS 26.0, *)
func generate() async throws {
    let input = FileHandle.standardInput.readDataToEndOfFile()
    let req = try JSONDecoder().decode(GenerateRequest.self, from: input)
    guard case .available = SystemLanguageModel.default.availability else {
        fail("unavailable: Apple Intelligence model is \(SystemLanguageModel.default.availability)")
    }
    let options = GenerationOptions(temperature: 0.2)
    do {
        if req.mode == "scores" {
            // Guided generation: output always matches the schema, default guardrails.
            let session = LanguageModelSession(model: SystemLanguageModel.default, instructions: req.instructions)
            let r = try await session.respond(to: req.prompt, generating: GenScores.self, options: options)
            FileHandle.standardOutput.write(try JSONEncoder().encode(r.content))
        } else {
            // Plain text, with guardrails meant for transforming user-supplied text (summaries,
            // or JSON-as-text scores). Podcasts discussing war, ethics or health trip the default
            // guardrails as false positives; the caller parses and validates the text strictly.
            let model = SystemLanguageModel(guardrails: .permissiveContentTransformations)
            let session = LanguageModelSession(model: model, instructions: req.instructions)
            let r = try await session.respond(to: req.prompt, options: options)
            FileHandle.standardOutput.write(try JSONEncoder().encode(["text": r.content]))
        }
    } catch let e as LanguageModelSession.GenerationError {
        switch e {
        case .guardrailViolation: fail("declined: guardrail")
        case .exceededContextWindowSize: fail("context: exceeded the on-device context window")
        case .refusal: fail("declined: refusal")
        default: fail("generation: \(e)")
        }
    } catch {
        let msg = "\(error)"
        fail(msg.localizedCaseInsensitiveContains("sensitive") ? "declined: \(msg)" : "generation: \(msg)")
    }
}

@main
struct Main {
    static func main() async {
        let args = CommandLine.arguments
        guard args.count >= 2 else { fail("usage: choco-apple status | transcribe <wav> <locale> | generate < request.json") }
        guard #available(macOS 26.0, *) else {
            if args[1] == "status" {
                let out = StatusOut(available: false, os: osVersion(), locales: [], installed_locales: [], llm_available: false, llm_status: "needs macOS 26")
                FileHandle.standardOutput.write(try! JSONEncoder().encode(out))
                exit(0)
            }
            fail("unavailable: needs macOS 26 or later; this Mac runs \(osVersion())")
        }
        do {
            let encoder = JSONEncoder()
            switch args[1] {
            case "status":
                FileHandle.standardOutput.write(try encoder.encode(await status()))
            case "generate":
                try await generate()
            case "transcribe" where args.count >= 4:
                let out = try await transcribe(path: args[2], localeId: args[3])
                FileHandle.standardOutput.write(try encoder.encode(out))
            default:
                fail("usage: choco-apple status | transcribe <wav> <locale> | generate < request.json")
            }
        } catch {
            fail("\(error)")
        }
    }
}
