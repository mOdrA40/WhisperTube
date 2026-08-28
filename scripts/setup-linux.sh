#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUNTIME="$ROOT/src-tauri/runtime/linux"
CPU="$RUNTIME/cpu"
WORK="${TMPDIR:-/tmp}/whispertube-linux"
mkdir -p "$CPU" "$WORK"

command -v cargo >/dev/null || { echo "Install Rust dari https://rustup.rs/"; exit 1; }
command -v npm >/dev/null || { echo "Install Node.js/npm."; exit 1; }
command -v cmake >/dev/null || { echo "Install cmake + compiler toolchain distro kamu."; exit 1; }
command -v ffmpeg >/dev/null || { echo "Install ffmpeg dari package manager distro kamu."; exit 1; }

cp "$(command -v ffmpeg)" "$RUNTIME/ffmpeg"
curl -L https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux -o "$RUNTIME/yt-dlp"
chmod +x "$RUNTIME/yt-dlp"

rm -rf "$WORK/whisper.cpp"
git clone --depth 1 --branch v1.9.1 https://github.com/ggml-org/whisper.cpp.git "$WORK/whisper.cpp"
cmake -S "$WORK/whisper.cpp" -B "$WORK/whisper.cpp/build" -DCMAKE_BUILD_TYPE=Release
cmake --build "$WORK/whisper.cpp/build" --config Release -j
find "$WORK/whisper.cpp/build/bin" -maxdepth 1 -type f -exec cp {} "$CPU/" \;
chmod +x "$CPU/whisper-cli" || true

cd "$ROOT"
npm install
echo "Linux development runtime ready. Run: npm run tauri:dev"
