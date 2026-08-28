# Troubleshooting

## `rustc` / `cargo` not recognized

Install Rust with rustup, close all PowerShell windows, open a new one, then run:

```powershell
rustc --version
cargo --version
```

## Tauri linker / MSVC error

Install Visual Studio Build Tools 2022 with **Desktop development with C++** and a Windows SDK. Then reboot or open a fresh terminal.

## `setup-windows.ps1 cannot be loaded because running scripts is disabled`

For the current PowerShell process only:

```powershell
Set-ExecutionPolicy -Scope Process Bypass
```

Then rerun the setup script.

## Video public works but Member-only fails

1. Confirm the selected browser is logged into the Google account that owns the membership.
2. Try Firefox if Chromium cookie extraction is blocked by a browser/OS security change.
3. Run the setup script again to refresh yt-dlp to the newest release.
4. YouTube may have changed PO Token/auth requirements; capture the exact yt-dlp error shown by WhisperTube.

## `CUDA selected but NVIDIA GPU/driver not detected`

Run:

```powershell
nvidia-smi
```

If that command fails, fix/update the NVIDIA driver first. Installing the full CUDA developer toolkit is not required by WhisperTube's prebuilt engine pack.

## CUDA engine missing

```powershell
.\scripts\install-cuda-engine.ps1
```

Restart the app after it finishes.

## Model checksum mismatch

Do not bypass the check. Delete the partial model from the UI and download again. A mismatch can mean an incomplete/corrupted download or an upstream model artifact changed unexpectedly.

## First compile feels heavy

`npm run tauri:dev` compiles the Rust dependency graph the first time. Subsequent incremental builds are much faster.

## Full diagnosis

```powershell
.\scripts\diagnose-windows.ps1
```
