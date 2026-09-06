# WhisperTube architecture

## Trust boundary

The React WebView never receives arbitrary shell access. It can only call the Rust commands explicitly registered by Tauri.

```text
React UI
  │ typed invoke/event payloads
  ▼
Tauri IPC
  ▼
Rust orchestration
  ├─ validates supported video URL and local cookies file
  ├─ owns process lifecycle / cancellation
  ├─ owns model paths and checksum validation
  ├─ owns SQLite history
  └─ owns export file copying
       │
       ├─ yt-dlp
       ├─ FFmpeg
       └─ whisper.cpp CPU/CUDA
```

## Source layout

Frontend mengikuti alur satu arah: `App.tsx` menyusun halaman, `useWhisperTube`
memegang state dan side effect, `services/tauri.ts` menjadi adapter IPC,
`i18n.tsx` menyediakan English/Indonesia/Mandarin, dan komponen di
`components/` hanya menangani tampilan serta callback pengguna.

Backend memakai boundary serupa: `commands.rs` hanya adapter command Tauri,
`types.rs` menyimpan DTO IPC, `state.rs` menyimpan state job runtime,
`paths.rs` menangani lokasi storage/runtime, `browsers.rs` menangani kompatibilitas argumen cookie
tanpa membaca cookies saat discovery, memilih file-cookie args, dan menyediakan sesi Safari langsung di macOS, sedangkan `models.rs`, `sources.rs`,
`transcription.rs`, dan `history.rs` menangani domain masing-masing. `lib.rs`
hanya melakukan bootstrap aplikasi dan registrasi command.

## Transcription state machine

```text
READY
  ↓
METADATA
  ↓
DOWNLOADING
  ↓
CONVERTING
  ↓
TRANSCRIBING
  ↓
FINALIZING
  ↓
DONE
```

Any external stage can terminate in `FAILED` or `CANCELLED`.

The backend reserves the job lifecycle before spawning the blocking pipeline:
`Idle → Starting → Running → Cancelling`. A guard releases the reservation on
every return path, so a process PID is only used for process termination, not
as the job's sole concurrency lock.

`job-progress` carries stage progress, transferred bytes, network speed, and
best-effort CPU/GPU utilization samples. GPU utilization is shown when the
platform exposes a supported metric; unavailable metrics are reported as such
instead of being guessed.

## Storage

App-local-data:

```text
models/
  ggml-base.bin
  ggml-large-v3-turbo-q5_0.bin
  ggml-large-v3-q5_0.bin
jobs/
  <uuid>/
    audio.wav             # only when Keep processed audio is enabled
    transcript.json
    transcript.txt
    transcript.srt
    transcript.vtt
    result.json
whispertube.db
```

Downloaded source audio and converted WAV are removed by default after a successful job.
History creation writes `result.json` atomically inside the same logical operation
as the SQLite insert, and the transaction commits only after the result file is
ready. History deletion first moves job directories to a same-filesystem staging
name, commits the SQLite deletion, then removes the staged folders; failed
database operations attempt to restore the original directories.

Each new job has an `.in-progress` marker. A failed or cancelled pipeline guard
removes its folder, while startup removes stale orphan/staging folders after a
24-hour grace period without touching rows that still reference a valid result.
When `Keep processed audio` is enabled, the processed WAV path is returned and
the UI can reveal it in the system file manager.

## Runtime packs

Development runtime binaries live under `src-tauri/runtime/<platform>` so Tauri can bundle them as resources.
The Windows bootstrap pins yt-dlp 2026.08.19, FFmpeg 9.0.1, and whisper.cpp
v1.9.1; archives are downloaded to unique temporary staging paths, checksum
verified, self-tested, and activated only after validation succeeds.

Windows:

```text
runtime/windows/
  yt-dlp.exe
  ffmpeg.exe
  cpu/
    whisper-cli.exe
    ggml*.dll
  cuda/                  # optional
    whisper-cli.exe
    ggml*.dll
    CUDA runtime DLLs...
```

The packaged application can download the optional CUDA pack from the UI. It
stores the verified runtime under user app-local-data and prefers that path
before the bundled runtime, so it does not need write access beside the EXE.
The download is pinned to an upstream whisper.cpp release, checked with
SHA-256, extracted with path traversal protection, and self-tested before
activation. The request has a 30-second connection/response timeout, a
30-second timeout per data chunk, and a cancellation token checked between
download chunks.

The system status query reads NVIDIA name, total VRAM, and free VRAM through
`nvidia-smi`, and uses platform-specific graphics detection for other GPU
vendors. Model entries expose conservative CUDA guardrails: Fast requires
about 2 GB, Balanced 4 GB, and Accurate 7 GB of free VRAM. These are
preflight safety thresholds; actual available memory can change when other
GPU applications are running. CUDA is offered only on supported Windows x64
NVIDIA builds. Metal/Vulkan catalog entries are filtered by target platform,
architecture, and detected GPU before they reach the UI or installer. Vulkan is
kept as an explicit alternative even when CUDA is available, so NVIDIA users
can compare backends or use Vulkan when needed.

Windows CPU telemetry uses the native `GetSystemTimes` API. GPU telemetry is
sampled every two seconds to avoid making PowerShell part of the CPU measurement
loop. External child processes use hidden-console creation flags on Windows.

The repository also contains `.github/workflows/build-accelerator-packs.yml`.
It builds Metal packs for macOS Intel/Apple Silicon and Vulkan packs for Linux
and Windows x64 from a pinned official whisper.cpp tag, then publishes ZIP
assets and SHA-256 sidecars on an `accelerators-v*` GitHub Release. The source
repository may remain private during development; those release assets must be
public before a shipped EXE can download them without a GitHub credential. A
pack remains non-downloadable until its final SHA-256 is copied into the
application catalog and a new application build is produced; the release
sidecar and GitHub digest are only secondary consistency checks.
ROCm and OpenVINO remain separate targets because they require specialized
toolchains or hardware runners.

The hash synchronization helper is `scripts/sync-accelerator-hashes.ps1`. It
first reads the four public release assets without changing source; its
`-Apply` mode writes the checked hashes into the target-specific catalog
constants. A release build must be produced after that change, and the
published assets must remain immutable for the lifetime of that application
build.

Native application bundles use `scripts/setup-macos.sh` plus
`scripts/build-macos.sh` on macOS and `scripts/setup-linux.sh` plus
`scripts/build-linux.sh` on Linux. The corresponding
`.github/workflows/build-application-bundles.yml` runs those scripts plus the
Windows bootstrap/build scripts on native GitHub-hosted runners and publishes
the NSIS, DMG, Debian, and AppImage installers directly for tagged `v*`
application releases. Each installer has a SHA-256 sidecar. These bundles are
unsigned on Windows/Linux in v0.1. macOS uses an ad-hoc signature but is not
Apple-notarized; official signing and broad Linux distribution QA remain
release work.
The bootstrap builds FFmpeg 9.0.1 from a pinned official source archive with
`--disable-shared` and no network support, rather than copying a host package
manager binary. Linux additionally requests static linking. This removes the
Homebrew/distro FFmpeg library dependency, although clean-machine and license
QA are still required.

## Next production milestones

1. Sign runtime/model manifests and release assets.
2. Add ROCm/OpenVINO packs with dedicated compatible runners.
3. Persist job queue and crash recovery.
4. Add playlist/batch processing.
5. Add local file drag-and-drop.
6. Add VAD model management and VAD-specific controls.
7. Add optional diarization.
8. Add timestamp seek with embedded audio player.
9. Sign Windows/macOS builds and notarize macOS.
10. Replace development runtime bundling with per-platform release manifests so installers do not contain unnecessary engines.
