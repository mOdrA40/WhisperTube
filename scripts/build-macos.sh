#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUNTIME="$ROOT/src-tauri/runtime/macos"

cd "$ROOT"
command -v npm >/dev/null 2>&1 || { echo "npm tidak ditemukan." >&2; exit 1; }
command -v cargo >/dev/null 2>&1 || { echo "cargo tidak ditemukan." >&2; exit 1; }
command -v xcode-select >/dev/null 2>&1 || { echo "Xcode Command Line Tools diperlukan." >&2; exit 1; }
xcode-select -p >/dev/null 2>&1 || { echo "Jalankan: xcode-select --install" >&2; exit 1; }

for required in "$RUNTIME/ffmpeg" "$RUNTIME/yt-dlp" "$RUNTIME/cpu/whisper-cli" "$RUNTIME/metal/whisper-cli"; do
  if [[ ! -f "$required" ]]; then
    echo "Runtime belum lengkap: $required" >&2
    echo "Jalankan ./scripts/setup-macos.sh terlebih dahulu." >&2
    exit 1
  fi
done

npm ci
build_args=(--bundles app,dmg)
if [[ "${TAURI_CREATE_UPDATER_ARTIFACTS:-}" == "true" ]]; then
  if [[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" && -z "${TAURI_SIGNING_PRIVATE_KEY_PATH:-}" ]]; then
    echo "TAURI_SIGNING_PRIVATE_KEY atau TAURI_SIGNING_PRIVATE_KEY_PATH wajib diisi saat membuat artifact updater." >&2
    exit 1
  fi
  build_args+=(--config '{"bundle":{"createUpdaterArtifacts":true}}')
fi
npm run tauri:build -- "${build_args[@]}"
echo "macOS bundles selesai. Cek: src-tauri/target/release/bundle/"
