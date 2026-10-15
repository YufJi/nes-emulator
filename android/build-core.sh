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
  # 64-bit devices — every Android TV box worth shipping to — plus the two
  # architectures an emulator runs on. `x86` is there because Google's Android TV
  # system images are 32-bit up to API 31, so it is the only way to try a TV
  # build without a TV; adding `armeabi-v7a` here and to the app's `abiFilters`
  # is the whole of what 32-bit boxes need.
  abis=(arm64-v8a x86_64 x86)
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

cargo ndk "${targets[@]}" \
  -o "$here/app/src/main/jniLibs" \
  build --release -p nes-uniffi

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
