#!/usr/bin/env bash
# Build the Rust core for a destination and generate the Swift bindings the app
# target compiles against.
#
#   ./ios/build-core.sh macos        # host, for quick checks
#   ./ios/build-core.sh iphoneos     # device (arm64)
#   ./ios/build-core.sh iphonesimulator
#
# Xcode calls this from a build phase with no argument: $PLATFORM_NAME and $ARCHS
# pick the Rust target, so a fresh clone builds by just hitting Run.
#
# Outputs (all generated, none committed):
#   ios/Artifacts/libnes_uniffi.a              the static library to link
#   ios/NesApp/Generated/nes_uniffi.swift      the Swift API
#   ios/NesApp/Generated/nes_uniffiFFI.h       the C header
#   ios/NesApp/Generated/module.modulemap      makes `import nes_uniffiFFI` work
set -euo pipefail

# Xcode's build phases get a bare PATH.
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/.." && pwd)"

platform="${1:-${PLATFORM_NAME:-macosx}}"
# Xcode can hand over several architectures; take the first.
arch="${ARCHS:-$(uname -m)}"
arch="${arch%% *}"

# Apple spells it arm64, Rust spells it aarch64.
case "$arch" in
  arm64) rust_arch="aarch64" ;;
  *)     rust_arch="$arch" ;;
esac

case "$platform" in
  iphoneos) triple="$rust_arch-apple-ios" ;;
  iphonesimulator)
    # The simulator is its own Rust target on Apple Silicon; on Intel it shares
    # the old device triple.
    case "$arch" in
      arm64) triple="aarch64-apple-ios-sim" ;;
      *)     triple="$rust_arch-apple-ios" ;;
    esac
    ;;
  macosx|macos) triple="$rust_arch-apple-darwin" ;;
  *) echo "[build-core] 未知平台：$platform" >&2; exit 2 ;;
esac

echo "[build-core] 平台 $platform / 架构 $arch → Rust target $triple"
cargo build --manifest-path "$root/Cargo.toml" -p nes-uniffi --target "$triple"

library="$root/target/$triple/debug/libnes_uniffi.a"
if [ ! -f "$library" ]; then
  echo "[build-core] 没有找到 $library（是不是还没装 target？rustup target add $triple）" >&2
  exit 1
fi

mkdir -p "$here/Artifacts" "$here/NesApp/Generated"
cp -f "$library" "$here/Artifacts/libnes_uniffi.a"

echo "[build-core] 生成 Swift 绑定"
cargo run -q -p nes-uniffi --bin uniffi-bindgen -- generate \
  --library "$library" \
  --language swift \
  --out-dir "$here/NesApp/Generated"

# SwiftPM/Xcode look for `module.modulemap` by that exact name.
mv -f "$here/NesApp/Generated/nes_uniffiFFI.modulemap" "$here/NesApp/Generated/module.modulemap"

echo "[build-core] 完成：$here/Artifacts/libnes_uniffi.a"
