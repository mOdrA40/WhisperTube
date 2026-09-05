$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$Root = Split-Path -Parent $PSScriptRoot
$Runtime = Join-Path $Root "src-tauri\runtime\windows"
$CudaDir = Join-Path $Runtime "cuda"
$Temp = Join-Path $env:TEMP ("whispertube-cuda-" + [guid]::NewGuid().ToString("N"))
$Extract = Join-Path $Temp "extract"
$Staging = Join-Path $Temp "staging"
$WhisperVersion = "v1.9.1"
$CudaBuild = "12.4.0"
$CudaSha256 = "106a2030eff8998e4ef320fe72e263a78449e9040386ee27c41ea80b001b601b"

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

    $Backup = Join-Path $Temp "cuda-backup"
    $HadExistingCuda = Test-Path $CudaDir
    $BackupCreated = $false
    try {
        if ($HadExistingCuda) {
            Move-Item -LiteralPath $CudaDir -Destination $Backup
            $BackupCreated = $true
        }
        Move-Item -LiteralPath $Staging -Destination $CudaDir
    } catch {
        if ($BackupCreated -and (Test-Path $CudaDir)) {
            Remove-Item -LiteralPath $CudaDir -Recurse -Force
        }
        if ($BackupCreated -and (Test-Path $Backup)) {
            Move-Item -LiteralPath $Backup -Destination $CudaDir
        } elseif (-not $HadExistingCuda -and (Test-Path $CudaDir)) {
            Remove-Item -LiteralPath $CudaDir -Recurse -Force
        }
        throw
    }
    if (Test-Path $Backup) {
        Remove-Item -LiteralPath $Backup -Recurse -Force -ErrorAction SilentlyContinue
    }

    Write-Host "`nCUDA engine installed. Restart WhisperTube agar status diperbarui." -ForegroundColor Green
} finally {
    if (Test-Path $Temp) {
        Remove-Item -LiteralPath $Temp -Recurse -Force
    }
}
