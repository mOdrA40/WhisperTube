$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$Root = Split-Path -Parent $PSScriptRoot
$Runtime = Join-Path $Root "src-tauri\runtime\windows"
$CudaDir = Join-Path $Runtime "cuda"
$Temp = Join-Path $env:TEMP "whispertube-cuda"
$WhisperVersion = "v1.9.1"
$CudaBuild = "12.4.0"

if (-not (Get-Command nvidia-smi -ErrorAction SilentlyContinue)) {
    throw "NVIDIA driver / nvidia-smi tidak terdeteksi. CUDA engine tidak perlu dipasang pada mesin ini."
}

Write-Host "NVIDIA GPU terdeteksi:" -ForegroundColor Green
& nvidia-smi --query-gpu=name,driver_version,memory.total --format=csv,noheader

New-Item -ItemType Directory -Force -Path $CudaDir, $Temp | Out-Null
$Zip = Join-Path $Temp "whisper-cuda.zip"
$Extract = Join-Path $Temp "extract"
if (Test-Path $Extract) { Remove-Item $Extract -Recurse -Force }

$Url = "https://github.com/ggml-org/whisper.cpp/releases/download/$WhisperVersion/whisper-cublas-$CudaBuild-bin-x64.zip"
Write-Host "`nDownloading whisper.cpp CUDA package (~hundreds of MB)..." -ForegroundColor Cyan
Invoke-WebRequest -Uri $Url -OutFile $Zip -UseBasicParsing
Expand-Archive -Path $Zip -DestinationPath $Extract -Force

$ReleaseDir = Get-ChildItem -Path $Extract -Directory -Recurse | Where-Object { $_.Name -eq "Release" } | Select-Object -First 1
if ($ReleaseDir) {
    Copy-Item (Join-Path $ReleaseDir.FullName "*") $CudaDir -Recurse -Force
} else {
    $Cli = Get-ChildItem -Path $Extract -Filter "whisper-cli.exe" -Recurse | Select-Object -First 1
    if (-not $Cli) { throw "whisper-cli.exe tidak ditemukan dalam CUDA package." }
    Copy-Item (Join-Path $Cli.Directory.FullName "*") $CudaDir -Recurse -Force
}

$Exe = Join-Path $CudaDir "whisper-cli.exe"
if (-not (Test-Path $Exe)) { throw "CUDA whisper engine gagal dipasang." }

Write-Host "`nTesting CUDA engine..." -ForegroundColor Cyan
& $Exe --version
Write-Host "`nCUDA engine installed. Restart WhisperTube agar status diperbarui." -ForegroundColor Green
