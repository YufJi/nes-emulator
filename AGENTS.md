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

## Coding Style & Naming Conventions

- Rust: rustfmt defaults (4 spaces), edition 2021, MSRV 1.70. Use `snake_case`
  functions/fields, `CamelCase` types, `SCREAMING_SNAKE_CASE` consts.
- TypeScript/React: 2-space indent, `camelCase` values, `PascalCase.tsx`
  components. Keep comments explaining *why*, matching the surrounding style.
- Prefer minimal, focused diffs; do not reformat unrelated code.

## Testing Guidelines

- Rust `#[test]` functions named `test_<behavior>`; add a regression test with
  every fix. Integration suites live in `crates/nes-core/tests/`
  (`ppu.rs`, `apu.rs`, `mappers.rs`, `nestest.rs`) and `crates/nes-wasm/tests/smb.rs`.
- ROM-dependent tests are skipped or marked `#[ignore]` when `roms/` is empty.
- For emulator changes, verify determinism (rewind/replay produces identical
  frames). For web changes, run `npm run build`.

## Commit & Pull Request Guidelines

- Follow Conventional Commits with an optional scope: `feat(apu): …`,
  `fix(mapper5): …`, `refactor: …`. Subjects are Chinese or English; history
  mixes both.
- Keep commits scoped, describe behavior changes, and state what you ran.
- PRs: summary, linked issue, test evidence, and screenshots/GIFs for rendering
  or UI changes. Call out the mapper and ROM used when touching cartridge code.
