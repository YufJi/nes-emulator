# Repository Guidelines

## Project Structure & Module Organization

- `crates/nes-core/` — the emulator core (Rust). Key modules: `src/cpu/`, `src/ppu/`
  (`rendering.rs`, `sprite.rs`), `src/apu/`, `src/mapper/` (one file per mapper),
  `src/bus.rs`, `src/nes.rs` (system wiring + public `Nes` API), `src/state.rs`
  (save-state encoding).
- `crates/nes-wasm/` — `wasm-bindgen` bindings; `src/lib.rs` is thin pass-throughs
  to `nes-core`.
- `web/` — React + Vite + TypeScript front end: `src/emulator/`, `src/hooks/`,
  `src/components/`. `src/wasm/` and `crates/nes-wasm/pkg/` are generated, not
  committed.
- `ios/` — the iOS app (Swift) and `ios/build-core.sh`, which cross-compiles the core
  and generates the Swift bindings into `ios/NesApp/Generated/` (not committed).
- `android/` — the Android TV app (Kotlin + Compose) and `android/build-core.sh`, which
  cross-compiles the core for three ABIs and generates the Kotlin bindings into
  `android/app/src/main/java/uniffi/` (not committed, like
  `app/src/main/jniLibs/`). Needs JDK 17, the NDK and `cargo-ndk`; see
  `android/README.md`.
- `roms/` — local test ROMs. Tests skip when a ROM is absent; never commit ROMs.

## Build, Test, and Development Commands

- `cargo test --workspace` — full Rust test suite (required before committing).
- `cargo test -p nes-core --lib` — core unit tests only.
- `cargo fmt --all` / `cargo clippy -p nes-core --all-targets` — format / lint.
- `cd web && npm install` — install front-end deps (Node 18+).
- `npm run dev` — dev server; `npm run build` — typecheck + production build.
- `npm run typecheck` — TypeScript only.
- `npm run build:games` — regenerate `web/public/games.json` (the game list shown by
  the "游戏列表" dialog) from a GitHub repository; needs network, no ROM data is stored.
- `npm run build:wasm` — regenerate `web/src/wasm` from `nes-wasm` (needs network
  and `wasm-pack`).
- `./android/build-core.sh` — cross-compile the core and generate the Kotlin bindings
  (needs the NDK and `cargo-ndk`); `cd android && ./gradlew :app:assembleDebug` — the APK
  (needs JDK 17); `./gradlew :app:testDebugUnitTest` — the app's JVM tests. Run
  `build-core.sh` before Gradle on a fresh clone: it produces what Gradle compiles against.

## Coding Style & Naming Conventions

- Rust: rustfmt defaults (4 spaces), edition 2021, MSRV 1.70. The toolchain itself
  is pinned in `rust-toolchain.toml` (1.97.0 + `rustfmt`/`clippy`); don't rely on
  a newer compiler than that pin. Use `snake_case` functions/fields, `CamelCase`
  types, `SCREAMING_SNAKE_CASE` consts.
- TypeScript/React: 2-space indent, `camelCase` values, `PascalCase.tsx`
  components. Keep comments explaining *why*, matching the surrounding style.
- Prefer minimal, focused diffs; do not reformat unrelated code.

## Testing Guidelines

- Rust `#[test]` functions named `test_<behavior>`; add a regression test with
  every fix. Integration suites live in `crates/nes-core/tests/`
  (`ppu.rs`, `apu.rs`, `mappers.rs`, `nestest.rs`).
- ROM-dependent tests are skipped or marked `#[ignore]` when `roms/` is empty.
- The accuracy baselines live in `crates/nes-core/tests/rom_tests.rs` (the
  blargg suites under `roms/blargg/`, `roms/accuracycoin/AccuracyCoin.nes`, and
  the `roms/nes-test-roms/` collection; layout and sources in
  `docs/architecture.md` §13.1). **That file and its harness (`tests/common/`,
  `tests/golden/`) are out of the tree for the MesenCE alignment pass** — see
  `docs/mesen-alignment.md` §6 — and come back with the rules they carried:
  ROMs are never committed, a new failure is a regression, and a baseline entry
  that starts passing must be removed from the list. While they are out, the
  loop is `cargo test --workspace` plus the `#[ignore]`d diagnostics in
  `tests/cycle_trace.rs`.
- For emulator changes, verify determinism (rewind/replay produces identical
  frames). For web changes, run `npm run build`.

## Commit & Pull Request Guidelines

- Follow Conventional Commits with an optional scope: `feat(apu): …`,
  `fix(mapper5): …`, `refactor: …`. Subjects are Chinese or English; history
  mixes both.
- Keep commits scoped, describe behavior changes, and state what you ran.
- PRs: summary, linked issue, test evidence, and screenshots/GIFs for rendering
  or UI changes. Call out the mapper and ROM used when touching cartridge code.

## Documentation Language

- Docs are English-first: `README.md`, `docs/architecture.md`, `web/README.md`,
  and `ios/README.md` are English.
- Keep a Chinese translation alongside each translated page as `*.zh-CN.md`
  (e.g. `README.zh-CN.md`, `docs/architecture.zh-CN.md`).
- When you edit one language, update the other in the same change, and keep the
  language switcher line at the top of both files pointing at each other.
