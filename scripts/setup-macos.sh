#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUNTIME="$ROOT/src-tauri/runtime/macos"
CPU="$RUNTIME/cpu"
METAL="$RUNTIME/metal"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/whispertube-macos.XXXXXX")"
FFMPEG_VERSION="9.0.1"
FFMPEG_SOURCE_SHA256="cf38e0e28c7e5605942c4a77755349b0145804a397af37eb1fb4c77cb237f635"
YT_DLP_VERSION="2026.08.19"
YT_DLP_SHA256="0f192b7ec147ab6288885d6351d9ab67367640029b4377576ef46dd79cf7b202"
YT_DLP_TMP="$RUNTIME/.yt-dlp.tmp"

cleanup() {
  rm -rf "$WORK"
  rm -f "$YT_DLP_TMP"
}
trap cleanup EXIT

require_command() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "$1 tidak ditemukan. $2" >&2
    exit 1
  }
}

require_command brew "Install Homebrew dari https://brew.sh/."
require_command cargo "Install Rust dari https://rustup.rs/."
require_command npm "Install Node.js/npm dari https://nodejs.org/."
require_command git "Install git atau Xcode Command Line Tools."
require_command curl "Install curl atau Xcode Command Line Tools."
require_command shasum "Install Xcode Command Line Tools."
require_command xcode-select "Install Xcode Command Line Tools dengan: xcode-select --install"

if ! xcode-select -p >/dev/null 2>&1; then
  echo "Xcode Command Line Tools belum aktif. Jalankan: xcode-select --install" >&2
  exit 1
fi

brew install cmake nasm
require_command cmake "CMake gagal terpasang melalui Homebrew."
require_command make "Install Xcode Command Line Tools."
require_command tar "Install Xcode Command Line Tools."
require_command getconf "Install Xcode Command Line Tools."
require_command otool "Install Xcode Command Line Tools."

mkdir -p "$CPU" "$METAL" "$RUNTIME"

ffmpeg_archive="$WORK/ffmpeg-${FFMPEG_VERSION}.tar.xz"
curl --fail --location --retry 3 --connect-timeout 30 \
  "https://ffmpeg.org/releases/ffmpeg-${FFMPEG_VERSION}.tar.xz" \
  -o "$ffmpeg_archive"
if ! echo "$FFMPEG_SOURCE_SHA256  $ffmpeg_archive" | shasum -a 256 --check --status; then
  echo "Checksum source FFmpeg tidak cocok; build dibatalkan." >&2
  exit 1
fi
tar -xf "$ffmpeg_archive" -C "$WORK"

ffmpeg_source="$WORK/ffmpeg-${FFMPEG_VERSION}"
ffmpeg_prefix="$WORK/ffmpeg-prefix"
pushd "$ffmpeg_source" >/dev/null
./configure \
  --prefix="$ffmpeg_prefix" \
  --disable-shared \
  --enable-static \
  --disable-autodetect \
  --disable-debug \
  --disable-doc \
  --disable-ffplay \
  --disable-ffprobe \
  --disable-network
make -j"$(getconf _NPROCESSORS_ONLN)"
make install
popd >/dev/null

if [[ ! -x "$ffmpeg_prefix/bin/ffmpeg" ]]; then
  echo "FFmpeg statik tidak ditemukan setelah build." >&2
  exit 1
fi
"$ffmpeg_prefix/bin/ffmpeg" -version >/dev/null
if otool -L "$ffmpeg_prefix/bin/ffmpeg" | grep -Eq '/opt/homebrew|/usr/local/opt|/opt/local'; then
  echo "FFmpeg masih memiliki dependency package manager macOS; build dibatalkan." >&2
  exit 1
fi
cp "$ffmpeg_prefix/bin/ffmpeg" "$RUNTIME/ffmpeg"
chmod +x "$RUNTIME/ffmpeg"

curl --fail --location --retry 3 --connect-timeout 30 \
  "https://github.com/yt-dlp/yt-dlp/releases/download/$YT_DLP_VERSION/yt-dlp_macos" \
  -o "$YT_DLP_TMP"
if ! echo "$YT_DLP_SHA256  $YT_DLP_TMP" | shasum -a 256 --check --status; then
  echo "Checksum yt-dlp macOS tidak cocok; file tidak dipasang." >&2
  exit 1
fi
mv "$YT_DLP_TMP" "$RUNTIME/yt-dlp"
chmod +x "$RUNTIME/yt-dlp"

git clone --depth 1 --branch v1.9.1 https://github.com/ggml-org/whisper.cpp.git "$WORK/whisper.cpp"

build_engine() {
  local build_dir="$1"
  local destination="$2"
  local metal="$3"
  local cli

  cmake -S "$WORK/whisper.cpp" -B "$build_dir" \
    -DGGML_METAL="$metal" \
    -DWHISPER_BUILD_TESTS=OFF \
    -DCMAKE_BUILD_TYPE=Release
  cmake --build "$build_dir" --config Release --target whisper-cli --parallel "$(sysctl -n hw.ncpu)"

  cli="$(find "$build_dir" -type f -name whisper-cli -print -quit)"
  if [[ -z "$cli" ]]; then
    echo "whisper-cli tidak ditemukan setelah build ($destination)." >&2
    exit 1
  fi

  rm -rf "$destination"
  mkdir -p "$destination"
  cp -R "$(dirname "$cli")/." "$destination/"
  "$destination/whisper-cli" --version >/dev/null
}

build_engine "$WORK/build-cpu" "$CPU" OFF
build_engine "$WORK/build-metal" "$METAL" ON

cd "$ROOT"
npm ci
echo "macOS development runtime ready. Run: npm run tauri:dev"
