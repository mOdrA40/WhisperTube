#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUNTIME="$ROOT/src-tauri/runtime/linux"

cd "$ROOT"
command -v npm >/dev/null 2>&1 || { echo "npm tidak ditemukan." >&2; exit 1; }
command -v cargo >/dev/null 2>&1 || { echo "cargo tidak ditemukan." >&2; exit 1; }
command -v pkg-config >/dev/null 2>&1 || { echo "pkg-config tidak ditemukan." >&2; exit 1; }
command -v patchelf >/dev/null 2>&1 || { echo "patchelf tidak ditemukan; diperlukan untuk AppImage." >&2; exit 1; }
pkg-config --exists webkit2gtk-4.1 || { echo "libwebkit2gtk-4.1-dev belum terpasang." >&2; exit 1; }

for required in "$RUNTIME/ffmpeg" "$RUNTIME/yt-dlp" "$RUNTIME/cpu/whisper-cli"; do
  if [[ ! -f "$required" ]]; then
    echo "Runtime belum lengkap: $required" >&2
    echo "Jalankan ./scripts/setup-linux.sh terlebih dahulu." >&2
    exit 1
  fi
done

npm ci
npm run tauri:build -- --bundles deb,appimage
echo "Linux bundles selesai. Cek: src-tauri/target/release/bundle/"
