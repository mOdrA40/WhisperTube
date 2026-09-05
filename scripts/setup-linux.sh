#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUNTIME="$ROOT/src-tauri/runtime/linux"
CPU="$RUNTIME/cpu"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/whispertube-linux.XXXXXX")"
FFMPEG_VERSION="9.0.1"
FFMPEG_SOURCE_SHA256="cf38e0e28c7e5605942c4a77755349b0145804a397af37eb1fb4c77cb237f635"
YT_DLP_VERSION="2026.08.19"
YT_DLP_SHA256="58162f9bfdc27458ea47bfcb311cf47028f17d8154a8bf7d689861d46399230a"
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

require_command cargo "Install Rust dari https://rustup.rs/."
require_command npm "Install Node.js/npm dari https://nodejs.org/."
require_command cmake "Install CMake dari package manager distro kamu."
require_command make "Install build-essential dari package manager distro kamu."
require_command nasm "Install nasm dari package manager distro kamu."
require_command curl "Install curl dari package manager distro kamu."
require_command git "Install git dari package manager distro kamu."
require_command tar "Install tar dari package manager distro kamu."
require_command xz "Install xz-utils dari package manager distro kamu."
require_command getconf "Install libc-dev dari package manager distro kamu."
require_command ldd "Install libc-dev dari package manager distro kamu."
require_command pkg-config "Install pkg-config dari package manager distro kamu."
require_command sha256sum "Install coreutils dari package manager distro kamu."

if ! pkg-config --exists webkit2gtk-4.1; then
  cat >&2 <<'EOF'
Dependency Tauri WebKitGTK 4.1 belum ditemukan.
Untuk Ubuntu/Debian, jalankan:
sudo apt-get update
sudo apt-get install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev patchelf xdg-utils nasm xz-utils
EOF
  exit 1
fi

if ! pkg-config --exists openssl; then
  echo "OpenSSL development package belum ditemukan. Install libssl-dev (Ubuntu/Debian) atau padanan distro kamu." >&2
  exit 1
fi

mkdir -p "$CPU" "$RUNTIME"

ffmpeg_archive="$WORK/ffmpeg-${FFMPEG_VERSION}.tar.xz"
curl --fail --location --retry 3 --connect-timeout 30 \
  "https://ffmpeg.org/releases/ffmpeg-${FFMPEG_VERSION}.tar.xz" \
  -o "$ffmpeg_archive"
if ! echo "$FFMPEG_SOURCE_SHA256  $ffmpeg_archive" | sha256sum --check --status; then
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
  --disable-network \
  --extra-ldflags="-static"
make -j"$(getconf _NPROCESSORS_ONLN)"
make install
popd >/dev/null

if [[ ! -x "$ffmpeg_prefix/bin/ffmpeg" ]]; then
  echo "FFmpeg statik tidak ditemukan setelah build." >&2
  exit 1
fi
"$ffmpeg_prefix/bin/ffmpeg" -version >/dev/null
ffmpeg_dependencies="$(ldd "$ffmpeg_prefix/bin/ffmpeg" 2>&1 || true)"
if grep -Eq 'not found|libav(util|codec|format|device|filter|swscale|swresample|postproc)' <<<"$ffmpeg_dependencies"; then
  echo "FFmpeg masih memiliki dependency FFmpeg eksternal atau dependency hilang; build dibatalkan." >&2
  exit 1
fi
cp "$ffmpeg_prefix/bin/ffmpeg" "$RUNTIME/ffmpeg"
chmod +x "$RUNTIME/ffmpeg"

curl --fail --location --retry 3 --connect-timeout 30 \
  "https://github.com/yt-dlp/yt-dlp/releases/download/$YT_DLP_VERSION/yt-dlp_linux" \
  -o "$YT_DLP_TMP"
if ! echo "$YT_DLP_SHA256  $YT_DLP_TMP" | sha256sum --check --status; then
  echo "Checksum yt-dlp tidak cocok; file tidak dipasang." >&2
  exit 1
fi
mv "$YT_DLP_TMP" "$RUNTIME/yt-dlp"
chmod +x "$RUNTIME/yt-dlp"

git clone --depth 1 --branch v1.9.1 https://github.com/ggml-org/whisper.cpp.git "$WORK/whisper.cpp"
cmake -S "$WORK/whisper.cpp" -B "$WORK/build" \
  -DGGML_METAL=OFF \
  -DWHISPER_BUILD_TESTS=OFF \
  -DCMAKE_BUILD_TYPE=Release
cmake --build "$WORK/build" --config Release --target whisper-cli --parallel

cli="$(find "$WORK/build" -type f -name whisper-cli -print -quit)"
if [[ -z "$cli" ]]; then
  echo "whisper-cli tidak ditemukan setelah build." >&2
  exit 1
fi

rm -rf "$CPU"
mkdir -p "$CPU"
cp -R "$(dirname "$cli")/." "$CPU/"
chmod +x "$CPU/whisper-cli"
"$CPU/whisper-cli" --version >/dev/null

cd "$ROOT"
npm ci
echo "Linux development runtime ready. Run: npm run tauri:dev"
