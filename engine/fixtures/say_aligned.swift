// Synthesize speech with exact word onsets, for scoring transcription timing.
// Usage: swift say_aligned.swift <text-file> <out.caf> <out.words.json>
// Uses AVSpeechSynthesizer word markers, which carry the audio offset of every word start.
import AVFoundation
import Foundation

let args = CommandLine.arguments
guard args.count >= 4 else { print("usage: say_aligned <text> <out.caf> <out.json> [voice-language, default en-US]"); exit(2) }
let text = try! String(contentsOfFile: args[1], encoding: .utf8).trimmingCharacters(in: .whitespacesAndNewlines)
let outURL = URL(fileURLWithPath: args[2])

let synth = AVSpeechSynthesizer()
let utt = AVSpeechUtterance(string: text)
utt.voice = AVSpeechSynthesisVoice(language: args.count >= 5 ? args[4] : "en-US")

var file: AVAudioFile?
var sampleRate = 0.0
var bytesPerFrame = 0
var markers: [(offset: Int, loc: Int, len: Int)] = []
var totalFrames: AVAudioFramePosition = 0
// Marker offsets restart at each internal synthesis chunk; an empty buffer marks the boundary.
var boundaries: [AVAudioFramePosition] = []
var done = false

// An empty buffer can arrive mid-utterance; only the delegate's finish callback means done.
final class Finish: NSObject, AVSpeechSynthesizerDelegate {
    func speechSynthesizer(_ s: AVSpeechSynthesizer, didFinish u: AVSpeechUtterance) { done = true }
}
let finish = Finish()
synth.delegate = finish

synth.write(utt, toBufferCallback: { buffer in
    guard let pcm = buffer as? AVAudioPCMBuffer, pcm.frameLength > 0 else { boundaries.append(totalFrames); return }
    if file == nil {
        sampleRate = pcm.format.sampleRate
        bytesPerFrame = Int(pcm.format.streamDescription.pointee.mBytesPerFrame)
        file = try! AVAudioFile(forWriting: outURL, settings: pcm.format.settings,
                                commonFormat: pcm.format.commonFormat, interleaved: pcm.format.isInterleaved)
    }
    try! file!.write(from: pcm)
    totalFrames += AVAudioFramePosition(pcm.frameLength)
}, toMarkerCallback: { ms in
    for m in ms where m.mark == .word {
        markers.append((Int(m.byteSampleOffset), m.textRange.location, m.textRange.length))
    }
})
let deadline = Date().addingTimeInterval(120)
while !done && Date() < deadline { RunLoop.main.run(until: Date().addingTimeInterval(0.05)) }
// Let trailing buffers land after the finish callback.
RunLoop.main.run(until: Date().addingTimeInterval(0.5))
file = nil

// byteSampleOffset is documented as a byte offset; convert using the buffer format.
let ns = text as NSString
let totalMs = Double(totalFrames) / sampleRate * 1000
var absFrames: [Double] = []
var chunk = 0
for (i, m) in markers.enumerated() {
    if i > 0 && m.offset < markers[i - 1].offset { chunk += 1 }
    let base = chunk > 0 && chunk - 1 < boundaries.count ? Double(boundaries[chunk - 1]) : 0
    absFrames.append(base + Double(m.offset) / Double(max(bytesPerFrame, 1)))
}
var words: [[String: Any]] = []
for (i, m) in markers.enumerated() {
    let startMs = absFrames[i] / sampleRate * 1000
    let endMs = i + 1 < markers.count ? absFrames[i + 1] / sampleRate * 1000 : totalMs
    words.append(["text": ns.substring(with: NSRange(location: m.loc, length: m.len)), "start_ms": Int(startMs.rounded()), "next_start_ms": Int(endMs.rounded())])
}
let out: [String: Any] = ["sample_rate": sampleRate, "bytes_per_frame": bytesPerFrame, "duration_ms": Int(totalMs), "words": words]
try! JSONSerialization.data(withJSONObject: out, options: [.prettyPrinted]).write(to: URL(fileURLWithPath: args[3]))
print("wrote \(words.count) word onsets, \(Int(totalMs)) ms, \(sampleRate) Hz, \(bytesPerFrame) bytes/frame")
