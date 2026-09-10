#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"

case "$(uname -s)" in
  Darwin)
    RUNTIME="$ROOT/src-tauri/runtime/macos"
    PLATFORM="macOS"
    ;;
  Linux)
    RUNTIME="$ROOT/src-tauri/runtime/linux"
    PLATFORM="Linux"
    ;;
  *)
    echo "Script ini hanya untuk macOS atau Linux." >&2
    exit 1
    ;;
esac

if [[ ! -d "$RUNTIME" ]]; then
  echo "Runtime developer $PLATFORM tidak ditemukan; tidak ada yang perlu dibersihkan."
  exit 0
fi

# The path is selected from the fixed repository root above. Preserve the
# tracked placeholder while removing the platform's generated runtime tree.
find "$RUNTIME" -mindepth 1 -maxdepth 1 ! -name .gitkeep -exec rm -rf -- {} +
echo "Runtime developer $PLATFORM dibersihkan. Data user aplikasi tidak disentuh."
