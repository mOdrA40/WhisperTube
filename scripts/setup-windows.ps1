$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$Root = Split-Path -Parent $PSScriptRoot
$Runtime = Join-Path $Root "src-tauri\runtime\windows"
$CpuDir = Join-Path $Runtime "cpu"
$Temp = Join-Path $env:TEMP ("whispertube-setup-" + [guid]::NewGuid().ToString("N"))
$Staging = Join-Path $Temp "staging"
$StagingCpu = Join-Path $Staging "cpu"

$YtDlpVersion = "2026.08.19"
$YtDlpSha256 = "66674953fe251b89f4d08c5f0e35e0728679bd67ab3d7d05c0562af101dd3e7a"
$FfmpegVersion = "9.0.1"
$FfmpegSha256 = "fec81ae03971d9dd4be3ebe02e263bd2ec1d789483f931bdba5f5715e65da2e9"
$WhisperVersion = "v1.9.1"
# Matches the official whisper.cpp v1.9.1 asset and the ScoopInstaller manifest.
$WhisperCpuSha256 = "7d8be46ecd31828e1eb7a2ecdd0d6b314feafd82163038ab6092594b0a063539"

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
    $previousProgressPreference = $ProgressPreference
    try {
        $ProgressPreference = "Continue"
        Invoke-WebRequest -Uri $Url -OutFile $OutFile -UseBasicParsing
    } finally {
        $ProgressPreference = $previousProgressPreference
    }
}

function Assert-Sha256($Path, $Expected, $Label) {
    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
    if ($actual -ne $Expected.ToLowerInvariant()) {
        throw "$Label checksum tidak cocok. Expected $Expected, actual $actual. File tidak dipasang."
    }
    Write-Host "    SHA-256 OK: $Label" -ForegroundColor Green
}

function Assert-Process($Path, $Arguments, $Label) {
    & $Path @Arguments | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "$Label gagal melakukan self-check."
    }
}

Write-Host "WhisperTube Windows bootstrap" -ForegroundColor Magenta
Write-Host "Runtime diverifikasi sebelum diaktifkan." -ForegroundColor DarkGray

New-Item -ItemType Directory -Force -Path $Runtime, $Temp, $Staging, $StagingCpu | Out-Null

try {
    Write-Step "Checking development prerequisites"
    Require-Command "node" "Install Node.js 22 LTS atau lebih baru."
    Require-Command "npm" "Install Node.js/npm terlebih dahulu."
    Require-Command "rustc" "Install Rust dari https://rustup.rs/ lalu buka terminal baru."
    Require-Command "cargo" "Install Rust toolchain terlebih dahulu."

    Write-Host "    Node:  $(node --version)"
    Write-Host "    npm:   $(npm --version)"
    Write-Host "    Rust:  $(rustc --version)"

    Write-Step "Downloading and verifying yt-dlp $YtDlpVersion"
    $YtDlp = Join-Path $Runtime "yt-dlp.exe"
    $StagedYtDlp = Join-Path $Staging "yt-dlp.exe"
    Download-File "https://github.com/yt-dlp/yt-dlp/releases/download/$YtDlpVersion/yt-dlp.exe" $StagedYtDlp
    Assert-Sha256 $StagedYtDlp $YtDlpSha256 "yt-dlp"
    Assert-Process $StagedYtDlp @("--version") "yt-dlp"
    Move-Item -LiteralPath $StagedYtDlp -Destination $YtDlp -Force

    Write-Step "Downloading and verifying FFmpeg $FfmpegVersion"
    $FfmpegZip = Join-Path $Temp "ffmpeg.zip"
    $FfmpegExtract = Join-Path $Temp "ffmpeg"
    Download-File "https://www.gyan.dev/ffmpeg/builds/packages/ffmpeg-$FfmpegVersion-essentials_build.zip" $FfmpegZip
    Assert-Sha256 $FfmpegZip $FfmpegSha256 "FFmpeg archive"
    Expand-Archive -Path $FfmpegZip -DestinationPath $FfmpegExtract -Force
    $FfmpegExe = Get-ChildItem -Path $FfmpegExtract -Filter "ffmpeg.exe" -Recurse | Select-Object -First 1
    if (-not $FfmpegExe) { throw "ffmpeg.exe tidak ditemukan setelah extract." }
    $StagedFfmpeg = Join-Path $Staging "ffmpeg.exe"
    Copy-Item -LiteralPath $FfmpegExe.FullName -Destination $StagedFfmpeg -Force
    Assert-Process $StagedFfmpeg @("-version") "FFmpeg"
    Move-Item -LiteralPath $StagedFfmpeg -Destination (Join-Path $Runtime "ffmpeg.exe") -Force

    Write-Step "Downloading and verifying whisper.cpp CPU engine $WhisperVersion"
    $WhisperZip = Join-Path $Temp "whisper-cpu.zip"
    $WhisperExtract = Join-Path $Temp "whisper-cpu"
    Download-File "https://github.com/ggml-org/whisper.cpp/releases/download/$WhisperVersion/whisper-bin-x64.zip" $WhisperZip
    Assert-Sha256 $WhisperZip $WhisperCpuSha256 "whisper.cpp CPU archive"
    Expand-Archive -Path $WhisperZip -DestinationPath $WhisperExtract -Force
    $ReleaseDir = Get-ChildItem -Path $WhisperExtract -Directory -Recurse | Where-Object { $_.Name -eq "Release" } | Select-Object -First 1
    if ($ReleaseDir) {
        Copy-Item -Path (Join-Path $ReleaseDir.FullName "*") -Destination $StagingCpu -Recurse -Force
    } else {
        $WhisperCli = Get-ChildItem -Path $WhisperExtract -Filter "whisper-cli.exe" -Recurse | Select-Object -First 1
        if (-not $WhisperCli) { throw "whisper-cli.exe tidak ditemukan setelah extract." }
        Copy-Item -Path (Join-Path $WhisperCli.Directory.FullName "*") -Destination $StagingCpu -Recurse -Force
    }

    $StagedWhisperCli = Join-Path $StagingCpu "whisper-cli.exe"
    if (-not (Test-Path $StagedWhisperCli)) { throw "CPU whisper engine gagal dipasang ke staging." }
    Assert-Process $StagedWhisperCli @("--version") "whisper.cpp CPU"

    $CpuBackup = Join-Path $Temp "cpu-backup"
    $HadExistingCpu = Test-Path $CpuDir
    $CpuBackupCreated = $false
    try {
        if ($HadExistingCpu) {
            Move-Item -LiteralPath $CpuDir -Destination $CpuBackup
            $CpuBackupCreated = $true
        }
        Move-Item -LiteralPath $StagingCpu -Destination $CpuDir
    } catch {
        if ($CpuBackupCreated -and (Test-Path $CpuDir)) {
            Remove-Item -LiteralPath $CpuDir -Recurse -Force
        }
        if ($CpuBackupCreated -and (Test-Path $CpuBackup)) {
            Move-Item -LiteralPath $CpuBackup -Destination $CpuDir
        } elseif (-not $HadExistingCpu -and (Test-Path $CpuDir)) {
            Remove-Item -LiteralPath $CpuDir -Recurse -Force
        }
        throw
    }
    if (Test-Path $CpuBackup) {
        Remove-Item -LiteralPath $CpuBackup -Recurse -Force -ErrorAction SilentlyContinue
    }

    Write-Step "Installing JavaScript dependencies"
    Push-Location $Root
    try {
        npm install
    } finally {
        Pop-Location
    }

    Write-Step "Runtime self-check"
    & (Join-Path $Runtime "yt-dlp.exe") --version
    & (Join-Path $Runtime "ffmpeg.exe") -version | Select-Object -First 1
    & (Join-Path $CpuDir "whisper-cli.exe") --version

    Write-Host "`nSetup selesai." -ForegroundColor Green
    Write-Host "Berikutnya jalankan:" -ForegroundColor White
    Write-Host "    .\scripts\run-dev.ps1" -ForegroundColor Yellow
    Write-Host "`nModel Whisper diunduh dari UI aplikasi saat pertama dipilih." -ForegroundColor DarkGray
    Write-Host "Jika ada NVIDIA GPU, gunakan installer CUDA dari Settings atau .\scripts\install-cuda-engine.ps1." -ForegroundColor DarkGray
} finally {
    if (Test-Path $Temp) {
        Remove-Item -LiteralPath $Temp -Recurse -Force
    }
}
