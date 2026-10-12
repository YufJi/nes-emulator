import Foundation

/// A NES console running on its own thread.
///
/// Everything that touches the core happens on one serial queue: that is the
/// single-owner model the core documents (`docs/architecture.md` §10.5). Audio
/// is pulled here and pushed into ``AudioRing`` for the device callback, and the
/// picture is handed out as a zero-copy ``FrameSource``.
///
/// Pacing comes from the core's own `FrameClock`, so a PAL cartridge runs at its
/// own rate instead of a hardcoded 60 Hz. Either let ``start()`` drive it, or
/// tick from the platform's display link with ``tick(nowMs:)``.
public final class Emulator {
    /// Audio produced by the emulation thread; hand it to the device callback.
    public let audio: AudioRing

    /// Called on the emulation queue, with the latest picture.
    ///
    /// Upload it right there (Metal is happy to be driven from any thread) or
    /// hop to the main queue for UI work — but do not keep the view: the pixels
    /// belong to the console and change on the next frame.
    public var onFrame: ((FrameSource) -> Void)?

    /// Called when the core panics and the console has been thrown away.
    ///
    /// The machine is left mid-instruction, so the only way on is to load
    /// another cartridge.
    public var onFatalError: ((String) -> Void)?

    /// Called when the measured frame rate moves by half a frame or more.
    public var onFramesPerSecond: ((Double) -> Void)?

    private let config: Config
    private let queue = DispatchQueue(label: "nes.emulation", qos: .userInteractive)
    private var nes: Nes?
    private var clock: FrameClock?
    private var running = false
    /// Set when the console has trapped; cleared by the next ``load(rom:saveRam:)``.
    private var spent = false
    private var lastFps = 0.0

    /// - Parameters:
    ///   - config: host settings; `sampleRate` should match the audio device.
    ///   - audioCapacitySamples: ring size in floats (frames × channels).
    public init(config: Config = defaultConfig(), audioCapacitySamples: Int = 48_000 * 2) {
        self.config = config
        self.audio = AudioRing(capacitySamples: audioCapacitySamples)
    }

    // MARK: - Lifecycle

    /// Load a cartridge and reset the console.
    ///
    /// `saveRam` is written back before the first frame runs: boot code can read
    /// its save that early, and a frame emulated against blank SRAM would look
    /// like "no save" to the game.
    public func load(rom: Data, saveRam: Data? = nil) throws {
        try queue.sync {
            let next = Nes.withConfig(config: config)
            try next.loadRom(data: rom)
            if let saveRam {
                next.loadSaveRam(data: saveRam)
            }

            nes = next
            clock = FrameClock(frameRate: next.frameRate())
            spent = false
            lastFps = 0
            audio.flush()

            // One frame up front so the first picture is not blank, and so a
            // cartridge that panics immediately is reported as a load failure.
            try next.frame()
        }
    }

    /// Reset the console, as if the reset button was pressed.
    public func reset() throws {
        try queue.sync {
            guard let nes else { return }
            try nes.reset()
            clock?.reset()
            audio.flush()
            try nes.frame()
        }
    }

    /// Start pacing on the emulation queue.
    public func start() {
        queue.async {
            guard !self.running, self.nes != nil else { return }
            self.running = true
            self.clock?.reset()
            while self.running {
                self.step(at: Self.monotonicMilliseconds())
                if self.running {
                    // Nothing may be due yet; sleep instead of spinning. A host
                    // with a display link should call `tick(nowMs:)` instead.
                    Thread.sleep(forTimeInterval: 0.002)
                }
            }
        }
    }

    /// Stop pacing. The console keeps its state.
    public func stop() {
        queue.sync { running = false }
    }

    /// Emulate whatever is due at `nowMs` (a monotonic timestamp in
    /// milliseconds), for hosts that own the timing — `CADisplayLink` on iOS,
    /// `CVDisplayLink` on macOS. Use this *or* ``start()``, not both.
    public func tick(nowMs: Double) {
        queue.async { self.step(at: nowMs) }
    }

    // MARK: - State

    /// Whether a cartridge is loaded and the console has not trapped.
    public var isLoaded: Bool {
        queue.sync { nes != nil && !spent }
    }

    /// Frame rate of the loaded cartridge, in Hz.
    public var frameRate: Double? {
        queue.sync { nes?.frameRate() }
    }

    /// Retarget the core's resampler, e.g. when the audio route changes to a
    /// device that runs at another rate. Queued audio is dropped; prefer setting
    /// the rate through `Config` at startup.
    public func setSampleRate(_ rate: UInt32) {
        queue.sync { nes?.setSampleRate(sampleRate: rate) }
    }

    /// The current picture, sized from the console.
    ///
    /// Zero-copy and subject to the same lifetime rule: while you hold it, do
    /// not emulate (i.e. do not call ``start()`` or ``tick(nowMs:)``).
    public func currentFrame() -> FrameSource? {
        queue.sync { nes.map(Self.frameSource) }
    }

    // MARK: - Input and saves

    /// Press or release a button; use the indices in ``Button``.
    public func setButton(_ button: UInt8, pressed: Bool, player: UInt8 = 1) {
        queue.sync { nes?.setButton(player: player, button: button, pressed: pressed) }
    }

    /// Release every button, e.g. when the app goes to the background.
    public func releaseAllButtons() {
        queue.sync {
            for button in NesButton.all {
                nes?.setButton(player: 1, button: button, pressed: false)
                nes?.setButton(player: 2, button: button, pressed: false)
            }
        }
    }

    /// Snapshot of the cartridge's battery RAM, or `nil` when it has none.
    public func saveRam() -> Data? {
        queue.sync { nes?.saveRam() }
    }

    /// Serialize the whole machine.
    public func saveState() -> Data? {
        queue.sync { nes?.saveState() }
    }

    /// Restore a state produced by ``saveState()``.
    public func loadState(_ state: Data) {
        queue.sync { nes?.loadState(data: state) }
    }

    // MARK: - Internals

    /// One pacing step: everything here runs on the emulation queue.
    private func step(at nowMs: Double) {
        guard let nes, let clock, !spent else { return }

        let due = clock.framesDue(nowMs: nowMs)
        guard due > 0 else { return }

        do {
            for _ in 0..<due {
                try nes.frame()
            }
        } catch {
            // The core trapped mid-instruction; that console is spent.
            spent = true
            running = false
            audio.flush()
            onFatalError?("\(error)")
            return
        }

        if let onFrame {
            onFrame(Self.frameSource(nes))
        }
        pumpAudio(from: nes)
        reportFps(from: clock)
    }

    /// Read the picture straight out of the core's memory.
    private static func frameSource(_ nes: Nes) -> FrameSource {
        // Re-read the pointer every frame rather than caching it: the buffer is
        // the core's, and `loadState` may move it.
        let count = Int(nes.frameBufferLen()) * MemoryLayout<UInt32>.size
        let base = UnsafeRawPointer(bitPattern: UInt(nes.frameBufferPtr()))
        return FrameSource(
            width: Int(nes.frameWidth()),
            height: Int(nes.frameHeight()),
            pixels: UnsafeRawBufferPointer(start: base, count: base == nil ? 0 : count),
        )
    }

    /// Move whatever the frames produced into the ring the device drains.
    private func pumpAudio(from nes: Nes) {
        let samples = nes.fillAudio(maxSamples: 4096)
        guard !samples.isEmpty else { return }
        samples.withUnsafeBufferPointer { buffer in
            guard let base = buffer.baseAddress else { return }
            audio.write(base, count: buffer.count)
        }
    }

    private func reportFps(from clock: FrameClock) {
        let fps = clock.fps()
        guard fps > 0, abs(fps - lastFps) >= 0.5 else { return }
        lastFps = fps
        onFramesPerSecond?(fps)
    }

    private static func monotonicMilliseconds() -> Double {
        Double(DispatchTime.now().uptimeNanoseconds) / 1_000_000
    }
}

/// Controller button indices, straight from the core so host and core cannot
/// drift apart.
///
/// Named `NesButton` rather than `Button` so it does not shadow SwiftUI's.
public enum NesButton {
    public static let a = buttonA()
    public static let b = buttonB()
    public static let select = buttonSelect()
    public static let start = buttonStart()
    public static let up = buttonUp()
    public static let down = buttonDown()
    public static let left = buttonLeft()
    public static let right = buttonRight()

    /// Every button, for "let go of everything" paths.
    public static let all: [UInt8] = [a, b, select, start, up, down, left, right]
}
