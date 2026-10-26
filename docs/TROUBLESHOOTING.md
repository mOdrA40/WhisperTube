# Troubleshooting

## Start with evidence

Capture the exact error message, the application stage, the operating system,
the architecture, and the selected backend. Do not bypass a checksum,
permission, or security error just to make the operation continue.

On Windows, the diagnostic script collects local tool and runtime information:

    .\scripts\diagnose-windows.ps1

The script does not perform a transcription or upload private data.

## Local validation commands

The project does not define a separate npm lint script. Use TypeScript checking
for routine changes; a full frontend build is needed for production-output,
bundling, runtime-resource, or release changes.

Run the relevant checks from the project root:

    npm run typecheck
    npm run test:frontend
    node --test scripts/test-runtime-integrity.mjs scripts/test-native-inference.mjs scripts/test-runtime-loadpaths.mjs scripts/test-distribution-config.mjs scripts/test-cargo-advisories.mjs
    npm audit --audit-level=moderate
    cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
    cargo check --manifest-path src-tauri/Cargo.toml --all-targets --all-features
    cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
    cargo test --manifest-path src-tauri/Cargo.toml --all-targets --all-features
    .\scripts\check-secrets.ps1

The Rust dependency audit also requires cargo-audit:

    cargo install cargo-audit --locked
    cargo audit --file src-tauri/Cargo.lock

Run these checks locally before pushing; agents must follow AGENTS.md before
reporting completion. GitHub Actions has no separate dependency-audit or
quality-gate workflow. Use `node scripts/check-cargo-advisories.mjs` for the
stricter local advisory policy. The
packaging validator rejects stale yt-dlp executables by SHA-256, including
direct Tauri builds and standalone `tauri bundle`; non-debug Cargo builds enforce the same pin independently.
The application workflow also verifies a real one-second CPU inference with a
hash-verified base model before collecting/distributing artifacts. This is not
a substitute for driver-specific GPU or clean-machine installer QA.
Prepared release jobs explicitly run the otherwise opt-in native FFmpeg test
for successful short audio, corrupt partial input and overlong conversion.
Check Unix script syntax locally with `bash -n scripts/<script-name>.sh`.

For a local native CPU smoke check, use an existing `ggml-base.bin` (read-only):

    node scripts/verify-native-inference.mjs <path-to-ggml-base.bin>

The check verifies the model hash from the Rust catalog, generates its WAV and
JSON inside a disposable temporary directory, disables GPU use explicitly,
and requires structured transcript output as well as a successful process.
It never installs/updates the application or uses real transcription data.

Before declaring GPU artifacts ready, run short success/failure tests on each
shipping backend and inspect the pinned whisper.cpp attestation, not just exit
status. CUDA must select a physical UUID through CUDA_VISIBLE_DEVICES and use
child ordinal 0; Vulkan must attest the selected engine ordinal; Metal must
attest MTL initialization. Reject CPU fallback, missing attestation and GPU
initialization-failure logs. Hosted CPU-only checks do not establish this.

CPU telemetry is sampled on Windows/Linux. macOS currently reports an aggregate
moving average from ps and can lag the activity; the UI tooltip makes that
distinction explicit. A model's large resident memory is not, by itself, a leak.

## Isolated interface review

Run `npm run dev`, then open
`http://localhost:1420/src/test/ui-preview.html?scene=ready&lang=en`.
This explicit development entry renders the real presentation components with
synthetic data. It imports no application service or operation-owning hook,
does not read native user storage, and is excluded from the production entry.
Native window buttons are unavailable in this browser fixture.

Useful scenes include `empty`, `ready`, `inspecting`, `cancelling`,
`missing-model`, `warning`, `downloading`, `converting`, `transcribing`,
`finalizing`, `resume`, `resume-download`, `retry`, `checking-resume`,
`result`, `search-empty`, `audio-only`, `duration-mismatch`, `model-download`,
`history`, `history-empty`, `history-failure`, `settings`, `settings-update`,
`settings-model-download`, `accelerator-download`, `cuda-download`,
`driver-warning`, `runtime-missing`, `error`, `result-model-download`,
`update`, `update-installing`, `update-error`, `settings-update-installing`,
`settings-update-error`, `settings-update-checking`, and `settings-update-current`.
Set `lang` to `en`, `id`, or `zh`. Light/dark appearance
follows `prefers-color-scheme`; use browser emulation when reviewing both.

Check 1440×900, 1280×820, 1024×768, and 760×650, including keyboard navigation,
200% zoom, reduced motion, long device names and paths, and dialog focus.
Fixtures intentionally cannot validate inference, file operations, native
window controls, or update installation; verify those separately in Tauri.

## Node.js, Rust, or Cargo is not recognized

For an installed app, YouTube also needs Node.js 22+ or Deno 2.3+ on PATH;
neither JavaScript runtime is included in the installer. yt-dlp's EJS scripts
are included. Install a supported runtime and restart WhisperTube so it inherits
the updated PATH. Other supported sources remain available without it. The
YouTube preflight checks the runtime version before invoking the downloader.

Install Node.js 22 or newer and Rust with rustup. Close the current terminal,
open a new one, and confirm:

    node --version
    npm --version
    rustc --version
    cargo --version

## Runtime is incomplete

The application requires yt-dlp, FFmpeg, and the CPU whisper.cpp engine. On
Windows, run:

    .\scripts\setup-windows.ps1

On macOS or Linux, run the corresponding setup-macos.sh or setup-linux.sh
script from a terminal that has Bash. The setup scripts verify downloaded
runtime files and run a version self-check before activation.

Windows CPU/CUDA development setup activates verified staging with directory
renames on the runtime volume. If activation fails, it restores the old runtime.
If restoration is blocked, the error reports a retained `.cpu-backup-*` or
`.cuda-backup-*` directory beside the runtime. Preserve that backup until the
old runtime has been recovered; temporary-download cleanup does not delete it.
Cleanup warnings also report any staging/backup directory that needs attention.
Resolve CPU leftovers in the bundled runtime tree before packaging: the bundle
allowlist rejects them. CUDA helper backups stay in the separate runtime-dev
tree and are not bundled.

## Tauri linker or MSVC errors on Windows

Install Visual Studio Build Tools 2022 with the Desktop development with C++
workload, an MSVC compiler, and a Windows SDK. Reopen the terminal after the
installation.

## PowerShell refuses to run a local script

For the current PowerShell process only:

    Set-ExecutionPolicy -Scope Process Bypass

Then rerun the setup or diagnostic script. Do not disable the system-wide
execution policy merely to run WhisperTube.

## Public video works but a login-protected video fails

1. Confirm that the imported cookies.txt came from the account that can access
   the video.
2. Export a fresh Netscape-format cookies.txt file if the current one is old.
3. On Windows, browser encryption can prevent a cookies export from being
   usable; export a fresh Netscape-format cookies.txt file.
4. Run the setup script again if yt-dlp is outdated.
5. Capture the exact yt-dlp error. Platform authentication, anti-bot, and
   extractor behavior can change independently of WhisperTube.

WhisperTube does not request the website password and does not upload the
cookies file.

## Metadata inspection fails or times out

Check that the URL belongs to a supported host and that yt-dlp can reach the
network. Metadata inspection retries only recognized transient failures and
has a bounded 45-second process timeout. A login-protected source may require
an imported cookies file.

## Job storage quota or low disk space

Settings shows the current job storage usage and the 20 GiB safety quota.
Review unfinished jobs in Settings and discard only the temporary jobs you no
longer need, or delete old history items. Discarding an unfinished job removes
its retry progress; it does not delete installed models or committed history.
Turning off Keep audio affects successful jobs and does not clear existing retry
caches. WhisperTube also reserves space for the maximum media and WAV sizes
before downloading.

## CUDA is selected but NVIDIA is not detected

Run:

    nvidia-smi

If this command fails, repair or update the NVIDIA driver first. WhisperTube's
prebuilt CUDA engine does not require the full CUDA developer toolkit. CUDA is
available only on supported Windows x64 NVIDIA builds.

## CUDA engine is missing

Install the optional engine from Settings, or run:

    .\scripts\install-cuda-engine.ps1

Restart the application after installation. The downloaded archive is
checksum-verified and self-tested before activation.

The CUDA archive is large (roughly 670 MB for the pinned release). Connection
establishment remains bounded at 30 seconds; after the response starts, the
download is allowed up to 120 seconds without receiving a new chunk. This is
an inactivity watchdog, not a total download-duration limit. A timeout after a
long idle period usually indicates an unstable connection, proxy/VPN issue, or
an exhausted network path. Failed installs clean up their temporary archive and
staging directory. The application retries CUDA up to four times and resumes
from the partial archive when the server supports HTTP Range; retry after
checking the connection and available disk space.

CUDA installation is blocked when the NVIDIA driver is missing, unreadable, or
older than 528.33 on Windows. The packaged CUDA 12.x runtime does not require
the full CUDA developer toolkit, but it does require a compatible NVIDIA
driver. Update the driver before attempting the large download.

## Metal or Vulkan is unavailable

The application offers only a pack matching the current operating system,
architecture, and detected hardware. Metal is for macOS. Vulkan packs are for
Windows and Linux x64. A CPU-only or unsupported device will not receive an
irrelevant accelerator option.

If an accelerator download fails, verify that the public accelerator release
contains the expected asset and SHA-256 sidecar. The application has no GitHub
credential and cannot download a private release.

The Vulkan install button is disabled when available system RAM is below the
2.5 GiB preflight floor. This is a conservative minimum for the Fast model on
shared-memory Vulkan plus basic headroom; it is not a guarantee that every
model and video duration will fit. Close memory-heavy applications and use
Re-check components before retrying.

When RAM is sufficient, the UI warns that Vulkan compatibility cannot be fully
verified before the engine is installed. The installed engine then enumerates
real Vulkan devices and runs a capability probe with the selected model. Use
Auto to allow CPU fallback if that probe fails.

## Model checksum mismatch

Do not bypass the check. Remove the incomplete model from the application and
download it again. A mismatch can indicate an incomplete download, corruption,
or an unexpected upstream artifact change.

## A model is rejected because of VRAM

The CUDA guardrails use free NVIDIA VRAM as a conservative preflight check:
Fast requires about 2 GB, Balanced about 4 GB, and Accurate about 7 GB. These
are safety thresholds, not guarantees. Close other GPU applications or select
a lighter model or CPU.

## The first compile is slow

The first npm run tauri:dev compiles the Rust dependency graph. Later runs use
incremental compilation and should be faster.

## App update check or installation fails

Confirm that a stable application GitHub Release contains latest.json and the
required signed updater artifacts. Prereleases are not selected by the
releases/latest endpoint. An installation created before updater support needs
one manual installation of a build that includes the updater.

During installation, WhisperTube reserves the application operation so that
transcription, downloads, history mutation, and data reset cannot overlap with
the update.

## Closing the window during audio download or transcription

While a transcription job is active (including its audio download, conversion,
inference, and finalization), closing the main window or using Alt+F4 hides it to
the system tray instead of cancelling the job. The hidden window leaves the
taskbar. Click the WhisperTube tray icon to restore it; alternatively, open the
tray icon menu and select **WhisperTube**, or launch WhisperTube again.

When the job finishes, fails, or is cancelled, the window stays in the tray until
you restore it. Background outcome notifications still follow the rules below.
To stop an active job, restore the window and use **Cancel**. After the job has
settled, closing the window exits normally. Ordinary minimize behavior is
unchanged. Model/runtime downloads, video inspection, history operations, and an
idle window do not use close-to-tray.

The tray uses the application icon and is created only when needed. Windows may
put it in the hidden-icons overflow next to the clock. If tray creation fails,
WhisperTube keeps the window open so an active job is not lost. No installer,
startup entry, or permanent background service is added.

## No transcription outcome notification appears

One notification is sent when an accepted transcription (including a resumed job)
ends while the main WhisperTube window is not focused. Distinct localized titles
identify success, completion with an audio/source duration discrepancy, failure,
and cancellation. Cancellation notifications are silent on Windows. A late cancel
cannot turn a saved success into cancellation or conceal an unrelated failure.
Opening history, progress updates, and rejected concurrent start requests do not
notify. This policy applies to both Whisper and Qwen; downloads and other settings
operations do not produce transcription notifications.
The message is generic, follows the interface language, and omits video titles,
URLs, file paths, and transcript content.

Enable notifications for WhisperTube in the operating system and check Do Not
Disturb/Focus settings. On Windows, WhisperTube registers its own per-user
notification identity and uses it directly, including development and no-bundle
executables. The sender name is WhisperTube and the icon follows the current
application branding. This does not require administrator access or an installer.
The icon is stored under app-local-data `branding` with a content-derived filename
so a new build does not reuse an old icon path. Registry/filesystem failures are
reported to the application log; there is no silent PowerShell fallback.
OS policies may suppress or delay delivery even when dispatch succeeds. A notification failure never changes
the original success, failure, or cancellation result.

Notification dispatch runs in Rust after the pipeline returns its terminal result. It does
not require a foreground WebView timer or new JavaScript notification permissions.

## Qwen installation, CUDA, or subtitle timing

Qwen backend options follow its own platform-specific runtime, independently of
the installed Whisper Metal/Vulkan packs. Intel Macs support Qwen Auto/CPU;
Apple Silicon Macs also support Qwen Metal without installing Whisper's Metal
pack. Switching from Whisper Metal to Qwen on Intel resets the target to Auto.
Unsupported Qwen backends are rejected before audio download or conversion.

Select Alibaba in the provider dropdown and download Qwen3-ASR-1.7B Q8. The
cancellable download installs its hash-verified native runtime before the GGUF.
CUDA needs the existing NVIDIA acceleration component; CPU remains available.
The pinned Qwen CUDA runtime requires compute capability 7.5 or newer. A GPU
that works with Whisper CUDA (for example, a Pascal GTX 1080) may not support
Qwen CUDA. Auto uses CPU when the selected GPU is too old or its compute
capability cannot be verified; explicit CUDA reports the reason instead.
Qwen subtitle timestamps follow audio section boundaries and are approximate.
Whisper remains available through its own provider for aligned subtitles.
Qwen requires a rebuilt native application; a frontend-only preview does not
provide its worker.

When Qwen language is Auto, every section keeps automatic language detection.
It must not inherit a forced language from the first section: on the reported
outro audio, forcing Indonesian triggered native repetition status 19, while Auto
completed successfully without recognized words in the final portion. The full
previously failing 293.64-second section also completed with Auto on the same GPU.
Explicit language choices remain explicit. The native repetition guard is still
active; a failed or truncated decode is never silently accepted as complete.

Old Qwen Auto checkpoints use a different policy and are not reused after this
fix. Existing converted audio can still be reused without downloading the video
again. Whisper checkpoint identities and explicit-language Qwen identities are
unchanged. A truly undecodable section can still fail; this is not a guarantee
that Qwen recognizes every audio source correctly.

For a corrupt Qwen runtime, close active jobs, remove only
`runtime/<platform>/qwen-transcribe-0.4.1` under application-local data, then
redownload Qwen. Its model weights can be reused if their checksum is valid.

## EXE or taskbar still shows the old icon

The native PNG, ICO, and ICNS assets are generated from `public/logo.svg`, which
matches the current teal waveform UI mark. Rebuild and restart the native app;
a Vite-only refresh cannot replace an executable resource or a running window's
icon. Confirm the shortcut points to the rebuilt EXE. Windows may retain a pinned
shortcut icon; unpin and repin that application if needed. Do not delete global
Windows icon caches just to refresh this app.
