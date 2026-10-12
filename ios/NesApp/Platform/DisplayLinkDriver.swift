#if os(iOS)
import QuartzCore

/// Drives an ``Emulator`` from the display's refresh — what an iOS app should do
/// instead of ``Emulator/start()``.
///
/// `CADisplayLink` reports the same kind of monotonic time `FrameClock` expects,
/// and `targetTimestamp` is when the frame will actually be shown, so pacing
/// compensates for the pipeline instead of trailing it.
public final class DisplayLinkDriver {
    private let emulator: Emulator
    private var link: CADisplayLink?

    public init(emulator: Emulator) {
        self.emulator = emulator
    }

    public func start() {
        guard link == nil else { return }
        let link = CADisplayLink(target: self, selector: #selector(tick(_:)))
        link.add(to: .main, forMode: .common)
        self.link = link
    }

    public func stop() {
        link?.invalidate()
        link = nil
    }

    deinit {
        link?.invalidate()
    }

    @objc private func tick(_ link: CADisplayLink) {
        emulator.tick(nowMs: link.targetTimestamp * 1000)
    }
}
#endif
