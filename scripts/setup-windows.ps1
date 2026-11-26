$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$Root = Split-Path -Parent $PSScriptRoot
$Runtime = Join-Path $Root "src-tauri\runtime\windows"
$CpuDir = Join-Path $Runtime "cpu"
$Temp = Join-Path $env:TEMP ("whispertube-setup-" + [guid]::NewGuid().ToString("N"))
$Staging = Join-Path $Temp "staging"
$CpuTransactionId = [guid]::NewGuid().ToString("N")
# Directory activation/rollback must be renames on the runtime volume.
$StagingCpu = Join-Path $Runtime (".cpu-staging-" + $CpuTransactionId)
$CpuBackup = Join-Path $Runtime (".cpu-backup-" + $CpuTransactionId)

$YtDlpVersion = "2026.10.26"
$YtDlpSha256 = "f5389d3698ecde4ef06f4e6b1ce0294fe9ac1396228c8990b8d3416d6d177a7d"
$FfmpegVersion = "9.0.2"
$FfmpegSha256 = "60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba"
$FfmpegBinarySha256 = "3256173f3f8bffd7df12227c68adf68025edb1832273a9530688a7bb1ed8edec"
$WhisperVersion = "v1.9.5"
# The b5454 assets were built from the exact v1.9.5 commit d1be6fde11ac6e0407606b4e42fe72d34add8037.
$WhisperAssetTag = "b5454"
$WhisperCpuSha256 = "6ba69e3482d7826214f90a6a9c84ca07782aec1e1d0c6a7c30c994fd5d816ccb"

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
    Download-File "https://github.com/GyanD/codexffmpeg/releases/download/$FfmpegVersion/ffmpeg-$FfmpegVersion-essentials_build.zip" $FfmpegZip
    Assert-Sha256 $FfmpegZip $FfmpegSha256 "FFmpeg archive"
    Expand-Archive -Path $FfmpegZip -DestinationPath $FfmpegExtract -Force
    $FfmpegExe = Get-ChildItem -Path $FfmpegExtract -Filter "ffmpeg.exe" -Recurse | Select-Object -First 1
    if (-not $FfmpegExe) { throw "ffmpeg.exe tidak ditemukan setelah extract." }
    $StagedFfmpeg = Join-Path $Staging "ffmpeg.exe"
    Copy-Item -LiteralPath $FfmpegExe.FullName -Destination $StagedFfmpeg -Force
    Assert-Sha256 $StagedFfmpeg $FfmpegBinarySha256 "FFmpeg binary"
    Assert-Process $StagedFfmpeg @("-version") "FFmpeg"
    Move-Item -LiteralPath $StagedFfmpeg -Destination (Join-Path $Runtime "ffmpeg.exe") -Force

    Write-Step "Downloading and verifying whisper.cpp CPU engine $WhisperVersion"
    $WhisperZip = Join-Path $Temp "whisper-cpu.zip"
    $WhisperExtract = Join-Path $Temp "whisper-cpu"
    Download-File "https://github.com/ggml-org/whisper.cpp/releases/download/$WhisperAssetTag/whisper-bin-x64.zip" $WhisperZip
    Assert-Sha256 $WhisperZip $WhisperCpuSha256 "whisper.cpp CPU archive"
    Expand-Archive -Path $WhisperZip -DestinationPath $WhisperExtract -Force
    $ReleaseDir = Get-ChildItem -Path $WhisperExtract -Directory -Recurse | Where-Object { $_.Name -eq "Release" } | Select-Object -First 1
    if ($ReleaseDir) {
        $RuntimeFiles = @(Get-ChildItem -LiteralPath $ReleaseDir.FullName -File | Where-Object {
            $_.Name -eq "whisper-cli.exe" -or
            $_.Name -eq "whisper.dll" -or
            $_.Name -like "ggml*.dll"
        })
        if (-not ($RuntimeFiles | Where-Object { $_.Name -eq "whisper-cli.exe" })) {
            throw "whisper-cli.exe tidak ditemukan dalam folder Release whisper.cpp."
        }
        foreach ($RuntimeFile in $RuntimeFiles) {
            Copy-Item -LiteralPath $RuntimeFile.FullName -Destination $StagingCpu -Force
        }
    } else {
        $WhisperCli = Get-ChildItem -Path $WhisperExtract -Filter "whisper-cli.exe" -Recurse | Select-Object -First 1
        if (-not $WhisperCli) { throw "whisper-cli.exe tidak ditemukan setelah extract." }
        $RuntimeFiles = @(Get-ChildItem -LiteralPath $WhisperCli.Directory.FullName -File | Where-Object {
            $_.Name -eq "whisper-cli.exe" -or
            $_.Name -eq "whisper.dll" -or
            $_.Name -like "ggml*.dll"
        })
        foreach ($RuntimeFile in $RuntimeFiles) {
            Copy-Item -LiteralPath $RuntimeFile.FullName -Destination $StagingCpu -Force
        }
    }

    $StagedWhisperCli = Join-Path $StagingCpu "whisper-cli.exe"
    if (-not (Test-Path $StagedWhisperCli)) { throw "CPU whisper engine gagal dipasang ke staging." }
    Assert-Process $StagedWhisperCli @("--version") "whisper.cpp CPU"

    $HadExistingCpu = Test-Path -LiteralPath $CpuDir
    $CpuBackupCreated = $false
    try {
        if ($HadExistingCpu) {
            Rename-Item -LiteralPath $CpuDir -NewName (Split-Path -Leaf $CpuBackup)
            $CpuBackupCreated = $true
        }
        Rename-Item -LiteralPath $StagingCpu -NewName (Split-Path -Leaf $CpuDir)
    } catch {
        $activationError = $_
        if ($CpuBackupCreated) {
            try {
                Rename-Item -LiteralPath $CpuBackup -NewName (Split-Path -Leaf $CpuDir)
            } catch {
                throw "Aktivasi CPU gagal: $($activationError.Exception.Message). Pemulihan gagal: $($_.Exception.Message). Runtime lama tetap tersedia di $CpuBackup."
            }
        }
        throw
    }
    if (Test-Path -LiteralPath $CpuBackup) {
        try {
            Remove-Item -LiteralPath $CpuBackup -Recurse -Force
        } catch {
            Write-Warning "CPU aktif, tetapi backup lama belum dapat dihapus di ${CpuBackup}: $($_.Exception.Message)"
        }
    }

    Write-Step "Installing JavaScript dependencies"
    Push-Location $Root
    try {
        npm ci
        if ($LASTEXITCODE -ne 0) { throw "Instalasi dependency gagal (npm exit code $LASTEXITCODE)." }
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
    # Never remove the backup here: a failed rollback must remain recoverable.
    foreach ($cleanupPath in @($Temp, $StagingCpu)) {
        if (Test-Path -LiteralPath $cleanupPath) {
            try {
                Remove-Item -LiteralPath $cleanupPath -Recurse -Force
            } catch {
                Write-Warning "Cleanup setup belum selesai di ${cleanupPath}: $($_.Exception.Message)"
            }
        }
    }
}
