# NES Emulator — Android TV

[简体中文](./README.zh-CN.md) · **English**

An Android TV front end for the Rust core: the same `nes-core` the web page and the iOS
app run, driven by a remote's D-pad or a gamepad, with a 1990s console interface. It talks
to the core through the Kotlin side of [`nes-uniffi`](../crates/nes-uniffi), so the four
host channels — timestamps in, picture out, buttons in, sound out — are the only things
this app implements.

- The interface is Chinese, like the iOS app's. The docs are English with a Chinese
  translation alongside.
- It ships **no ROM and no game data**. The game list is the web front end's own
  `web/public/games.json` copied into the APK at build time; a cartridge is downloaded
  from the link in that list when it is opened, and deleted when the session ends.

![The home screen: a red header, the recently played row, and a wall of cartridge-shaped tiles](../docs/img/android-home.png)

![The in-game menu over a paused game](../docs/img/android-menu.png)

The pictures are from an API 30 emulator; the interface is the same on a TV, where the
remote's D-pad does what the arrow keys do here.

## Requirements

- **JDK 17** — Android Studio's bundled one works:
  `export JAVA_HOME="/Applications/Android Studio.app/Contents/jbr/Contents/Home"`.
  `./gradlew` needs it before it can do anything.
- **Android SDK** with platform 34 and build-tools 34.0.0 (`ANDROID_HOME`, or
  `android/local.properties` with `sdk.dir=…`).
- **Android NDK 26.1.10909125** — `cargo build --target aarch64-linux-android` cannot link
  without it. `sdkmanager --install "ndk;26.1.10909125"` does it.
- **`cargo-ndk`** — `cargo install cargo-ndk --locked`. It is what points the linker at the
  NDK for the ABI being built, which `cargo build --target` alone cannot do.
- The pinned Rust toolchain, whose `rust-toolchain.toml` already lists the three Android
  targets; rustup installs them on the first cargo command.

## Commands

```bash
./android/build-core.sh                # Rust core + Kotlin bindings (both ABIs)
./android/build-core.sh arm64-v8a      # one ABI, for a quick check

cd android
./gradlew :app:assembleDebug           # the APK
./gradlew :app:testDebugUnitTest       # the app's own JVM tests
./gradlew :app:installDebug            # install on a connected device or emulator
```

`build-core.sh` writes two generated things, neither of which is committed:
`app/src/main/jniLibs/<abi>/libnes_uniffi.so` and the Kotlin API under
`app/src/main/java/uniffi/`. A fresh clone needs it once before Gradle can compile.

## ABIs

Four, because an APK with no matching ABI does not install at all — Android refuses it
rather than failing later:

| ABI | What runs it |
| --- | --- |
| `arm64-v8a` | current TV boxes and sets |
| `armeabi-v7a` | sets that run a 32-bit Android userspace on 64-bit silicon — Sony's BRAVIA line among them |
| `x86_64` | 64-bit emulator images |
| `x86` | Google's own Android TV system images, which are 32-bit up to API 31 |

`adb shell getprop ro.product.cpu.abilist` on the target says which one it needs; the list
lives in `build-core.sh` and in `abiFilters` in `app/build.gradle.kts`, and the two must
agree.

## Trying it on an emulator

```bash
export JAVA_HOME="/Applications/Android Studio.app/Contents/jbr/Contents/Home"
"$ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager" --install "system-images;android-30;android-tv;x86"
"$ANDROID_HOME/cmdline-tools/latest/bin/avdmanager" create avd -n nes-tv -k "system-images;android-30;android-tv;x86"
"$ANDROID_HOME/emulator/emulator" -avd nes-tv
```

A phone image works just as well for everything except the TV launcher itself — the
interface is D-pad driven either way. Over `adb`, `input keyevent KEYCODE_DPAD_*` drives the
focus and `KEYCODE_MENU` opens the settings or the in-game menu.

## How it is put together

| File | What it owns |
| --- | --- |
| `NesSession.kt` | The console and its thread: `Choreographer` on the main thread hands timestamps to a dedicated emulation thread, which runs frames, pushes sound into the ring and hands pictures to the surface |
| `VideoSurface.kt` | A `GLSurfaceView` in `WHEN_DIRTY` mode: one GLES2 texture, whole-multiple scaling, optional scanlines |
| `AudioOutput.kt` | An `AudioTrack` draining `AudioRing` on its own thread, at the device's own sample rate |
| `InputMapper.kt` | Key and joystick events to the console's eight buttons, turbo included |
| `AppState.kt` | The flow: which screen, what is downloading, which panel is over the game |
| `ui/` | The screens and the palette they draw from |
| `GameCatalog.kt`, `RomDownloader.kt`, `SaveStore.kt`, `Recents.kt`, `Settings.kt` | The catalogue, the one-off download, and what is kept on disk |

Three things are worth knowing before changing any of it:

- **The core is single-owner.** Every call into `Nes` happens on the emulation thread
  (`docs/architecture.md` §10.5); the audio thread only ever touches the ring, and the
  main thread only ever posts timestamps. `NesSession` is the only place that has a `Nes`.
- **The picture crosses as bytes.** Kotlin cannot dereference `frame_buffer_ptr()`, and
  `frame_buffer()` would marshal 61,440 pixels across the boundary as individual boxed
  integers, so the binding grew `frame_buffer_bytes()` for it: one `ByteArray` per frame,
  uploaded as `GL_RGBA` and read back with a `.bgr` swizzle in the fragment shader (the
  bytes are `0xAARRGGBB` little-endian, so a pixel arrives blue first).
- **Downloads are not a library.** A cartridge goes to one file in the cache and is deleted
  when the session ends; the battery save and the four save-state slots are keyed by the
  game's name and outlive it.

## Performance on a TV

Two decisions in this front end were worth real frames on the target hardware, and both were
measured on a Sony BRAVIA 4K VH22 (ARMv8 cores at 1.8 GHz, 32-bit Android userspace, 1080p):

- **The pacemaker is a timer on the emulation thread, not the display link.** That TV delivers
  a `Choreographer` callback every ~84 ms — one in five vsyncs — so `FrameClock` was handed a
  fifth of the timestamps, spent its four-frame catch-up budget on each of them and settled at
  a hard 50 fps. A timer is not the display's business to throttle, and the picture is
  presented on demand anyway, so pacing and presentation stay independent.
- **The core's PPU bus trace is switched off.** It writes a sample on every one of the 89,342
  dots in a frame, and this front end never reads it. Worth 5–8%.

What is left is the emulation itself:

| | Per frame |
| --- | --- |
| Emulation (`Nes::frame`) | ~13.4 ms |
| This front end (picture + audio plumbing) | ~1.4 ms |
| Budget at 60 Hz | 16.6 ms |

So a TV of that class runs NTSC at ~96% of real time — Super Mario Bros measures ~58 fps with
sound on. The wall is the core's per-dot PPU and CPU work, not this host; the same build on a
phone, a 64-bit box or an emulator has headroom, and a PAL cartridge (50 Hz) is comfortable.
Two things that did *not* help, worth recording so nobody tries them again: enabling
NEON/ARMv8 codegen for `armeabi-v7a` changed nothing measurable, and neither did turning the
core's audio capture off (the resampler is not what costs the time).

## Keys

| Pad | Console |
| --- | --- |
| D-pad, left stick, hat | D-pad |
| A, centre of the D-pad, Y | A |
| B, X | B |
| START | START |
| SELECT | SELECT |
| R1 / R2, L1 / L2 | turbo A, turbo B |
| MENU | settings, or the in-game menu |
| BACK | up a level: out of the menu, out of a game, out of the app |

The game list pages with the two buttons under the wall — `A` on one turns the page and
leaves focus there, so a run of pages is one button held under the thumb. `PAGE`/`CHANNEL`
keys do the same, a held direction does it four rows at a time, and the button that has run
out of pages is dimmed rather than hidden.

A remote is always player 1; gamepads take player 1 and then player 2 in the order they are
seen, so a two-player game needs nothing but a second pad.

## Not done yet

- Remapping the buttons (the settings screen has the read-only guide above instead).
- Importing a ROM the viewer already has: only the catalogue's links are supported, so
  playing needs a network.
- Low-latency native audio (Oboe/AAudio): this uses `AudioTrack`. The sample rate already
  follows the device.
- A touch interface. The layout is D-pad driven; a phone without a gamepad can navigate but
  cannot play comfortably.
- Release signing, shrinking and a Play/TV store manifest.

## Disclaimer

The app is part of a personal, non-commercial study project: it ships no ROM or game files,
downloads nothing on its own, and is not affiliated with Nintendo. "关于与免责声明" in the
settings shows the same statement inside the app, and the full text — covering the
repository, the web UI, the iOS app and this one — is in [`../DISCLAIMER.md`](../DISCLAIMER.md).
