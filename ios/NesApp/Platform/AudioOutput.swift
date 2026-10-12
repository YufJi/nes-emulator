#if os(iOS)
import AVFoundation

/// Plays ``AudioRing`` through `AVAudioEngine`.
///
/// The render block only ever reads the ring — no allocation, no emulation — and
/// the emulation thread keeps it fed (see `docs/architecture.md` §10.5). When the
/// route changes (headphones, Bluetooth) the hardware rate can change with it, so
/// the new rate is handed to the core, which resamples for us.
public final class AudioOutput {
    private let engine = AVAudioEngine()
    private let emulator: Emulator
    private var sourceNode: AVAudioSourceNode?
    private var observer: NSObjectProtocol?
    /// Owned for the lifetime of the node: the render block writes into it, so it
    /// must outlive `start()`.
    private let scratch: UnsafeMutablePointer<Float>
    private let scratchSamples = 4096 * 2

    public init(emulator: Emulator) {
        self.emulator = emulator
        self.scratch = .allocate(capacity: scratchSamples)
        self.scratch.initialize(repeating: 0, count: scratchSamples)
    }

    /// Whether the graph is up and playing.
    public var isRunning: Bool { sourceNode != nil }

    deinit {
        stop()
        scratch.deinitialize(count: scratchSamples)
        scratch.deallocate()
    }

    /// Configure the session, build the graph and start playing.
    public func start() throws {
        guard sourceNode == nil else { return }

        let session = AVAudioSession.sharedInstance()
        try session.setCategory(.playback, mode: .default)
        try session.setActive(true)

        let rate = session.sampleRate
        emulator.setSampleRate(UInt32(rate))
        guard let format = AVAudioFormat(standardFormatWithSampleRate: rate, channels: 2) else {
            return
        }

        // The core emits interleaved stereo; `AVAudioSourceNode` hands us
        // de-interleaved buffers, so scatter as we copy.
        let scratch = self.scratch
        let capacity = scratchSamples
        let node = AVAudioSourceNode(format: format) { [weak self] _, _, frameCount, audioBufferList in
            let buffers = UnsafeMutableAudioBufferListPointer(audioBufferList)
            let wanted = Int(frameCount) * 2
            let got = self?.emulator.audio.read(into: scratch, count: min(wanted, capacity)) ?? 0

            for frame in 0..<Int(frameCount) {
                let index = frame * 2
                let left = index < got ? scratch[index] : 0
                let right = index + 1 < got ? scratch[index + 1] : 0
                if let channel = buffers[0].mData?.assumingMemoryBound(to: Float.self) {
                    channel[frame] = left
                }
                if buffers.count > 1, let channel = buffers[1].mData?.assumingMemoryBound(to: Float.self) {
                    channel[frame] = right
                }
            }
            return noErr
        }

        engine.attach(node)
        engine.connect(node, to: engine.mainMixerNode, format: format)
        try engine.start()
        sourceNode = node

        observer = NotificationCenter.default.addObserver(
            forName: AVAudioSession.routeChangeNotification,
            object: session,
            queue: .main,
        ) { [weak self] _ in
            guard let self else { return }
            // The hardware may now run at another rate; the core resamples to it.
            self.emulator.setSampleRate(UInt32(AVAudioSession.sharedInstance().sampleRate))
            self.emulator.audio.flush()
        }
    }

    public func stop() {
        if let observer {
            NotificationCenter.default.removeObserver(observer)
            self.observer = nil
        }
        engine.stop()
        if let sourceNode {
            engine.detach(sourceNode)
            self.sourceNode = nil
        }
    }
}
#endif
