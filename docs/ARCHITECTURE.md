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
  ├─ validates YouTube URL and browser enum
  ├─ owns process lifecycle / cancellation
  ├─ owns model paths and checksum validation
  ├─ owns SQLite history
  └─ owns export file copying
       │
       ├─ yt-dlp
       ├─ FFmpeg
       └─ whisper.cpp CPU/CUDA
```

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

The application selects CUDA only when both NVIDIA detection and the CUDA engine pack succeed. Otherwise `Auto` falls back to CPU.

## Next production milestones

1. Separate runtime/model component updater with signed manifests.
2. Build our own Vulkan engine pack in CI for AMD/Intel Windows GPUs.
3. Persist job queue and crash recovery.
4. Add playlist/batch processing.
5. Add local file drag-and-drop.
6. Add VAD model management and VAD-specific controls.
7. Add optional diarization.
8. Add timestamp seek with embedded audio player.
9. Sign Windows/macOS builds and notarize macOS.
10. Replace development runtime bundling with per-platform release manifests so installers do not contain unnecessary engines.
