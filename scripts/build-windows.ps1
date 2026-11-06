$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

foreach ($required in @(
    'src-tauri\runtime\windows\yt-dlp.exe',
    'src-tauri\runtime\windows\ffmpeg.exe',
    'src-tauri\runtime\windows\cpu\whisper-cli.exe',
    'src-tauri\runtime\windows\cpu\whisper.dll'
)) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) {
        throw "Runtime belum lengkap: $required. Jalankan .\scripts\setup-windows.ps1 terlebih dahulu."
    }
}

$allowedRuntimeFiles = @(
    'yt-dlp.exe',
    'ffmpeg.exe',
    'cpu\.gitkeep',
    'cpu\whisper-cli.exe',
    'cpu\whisper.dll',
    'cuda\.gitkeep',
    '.gitkeep'
)
$unexpectedRuntimeFiles = Get-ChildItem -LiteralPath 'src-tauri\runtime\windows' -Recurse -File -Force |
    ForEach-Object {
        $relative = $_.FullName.Substring((Join-Path $Root 'src-tauri\runtime\windows').Length + 1)
        if ($relative -like 'cpu\ggml*.dll') { return }
        if ($allowedRuntimeFiles -notcontains $relative) { $relative }
    }
if ($unexpectedRuntimeFiles) {
    throw "Runtime Windows berisi file yang tidak diizinkan untuk bundle: $($unexpectedRuntimeFiles -join ', '). Jalankan setup ulang untuk membuat runtime minimal."
}

$buildArgs = @('--bundles', 'nsis')
if ($env:TAURI_CREATE_UPDATER_ARTIFACTS -eq 'true') {
    if ([string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY) -and [string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY_PATH)) {
        throw 'TAURI_SIGNING_PRIVATE_KEY atau TAURI_SIGNING_PRIVATE_KEY_PATH wajib diisi saat membuat artifact updater.'
    }
    $buildArgs += @('--config', '{"bundle":{"createUpdaterArtifacts":true}}')
}

npm run tauri:build -- @buildArgs
if ($LASTEXITCODE -ne 0) { throw "Build aplikasi gagal (npm exit code $LASTEXITCODE)." }
$metadata = cargo metadata --no-deps --format-version 1 --manifest-path src-tauri/Cargo.toml
if ($LASTEXITCODE -ne 0) { throw 'Lokasi hasil build Cargo tidak dapat ditentukan.' }
$targetRoot = ($metadata | ConvertFrom-Json).target_directory
$releaseRoot = Join-Path $targetRoot 'release'
$bundleConfig = Get-Content -LiteralPath 'src-tauri/tauri.conf.json' -Raw | ConvertFrom-Json
$installer = Join-Path $releaseRoot "bundle\nsis\$($bundleConfig.productName)_$($bundleConfig.version)_x64-setup.exe"
node scripts/verify-windows-icons.mjs (Join-Path $releaseRoot 'whispertube.exe') $installer
if ($LASTEXITCODE -ne 0) { throw 'Ikon aplikasi/installer Windows gagal verifikasi.' }
Write-Host "`nBuild selesai. Cek: $(Join-Path $releaseRoot 'bundle')" -ForegroundColor Green
