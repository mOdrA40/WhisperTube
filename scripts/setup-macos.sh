#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUNTIME="$ROOT/src-tauri/runtime/macos"
CPU="$RUNTIME/cpu"
METAL="$RUNTIME/metal"
source "$ROOT/scripts/activate-runtime.sh"
STAGED_CPU=""
STAGED_METAL=""
STAGED_FFMPEG=""
WORK="$(mktemp -d "${TMPDIR:-/tmp}/whispertube-macos.XXXXXX")"
FFMPEG_VERSION="9.0.1"
FFMPEG_SOURCE_SHA256="cf38e0e28c7e5605942c4a77755349b0145804a397af37eb1fb4c77cb237f635"
YT_DLP_VERSION="2026.10.21"
YT_DLP_SHA256="7a6fe71d7d680f1fb0ac18d4949873d77db2859e90017c0f44bd94b23be4ee40"
YT_DLP_TMP="$RUNTIME/.yt-dlp.tmp"

cleanup() {
  if [[ -n "$STAGED_FFMPEG" ]]; then rm -f "$STAGED_FFMPEG"; fi
  if [[ -n "$STAGED_CPU" ]]; then rm -rf "$STAGED_CPU"; fi
  if [[ -n "$STAGED_METAL" ]]; then rm -rf "$STAGED_METAL"; fi
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
node "$ROOT/scripts/test-distribution-config.mjs"

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
STAGED_FFMPEG="$(mktemp "$RUNTIME/.ffmpeg-staging.XXXXXX")"
cp -p "$ffmpeg_prefix/bin/ffmpeg" "$STAGED_FFMPEG"
"$STAGED_FFMPEG" -version >/dev/null
mv -f "$STAGED_FFMPEG" "$RUNTIME/ffmpeg"
STAGED_FFMPEG=""
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

WHISPER_VERSION="v1.9.2"
WHISPER_COMMIT="306c88f4d1286aec1bf96e544632897886af5501"
git clone --depth 1 --branch "$WHISPER_VERSION" https://github.com/ggml-org/whisper.cpp.git "$WORK/whisper.cpp"
actual_whisper_commit="$(git -C "$WORK/whisper.cpp" rev-parse HEAD)"
if [[ "$actual_whisper_commit" != "$WHISPER_COMMIT" ]]; then
  echo "Commit whisper.cpp tidak sesuai: expected $WHISPER_COMMIT, actual $actual_whisper_commit" >&2
  exit 1
fi

build_engine() {
  local build_dir="$1"
  local destination="$2"
  local metal="$3"
  local cli

  cmake -S "$WORK/whisper.cpp" -B "$build_dir" \
    -C "$ROOT/scripts/whisper-distribution.cmake" \
    -DGGML_METAL="$metal" \
    -DWHISPER_BUILD_TESTS=OFF \
    -DCMAKE_BUILD_TYPE=Release
  cmake --build "$build_dir" --config Release --target whisper-cli --parallel "$(sysctl -n hw.ncpu)"

  cli="$(find "$build_dir" -type f -name whisper-cli -print -quit)"
  if [[ -z "$cli" ]]; then
    echo "whisper-cli tidak ditemukan setelah build ($destination)." >&2
    exit 1
  fi

  # Destination is an empty staging directory, never the active runtime.
  find "$(dirname "$cli")" -maxdepth 1 \( -type f -o -type l \) \( \
    -name "whisper-cli" -o \
    -name "libwhisper*" -o \
    -name "libggml*" -o \
    -name "ggml*" \
  \) -exec cp -a {} "$destination/" \;
  if [[ ! -x "$destination/whisper-cli" ]]; then
    echo "whisper-cli gagal disalin ke runtime yang diizinkan." >&2
    exit 1
  fi
  "$destination/whisper-cli" --version >/dev/null
}

STAGED_CPU="$(mktemp -d "$RUNTIME/.cpu-staging.XXXXXX")"
STAGED_METAL="$(mktemp -d "$RUNTIME/.metal-staging.XXXXXX")"
build_engine "$WORK/build-cpu" "$STAGED_CPU" OFF
build_engine "$WORK/build-metal" "$STAGED_METAL" ON

# A build-tree self-check can accidentally resolve absolute dylib paths. Remove
# both trees before exercising only the staged runtime that will be bundled.
rm -rf "$WORK/build-cpu" "$WORK/build-metal" "$WORK/whisper.cpp"
"$STAGED_CPU/whisper-cli" --version >/dev/null
"$STAGED_METAL/whisper-cli" --version >/dev/null
activate_runtime "$STAGED_CPU" "$CPU"
STAGED_CPU=""
activate_runtime "$STAGED_METAL" "$METAL"
STAGED_METAL=""

cd "$ROOT"
npm ci
echo "macOS development runtime ready. Run: npm run tauri:dev"
