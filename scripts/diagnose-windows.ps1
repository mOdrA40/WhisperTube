$ErrorActionPreference = "Continue"
$Root = Split-Path -Parent $PSScriptRoot
$Runtime = Join-Path $Root "src-tauri\runtime\windows"

function Test-Cmd($Name) {
    $cmd = Get-Command $Name -ErrorAction SilentlyContinue
    if ($cmd) {
        Write-Host "[OK]   $Name -> $($cmd.Source)" -ForegroundColor Green
        return $true
    }
    Write-Host "[MISS] $Name" -ForegroundColor Red
    return $false
}

function Test-FileNice($Label, $Path) {
    if (Test-Path $Path) {
        $size = [math]::Round((Get-Item $Path).Length / 1MB, 1)
        Write-Host "[OK]   $Label -> $Path ($size MB)" -ForegroundColor Green
        return $true
    }
    Write-Host "[MISS] $Label -> $Path" -ForegroundColor Red
    return $false
}

Write-Host "WhisperTube diagnostics" -ForegroundColor Cyan
Write-Host "Project: $Root`n"

$node = Test-Cmd "node"
$npm = Test-Cmd "npm"
$rustc = Test-Cmd "rustc"
$cargo = Test-Cmd "cargo"
$linker = Test-Cmd "link.exe"

if ($node) { Write-Host "       $(node --version)" -ForegroundColor DarkGray }
if ($npm) { Write-Host "       npm $(npm --version)" -ForegroundColor DarkGray }
if ($rustc) { Write-Host "       $(rustc --version)" -ForegroundColor DarkGray }
if ($cargo) { Write-Host "       $(cargo --version)" -ForegroundColor DarkGray }
if (-not $linker) {
    Write-Host "       link.exe kadang hanya masuk PATH di Developer PowerShell. Tauri tetap dapat menemukannya jika Build Tools terpasang dengan benar." -ForegroundColor DarkYellow
}

Write-Host "`nRuntime binaries:" -ForegroundColor Cyan
Test-FileNice "yt-dlp" (Join-Path $Runtime "yt-dlp.exe") | Out-Null
Test-FileNice "FFmpeg" (Join-Path $Runtime "ffmpeg.exe") | Out-Null
Test-FileNice "whisper CPU" (Join-Path $Runtime "cpu\whisper-cli.exe") | Out-Null
Test-FileNice "whisper CUDA" (Join-Path $Runtime "cuda\whisper-cli.exe") | Out-Null

Write-Host "`nGPU:" -ForegroundColor Cyan
if (Test-Cmd "nvidia-smi") {
    & nvidia-smi --query-gpu=name,driver_version,memory.total --format=csv,noheader
} else {
    Write-Host "       NVIDIA tidak terdeteksi; CPU fallback tetap valid." -ForegroundColor DarkGray
}

Write-Host "`nFrontend dependencies:" -ForegroundColor Cyan
if (Test-Path (Join-Path $Root "node_modules")) {
    Write-Host "[OK]   node_modules exists" -ForegroundColor Green
} else {
    Write-Host "[MISS] node_modules -> jalankan npm install atau setup-windows.ps1" -ForegroundColor Red
}

Write-Host "`nJika semua prerequisite utama OK, jalankan:" -ForegroundColor White
Write-Host "  .\scripts\run-dev.ps1" -ForegroundColor Yellow
