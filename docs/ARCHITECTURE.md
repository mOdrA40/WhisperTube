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
  ├─ validates YouTube URL and selected browser/profile
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
`paths.rs` menangani lokasi storage/runtime, `browsers.rs` menemukan browser/profile
tanpa membaca cookies saat discovery, sedangkan `models.rs`, `youtube.rs`,
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

## Storage

App-local-data:

```text
models/
  ggml-base.bin
  ggml-large-v3-turbo-q5_0.bin
  ggml-large-v3-q5_0.bin
jobs/
  <uuid>/
    transcript.json
    transcript.txt
    transcript.srt
    transcript.vtt
    result.json
whispertube.db
```

Downloaded YouTube audio and converted WAV are removed by default after a successful job.

## Runtime packs

Development runtime binaries live under `src-tauri/runtime/<platform>` so Tauri can bundle them as resources.

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
architecture, and detected GPU before they reach the UI or installer.

The repository also contains `.github/workflows/build-accelerator-packs.yml`.
It builds Metal packs for macOS Intel/Apple Silicon and Vulkan packs for Linux
and Windows x64 from a pinned official whisper.cpp tag, then publishes ZIP
assets and SHA-256 sidecars on an `accelerators-v*` GitHub Release. The source
repository may remain private during development; those release assets must be
public before a shipped EXE can download them without a GitHub credential.
ROCm and OpenVINO remain separate targets because they require specialized
toolchains or hardware runners.

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
