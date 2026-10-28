#!/usr/bin/env bash
# Cross-compiles the Windows installer (NSIS setup.exe) on Linux.
#
# Uses the x86_64-pc-windows-gnu target with MinGW. When the prebuilt Windows
# standard library cannot be downloaded with rustup, it is built from source
# (-Zbuild-std), which needs the matching `library/` sources in the sysroot.
#
# Prerequisites (Ubuntu 24.04):
#   sudo apt install mingw-w64 nsis llvm lld
#   rustup target add x86_64-pc-windows-gnu   # or provide rust-src, see below
#
# Output: release/ with the installer and SHA256SUMS.txt
set -euo pipefail
cd "$(dirname "$0")/.."

TARGET=x86_64-pc-windows-gnu
for tool in x86_64-w64-mingw32-gcc makensis npm cargo; do
  command -v "$tool" >/dev/null || { echo "missing: $tool" >&2; exit 1; }
done

SYSROOT="$(rustc --print sysroot)"
SHIM_DIR=""
if [ ! -d "$SYSROOT/lib/rustlib/$TARGET/lib" ] || [ -z "$(ls -A "$SYSROOT/lib/rustlib/$TARGET/lib" 2>/dev/null)" ]; then
  if [ ! -f "$SYSROOT/lib/rustlib/src/rust/library/std/Cargo.toml" ]; then
    echo "No prebuilt std for $TARGET and no rust-src in the sysroot." >&2
    echo "Run 'rustup target add $TARGET' or copy rust-lang/rust@$(rustc -V | cut -d' ' -f2):library into $SYSROOT/lib/rustlib/src/rust/library" >&2
    exit 1
  fi
  echo "Building the Windows std from source (-Zbuild-std)."
  export RUSTC_BOOTSTRAP=1 CARGO_UNSTABLE_BUILD_STD=std,panic_abort
  # The Tauri CLI asks rustup whether the target is installed; answer yes.
  SHIM_DIR="$(mktemp -d)"
  REAL_RUSTUP="$(command -v rustup)"
  cat > "$SHIM_DIR/rustup" <<EOF
#!/bin/sh
if [ "\$1" = "target" ] && [ "\$2" = "list" ]; then
  "$REAL_RUSTUP" "\$@" | sed 's/^$TARGET\$/$TARGET (installed)/'
  case "\$*" in *--installed*) echo $TARGET;; esac
  exit 0
fi
exec "$REAL_RUSTUP" "\$@"
EOF
  chmod +x "$SHIM_DIR/rustup"
  export PATH="$SHIM_DIR:$PATH"
fi
trap '[ -n "$SHIM_DIR" ] && rm -rf "$SHIM_DIR"' EXIT

export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_AR=x86_64-w64-mingw32-ar

npm ci --no-audit --no-fund
npm test
npx tauri build --target "$TARGET" --bundles nsis

VERSION="$(node -p "require('./package.json').version")"
BUNDLE="src-tauri/target/$TARGET/release/bundle/nsis"
mkdir -p release
OUT="release/CanvasAssistant-$VERSION-x64-setup.exe"
cp "$BUNDLE/"*-setup.exe "$OUT"
(cd release && sha256sum "$(basename "$OUT")" > SHA256SUMS.txt && cat SHA256SUMS.txt)
echo "Built $OUT"
