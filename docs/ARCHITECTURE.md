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
- cookies.rs validates and copies a user-selected cookies.txt into temporary
  application-owned storage for yt-dlp, then removes the internal copy;
- archive.rs centralizes bounded ZIP extraction and staged runtime copying;
- network.rs provides cancellable, timeout-bounded HTTP operations;
- io_cancel.rs provides bounded, cancellable copying and hashing;
- runtime_snapshot.rs validates runtime integrity once and checks file identity
  before subsequent transcription sections;
- job_cache.rs lists and selectively discards unfinished jobs;
- models.rs manages model and optional CUDA downloads;
- sources.rs validates source URLs and performs metadata inspection;
- process.rs provides platform-specific child-process setup and termination;
- durable.rs publishes flushed files and synchronizes directory entries;
- resources.rs performs memory, disk, and per-mount allocation checks;
- transcription.rs owns the media-to-transcript pipeline;
- history.rs owns SQLite history, result files, and exports;
- history_files.rs owns file inventory, safe location reveal, and durable partial
  deletion of saved audio or transcripts;
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

Conversion aborts on FFmpeg decode errors. Result and history duration use the
validated PCM sample count. When available audio differs from source metadata
by more than two seconds, result.json retains `sourceDuration` and the UI says
that only the available audio was transcribed. A genuinely shorter audio track
remains usable. Conversion reads one second beyond the accepted duration window
as a bounded sentinel; audio exceeding metadata by more than two seconds or the
eight-hour limit fails validation rather than being silently truncated to the
metadata duration. Cache/checkpoint matching retains the source duration.

The pipeline accepts on-demand media and completed live replays
with a known duration of at most eight hours and a source download limit of
4 GiB. Active and upcoming live streams remain unsupported. checkpoint.rs
divides normalized audio into approximately 300-second sections (maximum
301 seconds), preferring quiet boundaries. A successful section is validated
and checkpointed atomically; matching completed sections are reused on retry.
Before saving fresh engine output, a final segment that starts within the real
audio and spans at most one 30-second decoder window has its endpoint clipped
to the chunk duration if necessary. Earlier outliers, malformed, reversed or
unordered ranges still fail validation; saved checkpoints are validated without
this repair.
The model reloads per section, and boundary recognition can differ from
whole-file inference. The converted WAV size is checked again
before inference, and transcript text, segment count, serialized result size,
and output files have hard limits.

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
pipeline. Each other operation reserves its own state. Model/runtime reservations
cover both preflight and transfer.
Blocking preflight owns the guard and checks cancellation before and after its
work, so cancelling during preflight prevents the subsequent transfer. The
guard also remains held until that worker finishes if its IPC future is dropped.
The guard releases its reservation on every return path, so an active child-process PID is used for
termination, not as the sole concurrency lock.

The cancellation path sets the relevant cancellation flag and terminates the
active process tree where supported. Child processes are also cleaned up during
application shutdown.

Cancellation resolves the PID through the owned-process registry. On Unix,
waitid with WNOWAIT observes leader exit without releasing its PID/PGID;
remaining group members are killed before the leader is reaped. On Windows,
the owned Job is terminated on leader exit. Piped execution observes leader
exit independently of stream EOF, then drains buffered output after cleanup.

job-progress events carry the current stage, progress percentage, transferred
bytes, network speed, and best-effort CPU/GPU utilization. Unavailable metrics
are reported as unavailable instead of being guessed.
Download speed uses yt-dlp's current transfer rate, which excludes the resume
offset; the cumulative downloaded-byte counter still includes cached bytes.

Each metadata extractor invocation has a 45-second process timeout and bounded
output retention. For missing duration on concrete Instagram Reels and LinkedIn
post VOD routes, inspection downloads at most 32 MiB of audio to a bounded
buffer under a separate 45-second watchdog, then decodes a temporary local file
to a null sink under a 10-second watchdog. Machine-readable completed FFmpeg
progress supplies measured duration; container text metadata is not trusted.
Only local-file protocols are enabled for this probe. Known live/upcoming/
processing streams and media above eight hours remain rejected. Other routes
with unknown duration remain rejected. No new executable/dependency is bundled.
Temporary files are removed on return; crash leftovers use the existing
UUID-scoped, 24-hour transient cleanup. The transcription download applies the
same source-aware filter and captures extractor duration at before_dl. If duration
is still missing, it rechecks actual audio before conversion, rejects a difference
above two seconds and uses measured duration for small rounding/codec-padding
differences. A known extractor duration keeps the normal long-VOD path without
an extra full decode under the probe watchdog. Inspection audio is not a persistent cache.
Download and inference paths use inactivity watchdogs; CPU
inference receives a longer allowance than downloads, conversion, and
accelerated inference.

Metadata errors distinguish explicit login, HTTP 403/session blocking, transient
network/service errors and extractor parse failures. These codes, including
unknown-duration errors, are preserved through ErrorPayload into localized UI
messages; HTTP 403 alone is not treated as proof that a signed-in account is
required. No unproven browser-profile or IPv4 retry is enabled globally.

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

Failed/cancelled jobs retain downloads, completed WAV conversion, and validated
inference checkpoints for retry, independently of the final audio preference.
The transcription action checks persisted cache availability for the inspected
source when idle and after operations finish. It offers Continue for a valid
cached WAV or nonempty saved download data, including after reopening the app.
Metadata alone and committed history do not qualify. The hint distinguishes
download reuse from audio reuse; completed inference sections are reused only
after the pipeline checks their settings and fingerprint. This advisory lookup
does not hash the whole WAV or promise a specific completed percentage.
An unsuccessful attempt without reusable data offers Try again rather than
claiming that it can continue saved work. No retry starts automatically.
On success, source downloads and checkpoints are removed; audio.wav remains
only when Keep audio after completion is enabled.

History offers audio-only deletion, transcript-only deletion, and deletion of
the complete history item. Partial deletion records a tombstone and pending
cleanup in `history_file_deletions` before unlinking files. Cleanup retries on
startup and reports locked files as pending. Audio-only deletion preserves
JSON/TXT/SRT/VTT; transcript-only deletion removes those plus recognized result/
export staging files and residual checkpoint text, while retaining WAV and the
SQLite metadata row. Loading a transcript-only
deleted item returns metadata and remaining audio, with transcript actions
disabled. Full history deletion removes its tombstones in the same transaction.
Existing history rows and result JSON remain compatible; missing
`transcriptAvailable` in legacy results defaults to true. Exported external
copies are never selected by these deletion operations. The saved-files dialog
lists WAV/TXT/SRT/VTT sizes and paths and reveals only validated internal files.

Each job receives an .in-progress marker. A failed or cancelled pipeline
preserves its job directory for retry. At startup, unreferenced jobs older than
the 24-hour grace period are removed. A committed history row protects its job
directory even if result.json is missing or corrupt; cleanup reports that
problem and preserves recoverable artifacts.

History creation writes result.json atomically and commits the SQLite row only
after the result file is ready. History deletion commits pending_job_deletions
intent and row deletion in one SQLite transaction, then removes owned job
directories. Startup replays committed deletion intent after interruption.
Rollback preserves both rows and files. Export copies to a temporary sibling,
flushes the new file, and publishes it with rename; a failed copy preserves
the old destination. Unix replacement exports preserve the target's owner,
group, mode and access ACL before writing transcript bytes; an inability to
preserve those permissions aborts the export and removes staging. Publication
syncs parent directories after rename on Unix; Windows uses MoveFileExW with
REPLACE_EXISTING and WRITE_THROUGH. Result/checkpoint writers use the same
publisher and propagate sync failures before acknowledging success or committing
new history. A sync failure after rename can leave the new file in place while
the operation reports failure. This does not guarantee survival of physical
power loss on every filesystem or storage device. Linux NFSv4 ACL destinations are rejected
until their access policy can be preserved. macOS staging uses an explicit
non-inheriting ACL at creation to prevent inherited grants during preparation.

The Settings reset operation is intentionally limited to WhisperTube-owned
application data: models, jobs, runtime, internal cookie-jars, whispertube.db
and its recovery sidecars. Recovery files remain when the main database cannot
be removed; reset reports failure instead of reopening a new database with
old recovery files. The frontend also
clears its saved cookie-path preference from local storage.
If a reset fails after partial cleanup, the frontend drops its active transcript
and cache hints before reconciliation, because backing files may be gone. An
operation conflict rejected before cleanup preserves the active transcript.
It does not delete the WebView profile/cache, interface-language preference,
external cookies.txt files, or exports saved outside WhisperTube storage.
Job storage is capped at 20 GiB and the current usage is exposed in Settings.
Settings also lists unfinished job caches, including their size and source
when metadata is available. Discarding one requires explicit confirmation and
an exclusive storage reservation; the backend rechecks committed history
references before removing the selected UUID directory. This recovers quota
without resetting models or unrelated history. Deleting progress requires a
fresh download/conversion/inference for that job on the next retry.

## Runtime layout and verification

YouTube inspection and fresh downloads require Node.js 22+ or Deno 2.3+.
The installer supplies yt-dlp EJS scripts, but the JavaScript runtime is an
explicit external prerequisite. The backend probes the runtime version and
returns a dedicated source error when it is unavailable. Other supported
sources and inference from a reusable WAV do not require this prerequisite.

Development runtime binaries live under src-tauri/runtime/<platform> so Tauri
can bundle them as resources. In packaged builds, the bundled runtime is
resolved from the application resource directory. An installed user runtime
under application-local-data takes precedence over the bundled engine only
when its runtime manifest matches the expected whisper.cpp version and
the SHA-256 of every runtime file. Modified or legacy user runtimes are ignored until
reinstalled. A transcription owns a verified runtime snapshot across chunks:
each spawn checks the full file inventory, identity and change metadata,
without rehashing unchanged dependencies. Windows read-sharing handles deny
runtime writes/deletion during the job; Unix checks include inode and ctime.
Filesystems lacking stable identities retain full hash checks. This does not
make a user-writable manifest an authenticated signature or eliminate every
filesystem race against a malicious local actor.

The Windows developer-only CUDA helper stores its optional engine under
runtime-dev/windows/cuda. This directory is intentionally outside the bundled
runtime staging tree, so installing CUDA for local development cannot silently
inflate a release installer or disable the application's download-on-demand
behavior. Packaged applications install CUDA into application-local-data via
the UI instead.

Windows bootstrap:

- yt-dlp 2026.08.19;
- FFmpeg 9.0.1 from the pinned Gyan.dev essentials archive;
- whisper.cpp v1.9.2 CPU binaries from the pinned upstream release;
- optional CUDA runtime installed on demand.

macOS bootstrap builds pinned FFmpeg and both CPU and Apple Metal whisper.cpp
engines. Linux bootstrap builds pinned static FFmpeg and a CPU whisper.cpp
engine. The Unix scripts pin whisper.cpp v1.9.2 to commit
306c88f4d1286aec1bf96e544632897886af5501. The published accelerator pack is
released separately as accelerators-v0.1.3 and uses whisper.cpp v1.9.2.
User-runtime manifests include the accelerator pack generation, so a legacy
runtime cannot bypass a corrected catalog merely by matching the engine version.

Downloaded model files are SHA-256 verified before use. The verification cache
is valid only for the current file size and modification timestamp and is
cleared by a data reset.
Accelerator inference probes use the same verifier: an invalid model is skipped
in favor of the next valid candidate. If models exist but none verify, installation
reports a model error rather than misdiagnosing all GPU devices as incompatible.

Archives are downloaded to unique temporary staging paths, checked for size
and checksum limits, extracted with path-traversal protection, self-tested,
and activated only after validation succeeds. Hashing, extraction, traversal,
copying, and manifest generation check cancellation between bounded I/O blocks.
The final check before rename is the activation commit boundary: cancellation
after commit does not undo an installed runtime. Stale installer staging and
temporary download directories older than 24 hours are cleaned at startup.

## Hardware and backend selection

The application distinguishes execution backends from models:

- CPU is the baseline fallback;
- CUDA is supported on Windows x64 with a detected NVIDIA GPU and an installed
  CUDA engine;
- Metal is supported on macOS with the matching Metal runtime;
- Vulkan is supported on Windows/Linux x64 with a matching Vulkan runtime.

NVIDIA device discovery uses one nvidia-smi query for device data and driver
version and preserves the selected device index. Executable discovery is
cached until a query fails; hardware values are queried afresh. Generic Windows
adapter discovery uses EnumDisplayDevicesW without a PowerShell subprocess.
Vulkan devices are enumerated by the selected whisper.cpp engine, with
known software adapters such as llvmpipe, lavapipe, SwiftShader, and WARP
filtered out. The compute selector exposes explicit targets such as
cuda:<index> and vulkan:<index> when available.

CUDA model guardrails use free NVIDIA VRAM as a conservative preflight:
approximately 2 GiB for Fast, 4 GiB for Balanced, and 7 GiB for Accurate.
An explicit CUDA target uses only that adapter's telemetry. Unknown memory or
a disappeared adapter cannot borrow the default GPU's available-memory value.
These thresholds are not a universal guarantee because other GPU processes
can consume memory.
CUDA installation additionally requires a detected Windows x64 NVIDIA GPU and
an NVIDIA driver compatible with the packaged CUDA 12.x runtime; the current
minimum driver gate is 528.33.

Node availability probes cache successful results for the resolved executable,
size, timestamp and permissions. Failed probes remain retryable. yt-dlp receives
the probed absolute executable path when representable; non-Unicode paths or
platform executable lookup outside PATH retain uncached legacy lookup.

GPU telemetry follows the resolved target where the platform exposes a reliable
mapping. CUDA uses the selected NVIDIA device index. Linux reports Vulkan GPU
busy time only when exactly one measurable DRM adapter exists; multi-adapter
and macOS mappings report unavailable instead of showing another GPU's usage.
Vulkan has no portable dedicated-VRAM budget in the engine listing. Integrated
Vulkan devices therefore receive a conservative shared-system-memory guard,
while discrete devices rely on capability probing plus the runtime memory
monitor. The execution monitor also checks available system memory and
child-process RSS during download, conversion, probing, and inference.
The Vulkan installer also blocks before downloading when available host RAM is
below 2.5 GiB, the minimum preflight budget for the Fast model plus headroom.
Vulkan driver/device capability remains unknown before installation because
the application uses the downloaded whisper.cpp engine to enumerate devices;
the UI warns about this and the backend runs a real capability probe before
manual Vulkan inference.

Auto and explicit GPU selection run a bounded capability probe with the
installed engine, model, and a generated one-second silent WAV. Only
successful probes are cached for the backend/device/engine/model signature
during the application session. Cancellation and timeout failures are retried.
If Auto CUDA fails, the application tries Vulkan and then CPU where available.
If CUDA is not installed, Auto can use CPU directly; CUDA remains an optional
performance accelerator. An explicitly selected GPU target fails closed with
a diagnostic error.

For cross-vendor stability, Vulkan child processes disable the optional
cooperative-matrix shader path and flash attention. This avoids known driver
paths that can be exposed by some AMD devices but fail during inference.

## Browser and cookie handling

The user can provide a Netscape-format cookies.txt file. WhisperTube validates
the path and size and copies the file into cookie-jars/<uuid>/cookies.txt.
Only the internal copy is passed to local yt-dlp, which may update it.
Normal completion removes the copy; startup removes stale copies left by a
crash. Reset and Windows uninstall with data removal also remove internal jars
without touching the original export or the WebView profile. Browser
encryption, OS permissions, extractor changes, and platform anti-bot behavior
can still prevent protected downloads.

## Release workflows

Native source builds share scripts/whisper-distribution.cmake. It disables
runner-native ISA selection and optional x86 features beyond the x86-64 SSE2
baseline, and sets an ARMv8-A baseline for AArch64. This favors compatibility
over peak CPU throughput; GPU backends remain enabled by their build recipe.
OpenMP, external BLAS and optional llamafile CPU kernels are disabled to avoid
incidental host-specific dependencies. Metal shaders are embedded.
New builds use @loader_path/@rpath on macOS and $ORIGIN on Linux. Setup/pack
recipes remove build trees before staged executable self-checks. The accelerator
workflow also checks the staged binary load paths and rejects external OpenMP
runtime dependencies. Configuration-policy tests do not replace native
artifact/inference QA.

The application catalog pins the verified accelerators-v0.1.3 assets. The old
v0.1.2 Unix packs contain build-host load paths and must not be reinstated.
For a future pack generation, publish a new accelerator tag, inspect all four
packs, update RELEASE_TAG, the accelerator manifest generation, and SHA-256 pins.
The catalog/generation contract is tested. Never replace old assets or use an
application v* tag for this step.

See [Dependency audit status](../SECURITY.md#dependency-audit-status) for open inherited
Rust advisories and the evidence required to close them.

The repository has two separate release paths:

1. build-application-bundles.yml builds Windows NSIS, macOS DMG, Linux Debian,
   and Linux AppImage installers for v* application tags. It also generates
   updater artifacts, SHA-256 sidecars, and latest.json for a tagged release.
2. build-accelerator-packs.yml builds macOS Metal and Windows/Linux Vulkan
   packs from pinned whisper.cpp source and publishes ZIP files with SHA-256
   sidecars for accelerators-v* tags.

Both release workflows start their native build jobs directly. Tests, lint
and secret scans are run locally before pushing; there is no separate CI
test/lint quality gate. dependency-audit.yml runs npm/Cargo audits when dependency
files change; release jobs do not depend on that workflow. Application packaging
still builds its frontend assets. Accelerator assets must be public before a shipped application can
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
