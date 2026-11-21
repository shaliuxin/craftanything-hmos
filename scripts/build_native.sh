#!/usr/bin/env bash
# Build the HarmonyOS native libraries (every ABI in CRAFTANYTHING_NATIVE_TARGETS) before hvigor/CMake.
#
# Windows 铁律（2026-11-18 SolidCraft 0.44 实撞，通用）：
#   一旦依赖里出现需要编译 C 的 crate（cc-rs），CC_<triple>/AR_<triple>/CARGO_TARGET_<triple>_LINKER
#   必须用 NDK 的 Windows 原生工具 clang.exe / llvm-ar.exe（PE，自带 OHOS sysroot，靠 --target 交叉编译）。
#   绝不能用 <triple>-clang —— 它是 #!/bin/... 的 POSIX shell 脚本，Windows 上执行报 os error 193。
#   判据：head -c4 看魔数，4d 5a=PE 可用；23 21 2f 62=shell 脚本不可用。
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ -f "$ROOT/scripts/dev.local.env" ]]; then source "$ROOT/scripts/dev.local.env"; fi

# 默认指向 Windows DevEco 安装；macOS/CI 可用 CRAFTANYTHING_NATIVE_SDK / DEVECO_SDK_HOME 覆盖。
DEVECO_DEFAULT="${CRAFTANYTHING_DEVECO_HOME:-/c/Program Files/Huawei/DevEco Studio}"
SDK="${CRAFTANYTHING_NATIVE_SDK:-${DEVECO_SDK_HOME:-$DEVECO_DEFAULT}/sdk/default/openharmony/native}"
# 兼容 SDK 根目录已指向 openharmony 或其 native 目录的 CI 布局。
if [[ ! -d "$SDK/llvm" ]]; then
  for candidate in "${DEVECO_SDK_HOME:-}/openharmony/native" "${DEVECO_SDK_HOME:-}/native" "${DEVECO_SDK_HOME:-}"; do
    if [[ -d "$candidate/llvm" && -d "$candidate/sysroot" ]]; then SDK="$candidate"; break; fi
  done
fi
LLVMBIN="$SDK/llvm/bin"
[[ -x "$LLVMBIN/clang.exe" || -x "$LLVMBIN/clang" ]] || { echo "HarmonyOS native SDK not found: $SDK" >&2; exit 1; }
# Windows 上必须用 clang.exe（PE），不要用 <triple>-clang（POSIX shell 脚本，Windows 跑不了）。
CLANG="clang.exe"; [[ -x "$LLVMBIN/clang" ]] && CLANG="clang"
AR="$LLVMBIN/llvm-ar"
[[ -x "$LLVMBIN/llvm-ar.exe" && ! -x "$LLVMBIN/llvm-ar" ]] && AR="$LLVMBIN/llvm-ar.exe"

# 一份清单驱动构建：CMake 映射同一组 ABI（entry/build-profile.json5 abiFilters），
# verify_native.py 校验打包结果。默认双 ABI：一个 HAP 同时跑 ARM 真机与 x86_64 手机/PC 模拟器。
TARGETS="${CRAFTANYTHING_NATIVE_TARGETS:-aarch64-unknown-linux-ohos x86_64-unknown-linux-ohos}"
PROFILE="${CRAFTANYTHING_RUST_PROFILE:-release}"
ARGS=()
case "$PROFILE" in release) ARGS+=(--release);; debug) ARGS+=(--profile dev);; *) echo 'Profile must be release or debug' >&2; exit 2;; esac
for target in $TARGETS; do
  case "$target" in
    aarch64-unknown-linux-ohos|x86_64-unknown-linux-ohos|armv7-unknown-linux-ohos) ;;
    *) echo "Unsupported target: $target" >&2; exit 2;;
  esac
  [[ -d "$(rustc --print sysroot)/lib/rustlib/$target" ]] || { echo "Install target with: rustup target add $target" >&2; exit 1; }
  suffix="${target//-/_}"
  upper="$(printf '%s' "$suffix" | tr '[:lower:]' '[:upper:]')"
  env "CC_${suffix}=$LLVMBIN/$CLANG" "AR_${suffix}=$AR" \
      "CARGO_TARGET_${upper}_LINKER=$LLVMBIN/$CLANG" \
      cargo build --locked --manifest-path "$ROOT/native/rust/Cargo.toml" --target "$target" "${ARGS[@]}"
done
