# WhisperTube 0.1

WhisperTube is a local-first desktop app that turns videos from supported platforms into transcripts using `yt-dlp`, FFmpeg, and whisper.cpp. Audio and transcript processing stay on the user's device.

Other languages: [Bahasa Indonesia](README_ID.md) · [中文（普通话）](README_CN.md)

## Current scope

- Windows 10/11 x64 is the primary development and testing target.
- English is the default interface language. Indonesian and Simplified Chinese are also available from Settings.
- CPU inference is included in the development setup.
- NVIDIA CUDA is optional and installed separately so the base setup stays small.
- The app detects the current operating system, architecture, GPU, and installed browsers before offering optional components.

Supported source families include YouTube, TikTok, X/Twitter, Facebook, Instagram, Reddit, Twitch, Vimeo, Dailymotion, Pinterest, LinkedIn, Tumblr, Bilibili, and VK. Support is based on the current yt-dlp extractor and can change as each platform changes.

## Features

- Paste a supported video URL and inspect metadata before downloading.
- Public videos.
- Login-protected videos through an imported Netscape `cookies.txt` file. The app never asks for site passwords.
- Manual Netscape `cookies.txt` access works across browser families; on macOS, Settings can also try the detected Safari session directly.
- Download `bestaudio/best`; the original download is not converted to MP3 first.
- FFmpeg normalization to signed 16-bit PCM WAV, mono, 16 kHz.
- Local whisper.cpp inference.
- Three interface languages: English, Bahasa Indonesia, and 中文（普通话）.
- Model manager:
  - Fast: `base` (~142 MB)
  - Balanced: `large-v3-turbo-q5_0` (~547 MB)
  - Accurate: `large-v3-q5_0` (~1.1 GB)
- SHA-1 verification for models against the known upstream manifest.
- CPU fallback.
- Windows x64 NVIDIA CUDA acceleration, installed on demand from the pinned official whisper.cpp release.
- Metal and Vulkan accelerator packs only appear when the platform, architecture, and compatible GPU detection match.
- Progress reporting, cancellation, timestamped transcripts, TXT/SRT/VTT export, and local SQLite history.
- Model and source-video download progress show transferred bytes, with cancellable model downloads.
- Transcription progress shows CPU/GPU utilization when the platform exposes those metrics, plus download network speed.
- Clear the current URL, preview, and transcript from the workspace; permanently delete one, multiple, or all visible history items.
- Temporary audio and WAV files are removed after processing unless `Keep processed audio` is enabled.

## Windows development setup

### 1. Extract the project

For example:

```text
D:\Projects\WhisperTube
```

Avoid protected directories such as `C:\Program Files` during development.

### 2. Install prerequisites

#### Node.js

Use Node.js 22 or newer:

```powershell
node -v
npm -v
```

#### Rust

Install Rust from <https://rustup.rs/>. Open a new PowerShell window afterward:

```powershell
rustc --version
cargo --version
```

#### Microsoft C++ Build Tools

Install Visual Studio Build Tools 2022 and select the **Desktop development with C++** workload. Keep the MSVC compiler and a Windows SDK selected. WebView2 Runtime is normally already available on Windows 10/11.

### 3. Open PowerShell in the project folder

```powershell
cd D:\Projects\WhisperTube
```

If PowerShell blocks local scripts, bypass the policy only for the current terminal:

```powershell
Set-ExecutionPolicy -Scope Process Bypass
```

### 4. Run the bootstrap

```powershell
.\scripts\setup-windows.ps1
```

The script checks Node/npm/Rust, downloads `yt-dlp.exe`, FFmpeg, and the whisper.cpp CPU engine, installs JavaScript dependencies, and runs a runtime self-check. Whisper models are downloaded later from the UI.

Development runtime files are stored under:

```text
src-tauri\runtime\windows\
```

### 5. Start the development app

```powershell
.\scripts\run-dev.ps1
```

or:

```powershell
npm run tauri:dev
```

The first Rust compilation can take a while. Later runs use incremental compilation. If Vite reports that port `1420` is already in use, close the previous development process before starting another one.

### 6. Download a model

Open the model panel and choose a model. `Balanced` is a good starting point for general use. Model files are stored in the user's application data, not in the source tree.

When a supported NVIDIA CUDA path is available, WhisperTube reads total and free VRAM and applies conservative model guardrails. These limits are safety checks, not a universal guarantee because drivers and other applications can consume VRAM.

### 7. Optional acceleration

Open **Settings → Hardware**. WhisperTube only offers an accelerator that matches the current platform and detected hardware:

- Windows x64 + detected NVIDIA GPU: CUDA may be installed from the official pinned whisper.cpp release.
- macOS: the matching Apple Metal pack can be offered.
- Windows/Linux x64 with a detected GPU: the matching Vulkan pack can be offered as an alternative accelerator.
- A CPU-only or unsupported device does not receive an irrelevant accelerator download button.

The CUDA package is large, so it is deliberately excluded from the base setup. The app downloads it into user app storage and verifies its SHA-256 before activation.

### 8. First transcription

1. Paste a supported video link.
2. Select **Check video**.
3. Confirm that the metadata preview appears.
4. Download and select a model.
5. Leave **Compute backend** on **Auto** unless you need a specific backend.
6. Choose a transcription language or **Auto detect**.
7. Select **Transcribe now**.

The internal pipeline is:

```text
Supported video source
  ↓ yt-dlp bestaudio
original audio stream
  ↓ FFmpeg
WAV PCM s16le / mono / 16 kHz
  ↓ whisper.cpp
segments + timestamps
  ↓
JSON + TXT + SRT + VTT + SQLite history
```

## Login-protected videos

Log in to the relevant platform first. In WhisperTube, open **Settings → Source access** and import a fresh Netscape-format `cookies.txt` file. On macOS, a detected Safari session can also be tried directly. Then return to Transcribe and select **Check video**.

WhisperTube does not request site credentials and does not copy cookies into its database. `yt-dlp` reads the imported local cookies file only while the process runs. Extractor, authentication, anti-bot, and platform changes can still affect protected downloads; keeping `yt-dlp` current is the first line of defense.

On Windows, Chromium-based browser encryption can prevent yt-dlp from decrypting a Brave/Chrome/Edge profile, so the manual cookies file is the primary path. On macOS, Safari cookie storage may require permission from macOS. The file/session is used only by the local WhisperTube process.

## Build a Windows installer

After development testing succeeds:

```powershell
.\scripts\build-windows.ps1
```

The installer is produced under:

```text
src-tauri\target\release\bundle\
```

`npm run tauri:build` is the full release build: it compiles Rust in release mode, builds the frontend, bundles runtime resources, and creates the Windows installer. It is not required for every development test.

## Build macOS and Linux bundles from source

The Windows installer is not the only build path. macOS and Linux must be
built on their native host environments because Tauri uses the host's native
WebView and packaging toolchain. The repository includes native bootstrap and
build scripts for both platforms.

### macOS

Install Xcode Command Line Tools, Homebrew, Rust, and Node.js first. Then run
from the repository root:

```bash
xcode-select --install
chmod +x scripts/setup-macos.sh scripts/build-macos.sh
./scripts/setup-macos.sh
./scripts/build-macos.sh
```

The setup builds separate CPU and Metal whisper engines. The application
bundles are written under `src-tauri/target/release/bundle/`, including the
native `.app` and `.dmg` outputs when supported by the host.

### Linux (Ubuntu/Debian)

Install Tauri's system dependencies first:

```bash
sudo apt-get update
sudo apt-get install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev patchelf xdg-utils cmake pkg-config git nasm xz-utils
```

Then run:

```bash
chmod +x scripts/setup-linux.sh scripts/build-linux.sh
./scripts/setup-linux.sh
./scripts/build-linux.sh
```

The Linux script builds a pinned static FFmpeg and CPU whisper engine; the
application script requests Debian and AppImage bundles. Outputs are written under
`src-tauri/target/release/bundle/`. Other distributions need their equivalent
WebKitGTK, compiler, OpenSSL, FFmpeg, and packaging packages.

The Linux bundle should be built on the oldest supported base distribution;
glibc compatibility can prevent a bundle built on a newer distribution from
running on an older one.

The optional `.github/workflows/build-application-bundles.yml` repeats the
native builds on GitHub-hosted Windows, macOS, and Linux runners. Run it
manually to get short-lived workflow artifacts, or push an application tag such
as `v0.1.2` to publish the native NSIS, DMG, Debian, and AppImage installers in
a GitHub Release. Updater artifacts use Tauri signatures for update verification;
Windows has no Authenticode signing yet, and macOS uses a free ad-hoc signature
but is not Apple-notarized in v0.1.
Each installer is accompanied by a SHA-256 sidecar. The application bundles
also contain the project license and third-party notices.

## Accelerator releases for maintainers

`.github/workflows/build-accelerator-packs.yml` builds Metal and Vulkan packs from a pinned official whisper.cpp tag. Run the workflow manually to test artifacts, or push a tag such as `accelerators-v0.1.0` to publish a GitHub Release with SHA-256 sidecars.

The repository can remain private during development and CI. Before shipping an EXE, the accelerator release must be public because the app does not embed a GitHub token. CUDA Windows downloads continue to use the pinned upstream whisper.cpp release.

After the accelerator release is public, maintainers must copy its verified hashes into the application build:

```powershell
.\scripts\sync-accelerator-hashes.ps1
.\scripts\sync-accelerator-hashes.ps1 -Apply
```

Review the source diff, run the release checks, and rebuild the application. The first command is read-only; `-Apply` enables downloads for the four matching platform packs. Do not replace release assets after the application has been built.

## Project structure

```text
WhisperTube/
├─ src/
│  ├─ components/          # UI components
│  ├─ assets/fonts/        # Bundled interface fonts
│  ├─ i18n.tsx             # English/Indonesian/Chinese UI translations
│  ├─ hooks/               # Application state and actions
│  ├─ services/            # Tauri IPC boundary
│  └─ styles.css
├─ src-tauri/
│  ├─ src/                  # Rust commands and domain modules
│  ├─ capabilities/
│  ├─ runtime/              # Development runtime downloads
│  ├─ Cargo.toml
│  └─ tauri.conf.json
├─ scripts/
├─ THIRD_PARTY_NOTICES.md
├─ README.md               # English, default documentation
├─ README_ID.md            # Indonesian documentation
└─ README_CN.md            # Simplified Chinese documentation
```

## Local data

Tauri selects the platform-specific app-local-data directory. WhisperTube stores model files, job data, exports, and `whispertube.db` there. Temporary source audio and WAV files are deleted when `Keep processed audio` is off.

## Security decisions

- Source URLs are restricted to an explicit allowlist of supported video hosts before downloader execution.
- The frontend has no arbitrary shell execution capability.
- External processes are launched by Rust with argument arrays, not concatenated shell commands.
- Known model and accelerator downloads are verified with checksums before activation.
- Browser discovery reads local profile metadata; imported cookies and the optional Safari session are read by yt-dlp only for the selected job.
- Only one transcription job and one runtime installer can run at a time; job reservation is independent of child-process PID state.
- Cancellation terminates the active process tree on Windows with `taskkill /T /F`.
- Failed jobs clean up their temporary job folders, and runtime bootstrap downloads are pinned and checksum-verified before activation.

## Known limitations

1. The pinned CUDA bootstrap currently targets Windows x64 with a detected NVIDIA driver.
2. Metal and Vulkan packs require matching public GitHub Release assets and are not bundled into the base source setup.
3. Browser support depends on yt-dlp's current cookie extraction support and the browser's OS security behavior; direct Safari access is macOS-only and may require permission.
4. Login-protected downloads can break when a platform changes authentication, anti-bot, or extractor requirements.
5. There are no playlist/batch jobs, speaker diarization, word-level subtitle editing, or runtime/model auto-updates yet. App updates are handled by the signed Tauri updater; see [`docs/UPDATER.md`](docs/UPDATER.md).
6. macOS/Linux builds are available from native scripts and CI. macOS uses an ad-hoc signature but is not notarized; Windows/Linux signing and broad distro/hardware QA are not complete in v0.1.
7. macOS/Linux bootstrap builds FFmpeg statically from a pinned source archive; clean-machine redistribution and license QA are still required.

## License

WhisperTube source code is MIT-licensed. Runtime components and bundled fonts retain their upstream licenses; see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
