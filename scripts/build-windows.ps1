$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

if (-not (Test-Path "src-tauri\runtime\windows\yt-dlp.exe")) {
    throw "Runtime belum disiapkan. Jalankan .\scripts\setup-windows.ps1 terlebih dahulu."
}

$buildArgs = @('--bundles', 'nsis')
if ($env:TAURI_CREATE_UPDATER_ARTIFACTS -eq 'true') {
    if ([string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY) -and [string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY_PATH)) {
        throw 'TAURI_SIGNING_PRIVATE_KEY atau TAURI_SIGNING_PRIVATE_KEY_PATH wajib diisi saat membuat artifact updater.'
    }
    $buildArgs += @('--config', '{"bundle":{"createUpdaterArtifacts":true}}')
}

npm run tauri:build -- @buildArgs
Write-Host "`nBuild selesai. Cek src-tauri\target\release\bundle\" -ForegroundColor Green
