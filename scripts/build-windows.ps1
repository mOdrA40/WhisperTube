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
Write-Host "`nBuild selesai. Cek src-tauri\target\release\bundle\" -ForegroundColor Green
