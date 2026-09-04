# WhisperTube 0.1

WhisperTube is a local-first desktop app that turns YouTube videos into transcripts using `yt-dlp`, FFmpeg, and whisper.cpp. Audio and transcript processing stay on the user's device.

Other languages: [Bahasa Indonesia](README_ID.md) · [中文（普通话）](README_CN.md)

## Current scope

- Windows 10/11 x64 is the primary development and testing target.
- English is the default interface language. Indonesian and Simplified Chinese are also available from Settings.
- CPU inference is included in the development setup.
- NVIDIA CUDA is optional and installed separately so the base setup stays small.
- The app detects the current operating system, architecture, GPU, and installed browsers before offering optional components.

## Features

- Paste a YouTube URL and inspect metadata before downloading.
- Public videos.
- Member-only/authenticated videos through `yt-dlp --cookies-from-browser` using a detected local browser profile. The app never asks for a Google password.
- Browser discovery for Chrome, Edge, Firefox, Brave, Chromium, Opera, Vivaldi, Whale, and Safari on macOS. Settings only shows browsers and profiles detected on the current device.
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
- Model and YouTube download progress show transferred bytes, with cancellable model downloads.
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

1. Paste a YouTube link.
2. Select **Check video**.
3. Confirm that the metadata preview appears.
4. Download and select a model.
5. Leave **Compute backend** on **Auto** unless you need a specific backend.
6. Choose a transcription language or **Auto detect**.
7. Select **Transcribe now**.

The internal pipeline is:

```text
YouTube
  ↓ yt-dlp bestaudio
original audio stream
  ↓ FFmpeg
WAV PCM s16le / mono / 16 kHz
  ↓ whisper.cpp
segments + timestamps
  ↓
JSON + TXT + SRT + VTT + SQLite history
```

## Member-only YouTube videos

Log in to YouTube in a supported browser first. In WhisperTube, open **Settings → YouTube access** and choose the detected browser and profile containing the membership. Then return to Transcribe and select **Check video**.

WhisperTube does not request Google credentials and does not copy cookies into its database. `yt-dlp` reads the selected browser session only while the process runs. YouTube extractor, authentication, and PO Token changes can still affect authenticated downloads; keeping `yt-dlp` current is the first line of defense.

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

## Accelerator releases for maintainers

`.github/workflows/build-accelerator-packs.yml` builds Metal and Vulkan packs from a pinned official whisper.cpp tag. Run the workflow manually to test artifacts, or push a tag such as `accelerators-v0.1.0` to publish a GitHub Release with SHA-256 sidecars.

The repository can remain private during development and CI. Before shipping an EXE, the accelerator release must be public because the app does not embed a GitHub token. CUDA Windows downloads continue to use the pinned upstream whisper.cpp release.

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

- YouTube URLs are restricted to official YouTube hosts before downloader execution.
- The frontend has no arbitrary shell execution capability.
- External processes are launched by Rust with argument arrays, not concatenated shell commands.
- Known model and accelerator downloads are verified with checksums before activation.
- Browser discovery reads local profile metadata; cookies are read by yt-dlp only for the selected job.
- Only one transcription job and one runtime installer can run at a time.
- Cancellation terminates the active process tree on Windows with `taskkill /T /F`.

## Known limitations

1. The pinned CUDA bootstrap currently targets Windows x64 with a detected NVIDIA driver.
2. Metal and Vulkan packs require matching public GitHub Release assets and are not bundled into the base source setup.
3. Browser support depends on yt-dlp's current cookie extraction support and the browser's OS security behavior.
4. Member-only downloads can break when YouTube changes authentication or PO Token requirements.
5. There are no playlist/batch jobs, speaker diarization, word-level subtitle editing, or runtime auto-updates yet.
6. macOS/Linux bootstrap and installer QA are not as production-ready as the Windows path in v0.1.

## License

WhisperTube source code is MIT-licensed. Runtime components and bundled fonts retain their upstream licenses; see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
