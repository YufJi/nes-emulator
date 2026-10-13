# Disclaimer

[简体中文](./DISCLAIMER.zh-CN.md) · **English**

This project is a personal, non-commercial project. It exists purely for personal study,
research and experimentation — emulator internals, Rust/WASM/UniFFI host bindings, and
front-end engineering. Nothing here is a product, a service, or a business.

## Scope

The statement below covers everything this repository publishes:

- **The repository** — all code, documentation and assets under version control.
- **The web UI** (`web/`) — the page itself, its built-in game list, and any deployment of
  it (for example the GitHub Pages site).
- **The iOS app** (`ios/`, `NesApp`) — the app and its Xcode project.
- **Everything else** — the `nes-core` emulator core, the `nes-wasm` and `nes-uniffi`
  bindings, the documentation, and anything else derived from them.

## What this means

- **No commercial activity of any kind.** The author sells nothing, charges nothing,
  accepts no payment, runs no advertising, and offers no paid support. No part of this
  project is offered as a product or a service.
- **No ROMs or game files are provided or distributed.** ROM images are never committed to
  the repository; the built-in game list stores names and links only, and usually does not
  host the files. Any `.nes` file you load is your own, and you are responsible for having
  the right to use it.
- **No affiliation and no endorsement.** This project is not affiliated with, authorized
  by, endorsed by, or sponsored by Nintendo or any other rights holder. "Nintendo", "NES",
  and any game names that appear in the project are trademarks of their respective owners
  and are used only to describe what the code does.
- **No warranty.** The code is provided as-is, for learning. Nothing here is guaranteed to
  work, and the author accepts no liability for how it is used.
- **Not a tool for circumvention.** The emulator defeats no copy protection and contains no
  copyrighted content; it is a study of emulation and of host integration.

If you are a rights holder and find something here objectionable, please open an issue and
it will be removed.

The source code stays under the MIT license (the `license` field in
[`Cargo.toml`](./Cargo.toml)). This statement describes the project's intent; it does not
change or replace that license.
