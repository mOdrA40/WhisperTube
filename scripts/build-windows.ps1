$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

if (-not (Test-Path "src-tauri\runtime\windows\yt-dlp.exe")) {
    throw "Runtime belum disiapkan. Jalankan .\scripts\setup-windows.ps1 terlebih dahulu."
}

npm run tauri:build -- --bundles nsis
Write-Host "`nBuild selesai. Cek src-tauri\target\release\bundle\" -ForegroundColor Green
