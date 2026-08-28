#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUNTIME="$ROOT/src-tauri/runtime/macos"
CPU="$RUNTIME/cpu"
WORK="${TMPDIR:-/tmp}/whispertube-macos"
mkdir -p "$CPU" "$WORK"

command -v brew >/dev/null || { echo "Homebrew diperlukan untuk bootstrap development macOS."; exit 1; }
command -v cargo >/dev/null || { echo "Install Rust dari https://rustup.rs/"; exit 1; }
command -v npm >/dev/null || { echo "Install Node.js/npm."; exit 1; }

brew install ffmpeg yt-dlp cmake
cp "$(command -v ffmpeg)" "$RUNTIME/ffmpeg"
cp "$(command -v yt-dlp)" "$RUNTIME/yt-dlp"

rm -rf "$WORK/whisper.cpp"
git clone --depth 1 --branch v1.9.1 https://github.com/ggml-org/whisper.cpp.git "$WORK/whisper.cpp"
cmake -S "$WORK/whisper.cpp" -B "$WORK/whisper.cpp/build" -DGGML_METAL=ON -DCMAKE_BUILD_TYPE=Release
cmake --build "$WORK/whisper.cpp/build" --config Release -j
find "$WORK/whisper.cpp/build/bin" -maxdepth 1 -type f -exec cp {} "$CPU/" \;

cd "$ROOT"
npm install
echo "macOS development runtime ready. Run: npm run tauri:dev"
