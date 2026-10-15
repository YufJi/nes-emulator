#!/usr/bin/env bash
# Build the Rust core for Android and generate the Kotlin bindings the app
# compiles against.
#
#   ./android/build-core.sh                # both ABIs the app ships
#   ./android/build-core.sh arm64-v8a      # one ABI, e.g. for a quick check
#
# Outputs (all generated, none committed):
#   android/app/src/main/jniLibs/<abi>/libnes_uniffi.so   the library to package
#   android/app/src/main/java/uniffi/nes_uniffi/nes_uniffi.kt   the Kotlin API
set -euo pipefail

# A build started from an IDE or a Gradle task gets a bare PATH.
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/.." && pwd)"

abis=("$@")
if [ ${#abis[@]} -eq 0 ]; then
  # `arm64-v8a` is what current TV boxes run; `armeabi-v7a` is there because a
  # lot of sets — Sony's included — ship a 32-bit Android userspace on 64-bit
  # silicon, and an APK with no matching ABI will not install at all. The two
  # x86 entries are the emulator: `x86` because Google's Android TV system
  # images are 32-bit up to API 31.
  abis=(arm64-v8a armeabi-v7a x86_64 x86)
fi

# `cargo build --target` cannot link for Android on its own: the linker has to
# come from the NDK, and pointing at the right one per ABI is what cargo-ndk is.
if ! command -v cargo-ndk >/dev/null 2>&1; then
  echo "[build-core] 没有找到 cargo-ndk，请先安装：cargo install cargo-ndk --locked" >&2
  exit 1
fi

# cargo-ndk finds the NDK through these variables or through the SDK's own
# `ndk/` directory; say so plainly rather than letting the linker fail obscurely.
if [ -z "${ANDROID_NDK_HOME:-}${ANDROID_NDK_ROOT:-}" ]; then
  sdk="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/Library/Android/sdk}}"
  if ! ls "$sdk"/ndk/*/ >/dev/null 2>&1; then
    echo "[build-core] 没有找到 NDK。用 SDK Manager 装一个，或指到已有的一份：" >&2
    echo "             \"$sdk/tools/bin/sdkmanager\" --install \"ndk;26.1.10909125\"" >&2
    echo "             export ANDROID_NDK_HOME=\"$sdk/ndk/26.1.10909125\"" >&2
    exit 1
  fi
fi

echo "[build-core] 目标 ABI：${abis[*]}"
targets=()
for abi in "${abis[@]}"; do
  targets+=(-t "$abi")
done

# `lto` and `codegen-units` are the two knobs that matter for a 32-bit ARM build
# and they are set here rather than in the workspace `Cargo.toml`, because they
# are only worth their build time for the artifact a device runs: measured on the
# host, fat LTO with one codegen unit is 14.6% off the frame cost (3.372 -> 2.880
# ms/frame on `nestest`), while the WASM and iOS builds keep the faster profile.
cargo ndk "${targets[@]}" \
  -o "$here/app/src/main/jniLibs" \
  build --release -p nes-uniffi \
  --config 'profile.release.lto="fat"' \
  --config 'profile.release.codegen-units=1'

library="$(find "$here/app/src/main/jniLibs" -name 'libnes_uniffi.so' | head -n 1)"
if [ -z "$library" ]; then
  echo "[build-core] 构建完成但没有找到 libnes_uniffi.so" >&2
  exit 1
fi

echo "[build-core] 生成 Kotlin 绑定"
# `--library` reads the metadata the crate embeds in the binary; it does not run
# the code, so generating from an Android library on a Mac is expected to work.
cargo run -q -p nes-uniffi --bin uniffi-bindgen -- generate \
  --library "$library" \
  --language kotlin \
  --out-dir "$here/app/src/main/java"

echo "[build-core] 完成：$here/app/src/main/jniLibs"
