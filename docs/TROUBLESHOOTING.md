# Troubleshooting

## Start with evidence

Capture the exact error message, the application stage, the operating system,
the architecture, and the selected backend. Do not bypass a checksum,
permission, or security error just to make the operation continue.

On Windows, the diagnostic script collects local tool and runtime information:

    .\scripts\diagnose-windows.ps1

The script does not perform a transcription or upload private data.

## Local validation commands

The project does not define a separate npm lint script. The frontend build
performs TypeScript checking before the Vite production build.

Run the relevant checks from the project root:

    npm run build
    npm run test:frontend -- --run
    npm audit --audit-level=high
    cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
    cargo test --manifest-path src-tauri/Cargo.toml --all-targets --all-features
    cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
    .\scripts\check-secrets.ps1

The Rust dependency audit also requires cargo-audit:

    cargo install cargo-audit --locked
    cargo audit --file src-tauri/Cargo.lock

The GitHub Actions quality gate runs these checks on a clean Windows runner and
also runs Bash parser checks for the macOS and Linux scripts.

## Node.js, Rust, or Cargo is not recognized

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
3. On macOS, try the detected Safari session and grant macOS permission if
   requested.
4. On Windows, Chromium profile encryption can prevent yt-dlp from reading
   Chrome, Edge, or Brave profiles; the manual cookies file is the primary
   fallback.
5. Run the setup script again if yt-dlp is outdated.
6. Capture the exact yt-dlp error. Platform authentication, anti-bot, and
   extractor behavior can change independently of WhisperTube.

WhisperTube does not request the website password and does not upload the
cookies file.

## Metadata inspection fails or times out

Check that the URL belongs to a supported host and that yt-dlp can reach the
network. Metadata inspection retries only recognized transient failures and
has a bounded 45-second process timeout. A login-protected source may require
an explicitly selected browser session or cookies file.

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

## Metal or Vulkan is unavailable

The application offers only a pack matching the current operating system,
architecture, and detected hardware. Metal is for macOS. Vulkan packs are for
Windows and Linux x64. A CPU-only or unsupported device will not receive an
irrelevant accelerator option.

If an accelerator download fails, verify that the public accelerator release
contains the expected asset and SHA-256 sidecar. The application has no GitHub
credential and cannot download a private release.

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
