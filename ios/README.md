# iOS App (NesApp)

[简体中文](./README.zh-CN.md) · **English**

A minimal iOS app you can just run: pick a `.nes` file and you get picture and sound, with a
virtual pad at the bottom of the screen.

```
ios/
  NesApp.xcodeproj/        Xcode project you can open directly (with a shared scheme)
  NesApp/
    NesApp.swift           App entry + UI (Metal picture + top bar + virtual pad)
    Emulator.swift         Host wrapper: emulation thread, FrameClock driving, input, saves
    AudioRing.swift        Ring buffer between the emulation thread and the audio callback
    FrameSource.swift      Zero-copy frame view
    Platform/
      DisplayLinkDriver.swift  CADisplayLink driver
      MetalScreen.swift        MTKView + upload + aspect-fit scaling
      AudioOutput.swift        AVAudioEngine + route changes
    Generated/             UniFFI output (not committed; produced by build-core.sh)
  build-core.sh            Build the Rust static library + generate Swift bindings
  Artifacts/               Output: libnes_uniffi.a (not committed)
```

There is **not one line of emulation logic** in the app: CPU/PPU/APU/mappers all live in
`crates/nes-core`, exposed as a Swift API through `crates/nes-uniffi`. This layer only does
what a host should.

## Running it

```bash
rustup target add aarch64-apple-ios aarch64-apple-ios-sim   # once
open ios/NesApp.xcodeproj                                   # pick a simulator/device, Run
```

The project has a **Build Rust core** build phase that calls `ios/build-core.sh` on every
build: it picks the Rust target from `$PLATFORM_NAME` / `$ARCHS`, runs `cargo build`,
generates the Swift bindings, and puts the `.a` in `Artifacts/`. So a freshly cloned
repository needs no manual preparation steps.

Command-line builds (the two verified here):

```bash
xcodebuild -project ios/NesApp.xcodeproj -scheme NesApp \
  -sdk iphonesimulator -configuration Debug -arch arm64 \
  -derivedDataPath /tmp/nes-dd build

xcodebuild -project ios/NesApp.xcodeproj -scheme NesApp \
  -sdk iphoneos -configuration Debug -arch arm64 \
  -derivedDataPath /tmp/nes-dd-dev build
```

`-arch arm64` must be given explicitly: with multiple matching destinations, xcodebuild
defaults to picking the x86_64 simulator, while the Rust side is only built for arm64 (for a
universal simulator library, combine arm64 + x86_64 with `lipo`; for actual distribution,
make an `.xcframework`).

## Why an iOS Rust target is needed

UniFFI is a **binding generator**: it produces two halves — Rust-side scaffolding (compiled
together with `nes-uniffi`) and the Swift-side API plus C header. The `uniffi_nes_uniffi_*`
symbols Swift calls are C symbols and must be provided by a Rust static library **compiled
for iOS**:

- Real device: `cargo build --target aarch64-apple-ios`
- Apple Silicon simulator: `--target aarch64-apple-ios-sim` (the simulator is a separate
  target, not macOS)

Handing a macOS-built `.a` to an iOS project will be rejected by the linker, with neither
the minimum OS version nor the platform matching. It is the same thing as the web needing
`cargo build --target wasm32-unknown-unknown` first.

## Three conventions

**Threading**: the core is single-owner (`docs/architecture.md` §10.5). `Emulator` puts
every core call on one serial queue, and `onFrame` is called back on that same queue —
which is exactly right for doing Metal uploads directly. Anything that touches UI hops to
the main thread.

**The zero-copy boundary**: `FrameSource.pixels` points straight at the core's own frame
buffer. No intermediate array, no per-pixel computation; `MetalScreen` uploads from it
directly with `replace(region:)`. Two things to watch:

- Re-fetch the pointer every frame (`loadState` may move the buffer), and don't keep a
  `FrameSource`, or a texture built from it, into the next frame;
- The remaining "upload to texture" is inherent: true zero-upload would need the core to
  render into GPU-mapped memory (`IOSurface`) that is page-aligned — that is future work.

**Audio**: `AudioRing` is the only bridge between producer and consumer (the emulation
thread writes, the device callback reads). The critical section is one `os_unfair_lock`
plus one `memcpy`; good enough, but production should switch to atomic head/tail, because
Apple expects the render callback to be entirely lock-free. The sample rate is handed from
`AVAudioSession` to the core, and resynced when the route changes (Bluetooth/headphones).

## Not done yet

- A universal emulator library (`.xcframework`) and code signing configuration (currently
  `CODE_SIGNING_ALLOWED = NO`, only for build verification).
- Picture filters (scanlines/palette) and integer-scaling settings.
- Battery saves to disk (`Emulator.saveRam()` already exists, it just needs wiring to
  `Documents/`) and save-state UI.
- Turbo buttons, four-player adapter, light gun.

## Disclaimer

The app is part of a personal, non-commercial study project: it exists to learn, ships no
ROM or game files, and is not affiliated with Nintendo. "关于" in the top bar shows the same
notice inside the app, and the full statement — covering the repository, the web UI, and
this app — is in [`../DISCLAIMER.md`](../DISCLAIMER.md).
