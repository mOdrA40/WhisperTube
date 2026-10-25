$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$Root = Split-Path -Parent $PSScriptRoot
$Runtime = Join-Path $Root "src-tauri\runtime-dev\windows"
$CudaDir = Join-Path $Runtime "cuda"
$Temp = Join-Path $env:TEMP ("whispertube-cuda-" + [guid]::NewGuid().ToString("N"))
$Extract = Join-Path $Temp "extract"
$CudaTransactionId = [guid]::NewGuid().ToString("N")
# Keep activation and rollback on the runtime volume, even when TEMP is elsewhere.
$Staging = Join-Path $Runtime (".cuda-staging-" + $CudaTransactionId)
$Backup = Join-Path $Runtime (".cuda-backup-" + $CudaTransactionId)
$WhisperVersion = "v1.9.2"
$CudaBuild = "12.4.0"
$CudaSha256 = "443110ddaad70d4290ab2e77179e31cf712035bbc4fad56bb4519a90c917b39c"

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

function Assert-Sha256($Path, $Expected) {
    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
    if ($actual -ne $Expected.ToLowerInvariant()) {
        throw "CUDA package checksum tidak cocok. Expected $Expected, actual $actual. File tidak dipasang."
    }
    Write-Host "    SHA-256 OK" -ForegroundColor Green
}

if (-not (Get-Command nvidia-smi -ErrorAction SilentlyContinue)) {
    throw "NVIDIA driver / nvidia-smi tidak terdeteksi. CUDA engine tidak perlu dipasang pada mesin ini."
}

Write-Host "NVIDIA GPU terdeteksi:" -ForegroundColor Green
& nvidia-smi --query-gpu=name,driver_version,memory.total --format=csv,noheader

New-Item -ItemType Directory -Force -Path $Runtime, $Temp, $Extract, $Staging | Out-Null

try {
    $Zip = Join-Path $Temp "whisper-cuda.zip"
    $Url = "https://github.com/ggml-org/whisper.cpp/releases/download/$WhisperVersion/whisper-cublas-$CudaBuild-bin-x64.zip"
    Write-Host "`nDownloading whisper.cpp CUDA package (~hundreds of MB)..." -ForegroundColor Cyan
    Download-File $Url $Zip
    Assert-Sha256 $Zip $CudaSha256
    Expand-Archive -Path $Zip -DestinationPath $Extract -Force

    $ReleaseDir = Get-ChildItem -Path $Extract -Directory -Recurse | Where-Object { $_.Name -eq "Release" } | Select-Object -First 1
    if ($ReleaseDir) {
        Copy-Item -Path (Join-Path $ReleaseDir.FullName "*") -Destination $Staging -Recurse -Force
    } else {
        $Cli = Get-ChildItem -Path $Extract -Filter "whisper-cli.exe" -Recurse | Select-Object -First 1
        if (-not $Cli) { throw "whisper-cli.exe tidak ditemukan dalam CUDA package." }
        Copy-Item -Path (Join-Path $Cli.Directory.FullName "*") -Destination $Staging -Recurse -Force
    }

    $StagedCli = Join-Path $Staging "whisper-cli.exe"
    if (-not (Test-Path $StagedCli)) { throw "CUDA whisper engine gagal dipasang ke staging." }
    & $StagedCli --version | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "CUDA engine gagal melakukan self-check." }

    $HadExistingCuda = Test-Path -LiteralPath $CudaDir
    $BackupCreated = $false
    try {
        if ($HadExistingCuda) {
            Rename-Item -LiteralPath $CudaDir -NewName (Split-Path -Leaf $Backup)
            $BackupCreated = $true
        }
        Rename-Item -LiteralPath $Staging -NewName (Split-Path -Leaf $CudaDir)
    } catch {
        $activationError = $_
        if ($BackupCreated) {
            try {
                Rename-Item -LiteralPath $Backup -NewName (Split-Path -Leaf $CudaDir)
            } catch {
                throw "Aktivasi CUDA gagal: $($activationError.Exception.Message). Pemulihan gagal: $($_.Exception.Message). Runtime lama tetap tersedia di $Backup."
            }
        }
        throw
    }
    if (Test-Path -LiteralPath $Backup) {
        try {
            Remove-Item -LiteralPath $Backup -Recurse -Force
        } catch {
            Write-Warning "CUDA aktif, tetapi backup lama belum dapat dihapus di ${Backup}: $($_.Exception.Message)"
        }
    }

    Write-Host "`nCUDA engine installed. Restart WhisperTube agar status diperbarui." -ForegroundColor Green
} finally {
    # A backup left by a failed rollback is intentionally preserved.
    foreach ($cleanupPath in @($Temp, $Staging)) {
        if (Test-Path -LiteralPath $cleanupPath) {
            try {
                Remove-Item -LiteralPath $cleanupPath -Recurse -Force
            } catch {
                Write-Warning "Cleanup CUDA belum selesai di ${cleanupPath}: $($_.Exception.Message)"
            }
        }
    }
}
