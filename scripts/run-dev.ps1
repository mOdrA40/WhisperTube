$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

if (-not (Test-Path "node_modules")) {
    Write-Host "node_modules belum ada. Menjalankan npm ci..." -ForegroundColor Yellow
    npm ci
    if ($LASTEXITCODE -ne 0) { throw "Instalasi dependency gagal (npm exit code $LASTEXITCODE)." }
}

if (-not (Test-Path "src-tauri\runtime\windows\yt-dlp.exe")) {
    throw "Runtime belum disiapkan. Jalankan .\scripts\setup-windows.ps1 terlebih dahulu."
}

npm run tauri:dev
if ($LASTEXITCODE -ne 0) { throw "Development server gagal (npm exit code $LASTEXITCODE)." }
