$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$Root = Split-Path -Parent $PSScriptRoot
$Runtime = Join-Path $Root "src-tauri\runtime\windows"
$CpuDir = Join-Path $Runtime "cpu"
$Temp = Join-Path $env:TEMP "whispertube-setup"
$WhisperVersion = "v1.9.1"

function Write-Step($Text) {
    Write-Host "`n==> $Text" -ForegroundColor Cyan
}

function Require-Command($Name, $Help) {
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "$Name tidak ditemukan. $Help"
    }
}

function Download-File($Url, $OutFile) {
    Write-Host "    Downloading $Url"
    Invoke-WebRequest -Uri $Url -OutFile $OutFile -UseBasicParsing
}

Write-Host "WhisperTube Windows bootstrap" -ForegroundColor Magenta
Write-Host "Source tetap ringan; runtime binaries diunduh dari upstream release." -ForegroundColor DarkGray

Write-Step "Checking development prerequisites"
Require-Command "node" "Install Node.js 22 LTS atau lebih baru."
Require-Command "npm" "Install Node.js/npm terlebih dahulu."
Require-Command "rustc" "Install Rust dari https://rustup.rs/ lalu buka terminal baru."
Require-Command "cargo" "Install Rust toolchain terlebih dahulu."

Write-Host "    Node:  $(node --version)"
Write-Host "    npm:   $(npm --version)"
Write-Host "    Rust:  $(rustc --version)"

New-Item -ItemType Directory -Force -Path $Runtime, $CpuDir, $Temp | Out-Null

Write-Step "Downloading yt-dlp"
$YtDlp = Join-Path $Runtime "yt-dlp.exe"
Download-File "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe" $YtDlp

Write-Step "Downloading FFmpeg essentials"
$FfmpegZip = Join-Path $Temp "ffmpeg.zip"
$FfmpegExtract = Join-Path $Temp "ffmpeg"
if (Test-Path $FfmpegExtract) { Remove-Item $FfmpegExtract -Recurse -Force }
Download-File "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip" $FfmpegZip
Expand-Archive -Path $FfmpegZip -DestinationPath $FfmpegExtract -Force
$FfmpegExe = Get-ChildItem -Path $FfmpegExtract -Filter "ffmpeg.exe" -Recurse | Select-Object -First 1
if (-not $FfmpegExe) { throw "ffmpeg.exe tidak ditemukan setelah extract." }
Copy-Item $FfmpegExe.FullName (Join-Path $Runtime "ffmpeg.exe") -Force

Write-Step "Downloading whisper.cpp CPU engine $WhisperVersion"
$WhisperZip = Join-Path $Temp "whisper-cpu.zip"
$WhisperExtract = Join-Path $Temp "whisper-cpu"
if (Test-Path $WhisperExtract) { Remove-Item $WhisperExtract -Recurse -Force }
Download-File "https://github.com/ggml-org/whisper.cpp/releases/download/$WhisperVersion/whisper-bin-x64.zip" $WhisperZip
Expand-Archive -Path $WhisperZip -DestinationPath $WhisperExtract -Force
$ReleaseDir = Get-ChildItem -Path $WhisperExtract -Directory -Recurse | Where-Object { $_.Name -eq "Release" } | Select-Object -First 1
if ($ReleaseDir) {
    Copy-Item (Join-Path $ReleaseDir.FullName "*") $CpuDir -Recurse -Force
} else {
    $WhisperCli = Get-ChildItem -Path $WhisperExtract -Filter "whisper-cli.exe" -Recurse | Select-Object -First 1
    if (-not $WhisperCli) { throw "whisper-cli.exe tidak ditemukan setelah extract." }
    Copy-Item (Join-Path $WhisperCli.Directory.FullName "*") $CpuDir -Recurse -Force
}

if (-not (Test-Path (Join-Path $CpuDir "whisper-cli.exe"))) {
    throw "CPU whisper engine gagal dipasang."
}

Write-Step "Installing JavaScript dependencies"
Push-Location $Root
try {
    npm install
} finally {
    Pop-Location
}

Write-Step "Runtime self-check"
& $YtDlp --version
& (Join-Path $Runtime "ffmpeg.exe") -version | Select-Object -First 1
& (Join-Path $CpuDir "whisper-cli.exe") --version

Write-Host "`nSetup selesai." -ForegroundColor Green
Write-Host "Berikutnya jalankan:" -ForegroundColor White
Write-Host "    .\scripts\run-dev.ps1" -ForegroundColor Yellow
Write-Host "`nModel Whisper diunduh dari UI aplikasi saat pertama dipilih." -ForegroundColor DarkGray
Write-Host "Jika ada NVIDIA GPU, jalankan .\scripts\install-cuda-engine.ps1 untuk akselerasi GPU." -ForegroundColor DarkGray
