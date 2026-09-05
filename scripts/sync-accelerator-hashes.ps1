param(
    [string]$Repository = "mOdrA40/WhisperTube",
    [string]$ReleaseTag = "accelerators-v0.1.0",
    [switch]$Apply
)

$ErrorActionPreference = "Stop"
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot ".." )).Path
$sourcePath = Join-Path $projectRoot "src-tauri\src\accelerators.rs"
$apiUrl = "https://api.github.com/repos/$Repository/releases/tags/$ReleaseTag"
$headers = @{ "User-Agent" = "WhisperTube-maintenance" }

try {
    $release = Invoke-RestMethod -Uri $apiUrl -Headers $headers -TimeoutSec 30
} catch {
    throw "Release '$ReleaseTag' tidak bisa dibaca dari $Repository. Pastikan repository/release public dan tag sudah selesai dibangun. Detail: $($_.Exception.Message)"
}

$assetNames = @(
    "whispertube-macos-arm64-metal.zip",
    "whispertube-macos-x64-metal.zip",
    "whispertube-linux-x64-vulkan.zip",
    "whispertube-windows-x64-vulkan.zip"
)

$hashes = @{}
foreach ($assetName in $assetNames) {
    $asset = @($release.assets) | Where-Object { $_.name -eq $assetName } | Select-Object -First 1
    $checksumName = "{0}.sha256" -f [System.IO.Path]::GetFileNameWithoutExtension($assetName)
    $sidecar = @($release.assets) | Where-Object { $_.name -eq $checksumName } | Select-Object -First 1
    if (-not $asset -or -not $sidecar) {
        throw "Asset atau sidecar checksum belum ada untuk $assetName."
    }

    $sidecarText = (Invoke-RestMethod -Uri $sidecar.browser_download_url -Headers $headers -TimeoutSec 30).ToString().Trim()
    $sidecarHash = ($sidecarText -split "\s+")[0].ToLowerInvariant()
    if ($sidecarHash -notmatch "^[0-9a-f]{64}$") {
        throw "Sidecar checksum untuk $assetName bukan SHA-256 yang valid."
    }

    $assetDigest = $null
    if ($asset.digest) {
        $assetDigest = $asset.digest -replace "^sha256:", ""
        if ($assetDigest -notmatch "^[0-9a-fA-F]{64}$") {
            throw "Digest GitHub untuk $assetName bukan SHA-256 yang valid."
        }
        if ($assetDigest.ToLowerInvariant() -ne $sidecarHash) {
            throw "Digest GitHub dan sidecar berbeda untuk $assetName."
        }
    }

    $hashes[$assetName] = $sidecarHash
}

Write-Host "Release: $Repository/$ReleaseTag" -ForegroundColor Cyan
$hashes.GetEnumerator() | Sort-Object Name | ForEach-Object {
    Write-Host ("{0}  {1}" -f $_.Value, $_.Key)
}

if (-not $Apply) {
    Write-Host ""
    Write-Host "Belum mengubah source. Jalankan ulang dengan -Apply setelah memeriksa hash." -ForegroundColor Yellow
    exit 0
}

$source = Get-Content -LiteralPath $sourcePath -Raw
$replacements = @{
    "METAL_MACOS_ARM64_SHA256" = $hashes["whispertube-macos-arm64-metal.zip"]
    "METAL_MACOS_X64_SHA256" = $hashes["whispertube-macos-x64-metal.zip"]
    "VULKAN_LINUX_X64_SHA256" = $hashes["whispertube-linux-x64-vulkan.zip"]
    "VULKAN_WINDOWS_X64_SHA256" = $hashes["whispertube-windows-x64-vulkan.zip"]
}

$sourceChanged = $false
foreach ($constantName in $replacements.Keys) {
    $hash = $replacements[$constantName]
    $escapedName = [regex]::Escape($constantName)
    $pattern = '(const\s+' + $escapedName + '\s*:\s*Option<&str>\s*=\s*)(?:None|Some\(\s*"[0-9a-fA-F]{64}"\s*\))\s*;'
    $replacement = '${1}Some("' + $hash + '");'
    if (-not [regex]::IsMatch($source, $pattern)) {
        throw "Konstanta $constantName tidak ditemukan atau formatnya tidak sesuai."
    }
    $updated = [regex]::Replace($source, $pattern, $replacement, 1)
    if ($updated -ne $source) {
        $sourceChanged = $true
    }
    $source = $updated
}

if ($sourceChanged) {
    Set-Content -LiteralPath $sourcePath -Value $source -Encoding utf8NoBOM
    Write-Host "Hash accelerator sudah ditulis ke $sourcePath" -ForegroundColor Green
} else {
    Write-Host "Hash accelerator sudah sama; source tidak perlu diubah." -ForegroundColor Green
}
Write-Host "Selanjutnya jalankan build release aplikasi dan jangan ubah asset pada release tersebut."
