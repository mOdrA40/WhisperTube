param(
    [string]$Repository,
    [string]$ReleaseTag,
    [switch]$Apply,
    [switch]$Promote
)

$ErrorActionPreference = "Stop"
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot ".." )).Path
$sourcePath = Join-Path $projectRoot "src-tauri\src\accelerators.rs"
$source = Get-Content -LiteralPath $sourcePath -Raw
$catalogRepository = [regex]::Match($source, 'const RELEASE_REPOSITORY:\s*&str\s*=\s*"([^"]+)";').Groups[1].Value
$generationPattern = [regex]'(const PACK_GENERATION:\s*&str\s*=\s*")([^"/]+)/whisper\.cpp-v[^"]+";'
$catalog = $generationPattern.Match($source)
$catalogTag = $catalog.Groups[2].Value
if (-not $catalogRepository -or -not $catalogTag) { throw 'Identitas release katalog tidak ditemukan.' }
if (-not $Repository) { $Repository = $catalogRepository }
if (-not $ReleaseTag) { $ReleaseTag = $catalogTag }
if ($Apply -and ($Repository -cne $catalogRepository -or ($ReleaseTag -cne $catalogTag -and -not $Promote))) {
    throw "-Apply harus memakai release katalog $catalogRepository/$catalogTag. Gunakan -Promote dengan tag baru untuk memperbarui generasi pack."
}
if ($Promote -and ($Repository -cne $catalogRepository -or $ReleaseTag -cne $ReleaseTag.Trim() -or
    $ReleaseTag -cnotmatch '^accelerators-v[0-9]+\.[0-9]+\.[0-9]+(?:[-+][A-Za-z0-9.+-]+)?$' -or
    $ReleaseTag -ceq $catalogTag)) {
    throw '-Promote membutuhkan tag accelerator baru dari repository katalog.'
}

$tagRef = [uri]::EscapeDataString($ReleaseTag)
$apiRoot = "https://api.github.com/repos/$Repository"
$headers = @{ "User-Agent" = "WhisperTube-maintenance" }

try {
    $release = Invoke-RestMethod -Uri "$apiRoot/releases/tags/$tagRef" -Headers $headers -TimeoutSec 30
} catch {
    throw "Release '$ReleaseTag' tidak bisa dibaca dari $Repository. Pastikan repository/release public dan tag sudah selesai dibangun. Detail: $($_.Exception.Message)"
}

if ($Promote) {
    if ($release.draft -or $release.tag_name -cne $ReleaseTag) {
        throw 'Release accelerator publik tidak cocok dengan tag tujuan.'
    }
    $workflow = Invoke-RestMethod -Uri "$apiRoot/contents/.github/workflows/build-accelerator-packs.yml?ref=$tagRef" -Headers $headers -TimeoutSec 30
    $recipe = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($workflow.content))
    $versions = [regex]::Matches($recipe, '(?m)^[ \t]+WHISPER_VERSION:[ \t]+(v[0-9]+\.[0-9]+\.[0-9]+)[ \t]*\r?$')
    if ($versions.Count -ne 1) { throw "Versi whisper.cpp tidak ditemukan secara unik pada workflow tag $ReleaseTag." }
    $newGeneration = "$ReleaseTag/whisper.cpp-$($versions[0].Groups[1].Value)"
}

$assetsByConstant = [ordered]@{
    METAL_MACOS_ARM64_SHA256 = "whispertube-macos-arm64-metal.zip"
    METAL_MACOS_X64_SHA256 = "whispertube-macos-x64-metal.zip"
    VULKAN_LINUX_X64_SHA256 = "whispertube-linux-x64-vulkan.zip"
    VULKAN_WINDOWS_X64_SHA256 = "whispertube-windows-x64-vulkan.zip"
}

$hashes = @{}
foreach ($assetName in $assetsByConstant.Values) {
    $asset = @($release.assets) | Where-Object { $_.name -ceq $assetName } | Select-Object -First 1
    $checksumName = "{0}.sha256" -f [System.IO.Path]::GetFileNameWithoutExtension($assetName)
    $sidecar = @($release.assets) | Where-Object { $_.name -ceq $checksumName } | Select-Object -First 1
    if (-not $asset -or -not $sidecar) {
        throw "Asset atau sidecar checksum belum ada untuk $assetName."
    }

    $sidecarText = (Invoke-RestMethod -Uri $sidecar.browser_download_url -Headers $headers -TimeoutSec 30).ToString().Trim()
    $sidecarHash = ($sidecarText -split "\s+")[0].ToLowerInvariant()
    if ($sidecarHash -notmatch "^[0-9a-f]{64}$") {
        throw "Sidecar checksum untuk $assetName bukan SHA-256 yang valid."
    }

    if ($asset.digest) {
        if ($asset.digest -notmatch '^sha256:([0-9a-fA-F]{64})$') {
            throw "Digest GitHub untuk $assetName bukan SHA-256 yang valid."
        }
        if ($Matches[1].ToLowerInvariant() -ne $sidecarHash) {
            throw "Digest GitHub dan sidecar berbeda untuk $assetName."
        }
    } elseif ($Promote) {
        throw "Digest ZIP GitHub belum tersedia untuk $assetName."
    }

    $hashes[$assetName] = $sidecarHash
}

Write-Host "Release: $Repository/$ReleaseTag" -ForegroundColor Cyan
if ($Promote) { Write-Host "Generasi pack: $newGeneration" }
$hashes.GetEnumerator() | Sort-Object Name | ForEach-Object {
    Write-Host ("{0}  {1}" -f $_.Value, $_.Key)
}

if (-not $Apply) {
    Write-Host ""
    Write-Host "Belum mengubah source. Jalankan ulang dengan -Apply setelah memeriksa hash." -ForegroundColor Yellow
    exit 0
}

$updated = $source
if ($Promote) {
    if ($generationPattern.Matches($updated).Count -ne 1) {
        throw 'Deklarasi generasi pack harus ada tepat satu kali.'
    }
    $updated = $generationPattern.Replace($updated, '${1}' + $newGeneration + '";', 1)
}
foreach ($constantName in $assetsByConstant.Keys) {
    $hash = $hashes[$assetsByConstant[$constantName]]
    $pattern = [regex]('(const\s+' + [regex]::Escape($constantName) + '\s*:\s*Option<&str>\s*=\s*)(?:None|Some\(\s*"[0-9a-fA-F]{64}"\s*\))\s*;')
    if ($pattern.Matches($updated).Count -ne 1) {
        throw "Konstanta $constantName harus ada tepat satu kali dan sesuai format."
    }
    $updated = $pattern.Replace($updated, '${1}Some("' + $hash + '");', 1)
}

if ($updated -cne $source) {
    $staging = Join-Path (Split-Path -Parent $sourcePath) (".accelerators-" + [guid]::NewGuid().ToString("N") + ".tmp")
    try {
        [IO.File]::WriteAllText($staging, $updated, [Text.UTF8Encoding]::new($false))
        if ((Get-Content -LiteralPath $sourcePath -Raw) -cne $source) {
            throw 'Katalog berubah selama pengambilan hash. Ulangi sinkronisasi.'
        }
        [IO.File]::Move($staging, $sourcePath, $true)
    } finally {
        if ([IO.File]::Exists($staging)) { [IO.File]::Delete($staging) }
    }
    Write-Host "Katalog accelerator sudah ditulis ke $sourcePath" -ForegroundColor Green
} else {
    Write-Host "Katalog accelerator sudah cocok." -ForegroundColor Green
}
Write-Host "Selanjutnya jalankan build release aplikasi dan jangan ubah asset pada release tersebut."
