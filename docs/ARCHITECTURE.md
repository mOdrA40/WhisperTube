# WhisperTube Architecture

## Product boundary

WhisperTube is a local-first Tauri desktop application. The React WebView
does not receive arbitrary shell access. It can invoke only the Rust commands
explicitly registered by Tauri.

    React UI
      │ typed invoke/event payloads
      ▼
    Tauri IPC
      ▼
    Rust orchestration
      ├─ validates supported video URLs and local cookie paths
      ├─ owns child-process lifecycle and cancellation
      ├─ owns model paths and checksum verification
      ├─ owns SQLite history
      └─ owns transcript export copying
           │
           ├─ yt-dlp
           ├─ FFmpeg
           └─ whisper.cpp CPU/CUDA/Metal/Vulkan engines

Audio and transcript processing remain on the user's device. Network access is
used for source inspection, media downloads, model downloads, accelerator
downloads, and updater checks.

## Source layout

The frontend follows a one-way state flow:

- App.tsx assembles the application shell and pages;
- useWhisperTube owns state, side effects, operation coordination, and event
  subscriptions;
- services/tauri.ts is the typed IPC adapter;
- i18n.tsx provides English, Indonesian, and Simplified Chinese strings;
- components/ owns presentation and user callbacks.

The Rust backend uses similar boundaries:

- commands.rs contains Tauri command adapters;
- types.rs contains IPC DTOs and transcript limits;
- state.rs contains runtime operation state, cancellation flags, and guards;
- paths.rs resolves application-data, resource, model, job, and runtime paths;
- cookies.rs validates the user-selected cookies.txt path without reading or
  storing cookie contents;
- archive.rs centralizes bounded ZIP extraction and staged runtime copying;
- network.rs provides cancellable, timeout-bounded HTTP operations;
- models.rs manages model and optional CUDA downloads;
- sources.rs validates source URLs and performs metadata inspection;
- process.rs provides platform-specific child-process setup and termination;
- resources.rs performs memory, disk, and per-mount allocation checks;
- transcription.rs owns the media-to-transcript pipeline;
- history.rs owns SQLite history, result files, and exports;
- accelerators.rs downloads and activates Metal/Vulkan packs;
- system.rs detects hardware and collects telemetry;
- user_data.rs implements the application-data reset;
- lib.rs bootstraps Tauri and registers the commands.

## Transcription pipeline

Metadata inspection is a bounded operation that runs before a transcription.
The transcription state machine is:

    READY
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

An external stage can terminate in FAILED or CANCELLED.

The pipeline downloads bestaudio/best through yt-dlp, converts it with FFmpeg
to signed 16-bit PCM WAV, mono, 16 kHz, and passes the WAV to the selected
whisper.cpp engine. Results contain timestamped segments and are written as
JSON, TXT, SRT, VTT, and a SQLite history row.

The current full-buffer path accepts on-demand media with a known duration of
at most two hours and a source download limit of 4 GiB. Chunked transcription
is not implemented. The converted WAV size is checked again before inference,
and transcript text, segment count, serialized result size, and output files
have hard limits.

## Operation coordination

AppState exposes one exclusive OperationGuard for destructive and long-running
operations. The guard covers:

- metadata inspection;
- transcription;
- model download and deletion;
- runtime installation;
- history reads, deletion, export, and audio reveal;
- application update;
- application-data reset.

A transcription reserves the Transcribing state before spawning the blocking
pipeline. Each other operation reserves its own state. The guard releases its
reservation on every return path, so an active child-process PID is used for
termination, not as the sole concurrency lock.

The cancellation path sets the relevant cancellation flag and terminates the
active process tree where supported. Child processes are also cleaned up during
application shutdown.

job-progress events carry the current stage, progress percentage, transferred
bytes, network speed, and best-effort CPU/GPU utilization. Unavailable metrics
are reported as unavailable instead of being guessed.

Metadata inspection has a bounded 45-second process timeout and bounded output
retention. Download and inference paths use inactivity watchdogs; CPU
inference receives a longer allowance than downloads, conversion, and
accelerated inference.

## Storage model

WhisperTube-owned application data is stored under the platform-specific
Tauri application-local-data directory:

    models/
      ggml-base.bin
      ggml-large-v3-turbo-q5_0.bin
      ggml-large-v3-q5_0.bin
    jobs/
      <uuid>/
        audio.wav
        transcript.json
        transcript.txt
        transcript.srt
        transcript.vtt
        result.json
    runtime/
    whispertube.db

audio.wav is retained only when Keep processed audio is enabled. Source audio
and converted WAV files are removed after processing by default.

Each job receives an .in-progress marker. A failed or cancelled pipeline
cleans up its job directory. At startup, orphaned or staging directories older
than the 24-hour grace period are removed unless a valid result file still
belongs to a history row.

History creation writes result.json atomically and commits the SQLite row only
after the result file is ready. History deletion first renames job directories
to same-filesystem staging names, commits the database deletion, and then
removes the staged directories. If the database operation fails, the code
attempts to restore the original directories.

The Settings reset operation is intentionally limited to WhisperTube-owned
application data: models, jobs, runtime, and whispertube.db. The frontend also
clears its saved cookie-path preference from local storage.
It does not delete the WebView profile/cache, interface-language preference,
external cookies.txt files, or exports saved outside WhisperTube storage.
Job storage is capped at 20 GiB and the current usage is exposed in Settings.

## Runtime layout and verification

Development runtime binaries live under src-tauri/runtime/<platform> so Tauri
can bundle them as resources. In packaged builds, the bundled runtime is
resolved from the application resource directory. An installed user runtime
under application-local-data takes precedence over the bundled engine only
when its runtime manifest matches the expected whisper.cpp version and
executable SHA-256. Modified or legacy user runtimes are ignored until
reinstalled.

Windows bootstrap:

- yt-dlp 2026.08.19;
- FFmpeg 9.0.1 from the pinned Gyan.dev essentials archive;
- whisper.cpp v1.9.1 CPU binaries from the pinned upstream release;
- optional CUDA runtime installed on demand.

macOS bootstrap builds pinned FFmpeg and both CPU and Apple Metal whisper.cpp
engines. Linux bootstrap builds pinned static FFmpeg and a CPU whisper.cpp
engine. The Unix scripts pin whisper.cpp v1.9.1 to commit
f049fff95a089aa9969deb009cdd4892b3e74916.

Downloaded model files are SHA-256 verified before use. The verification cache
is valid only for the current file size and modification timestamp and is
cleared by a data reset.

Archives are downloaded to unique temporary staging paths, checked for size
and checksum limits, extracted with path-traversal protection, self-tested,
and activated only after validation succeeds. Stale installer staging and
temporary download directories older than 24 hours are cleaned at startup.

## Hardware and backend selection

The application distinguishes execution backends from models:

- CPU is the baseline fallback;
- CUDA is supported on Windows x64 with a detected NVIDIA GPU and an installed
  CUDA engine;
- Metal is supported on macOS with the matching Metal runtime;
- Vulkan is supported on Windows/Linux x64 with a matching Vulkan runtime.

NVIDIA device discovery uses nvidia-smi and preserves the selected device
index. Vulkan devices are enumerated by the selected whisper.cpp engine. The
compute selector exposes explicit targets such as cuda:<index> and
vulkan:<index> when available.

CUDA model guardrails use free NVIDIA VRAM as a conservative preflight:
approximately 2 GiB for Fast, 4 GiB for Balanced, and 7 GiB for Accurate.
These thresholds are not a universal guarantee because other GPU processes
can consume memory.

GPU telemetry follows the resolved target where the platform exposes a reliable
mapping. CUDA uses the selected NVIDIA device index. Vulkan and unsupported
cross-adapter mappings report unavailable instead of showing another GPU's
usage. The execution monitor also checks available system memory and
child-process RSS during download, conversion, probing, and inference.

Auto and explicit GPU selection run a bounded capability probe with the
installed engine, model, and a generated one-second silent WAV. Only
successful probes are cached for the backend/device/engine/model signature
during the application session. Cancellation and timeout failures are retried.
If Auto CUDA fails, the application tries Vulkan and then CPU where available.
An explicitly selected GPU target fails closed with a diagnostic error.

For cross-vendor stability, Vulkan child processes disable the optional
cooperative-matrix shader path and flash attention. This avoids known driver
paths that can be exposed by some AMD devices but fail during inference.

## Browser and cookie handling

The user can provide a Netscape-format cookies.txt file. WhisperTube validates
the path and size, then passes it only to the local yt-dlp process. Browser
encryption, OS permissions, extractor changes, and platform anti-bot behavior
can still prevent protected downloads.

## Release workflows

The repository has two separate release paths:

1. build-application-bundles.yml builds Windows NSIS, macOS DMG, Linux Debian,
   and Linux AppImage installers for v* application tags. It also generates
   updater artifacts, SHA-256 sidecars, and latest.json for a tagged release.
2. build-accelerator-packs.yml builds macOS Metal and Windows/Linux Vulkan
   packs from pinned whisper.cpp source and publishes ZIP files with SHA-256
   sidecars for accelerators-v* tags.

Both release workflows call the reusable CI quality gate before native build
jobs. Accelerator assets must be public before a shipped application can
download them without a GitHub credential.

The accelerator hash helper reads the four public release assets without
changing source by default. Its Apply mode writes verified hashes into the
application catalog. The application must be rebuilt after that change, and
published accelerator assets must remain immutable for the lifetime of the
application build that references them.

## Known production work

1. Sign runtime and model manifests and release assets where appropriate.
2. Add dedicated ROCm and OpenVINO packs with compatible build runners.
3. Persist the job queue and crash recovery state.
4. Add playlist and batch processing.
5. Add local-file drag and drop.
6. Add VAD model management and VAD-specific controls.
7. Add optional speaker diarization.
8. Add timestamp seeking with an embedded audio player.
9. Configure Windows/macOS signing and macOS notarization.
10. Replace development runtime bundling with per-platform release manifests
    so installers do not contain unnecessary engines.
