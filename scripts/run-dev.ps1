$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

if (-not (Test-Path "node_modules")) {
    Write-Host "node_modules belum ada. Menjalankan npm install..." -ForegroundColor Yellow
    npm install
}

if (-not (Test-Path "src-tauri\runtime\windows\yt-dlp.exe")) {
    throw "Runtime belum disiapkan. Jalankan .\scripts\setup-windows.ps1 terlebih dahulu."
}

npm run tauri:dev
