import Foundation
import os.lock

/// A float ring buffer shared between the emulation thread and the audio
/// callback.
///
/// The core is deliberately single-owner: audio is pulled on the emulation
/// thread with `fillAudio` and handed to the device here, which is the model
/// `docs/architecture.md` §10.5 describes. The producer never blocks; if the
/// consumer falls behind, the oldest samples are dropped, because stale audio is
/// worse than a gap.
///
/// The critical section is a `memcpy` under an `os_unfair_lock`. That is fine at
/// this scale and keeps the kit free of dependencies, but a production app would
/// replace it with an atomic head/tail — Apple asks render callbacks not to take
/// locks at all.
public final class AudioRing {
    private var storage: UnsafeMutablePointer<Float>
    private let capacitySamples: Int
    private var head = 0 // next write
    private var tail = 0 // next read
    private var lock = os_unfair_lock()

    /// - Parameter capacitySamples: total floats (frames × channels).
    public init(capacitySamples: Int) {
        self.capacitySamples = max(1, capacitySamples)
        self.storage = .allocate(capacity: self.capacitySamples)
        self.storage.initialize(repeating: 0, count: self.capacitySamples)
    }

    deinit {
        storage.deinitialize(count: capacitySamples)
        storage.deallocate()
    }

    /// Samples waiting to be played.
    public var availableSamples: Int {
        os_unfair_lock_lock(&lock)
        defer { os_unfair_lock_unlock(&lock) }
        return (head - tail + capacitySamples) % capacitySamples
    }

    /// Drop everything queued, e.g. when a cartridge is loaded.
    public func flush() {
        os_unfair_lock_lock(&lock)
        head = 0
        tail = 0
        os_unfair_lock_unlock(&lock)
    }

    /// Append samples from the emulation thread, discarding the oldest ones when
    /// the consumer has fallen behind.
    public func write(_ samples: UnsafePointer<Float>, count: Int) {
        guard count > 0 else { return }
        os_unfair_lock_lock(&lock)
        defer { os_unfair_lock_unlock(&lock) }

        // Keep the newest audio: drop from the tail until this fits.
        let overflow = count - capacitySamples
        if overflow > 0 {
            tail = (tail + overflow) % capacitySamples
        }
        for index in 0..<min(count, capacitySamples) {
            let sample = samples[index + max(0, overflow)]
            storage[head] = sample
            head = (head + 1) % capacitySamples
        }
    }

    /// Take up to `count` samples on the audio thread; returns how many were
    /// written into `out`, which is left untouched beyond that.
    @discardableResult
    public func read(into out: UnsafeMutablePointer<Float>, count: Int) -> Int {
        os_unfair_lock_lock(&lock)
        defer { os_unfair_lock_unlock(&lock) }

        let available = (head - tail + capacitySamples) % capacitySamples
        let take = min(available, count)
        for index in 0..<take {
            out[index] = storage[tail]
            tail = (tail + 1) % capacitySamples
        }
        return take
    }
}
