[CmdletBinding(SupportsShouldProcess = $true)]
param()

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$RuntimeRoot = Join-Path $Root "src-tauri\runtime-dev\windows"

if (-not (Test-Path -LiteralPath $RuntimeRoot -PathType Container)) {
    Write-Host "Runtime developer Windows tidak ditemukan; tidak ada yang perlu dibersihkan." -ForegroundColor DarkGray
    exit 0
}

$entries = @(Get-ChildItem -LiteralPath $RuntimeRoot -Force -Recurse | Sort-Object { $_.FullName.Length } -Descending)
$removed = 0
foreach ($entry in $entries) {
    if ($entry.Name -eq ".gitkeep") {
        continue
    }
    if ($entry.PSIsContainer -and @(Get-ChildItem -LiteralPath $entry.FullName -Force).Count -gt 0) {
        continue
    }
    if ($PSCmdlet.ShouldProcess($entry.FullName, "Hapus runtime developer Windows")) {
        Remove-Item -LiteralPath $entry.FullName -Force
        $removed++
    }
}

if ($removed -eq 0) {
    Write-Host "Runtime developer Windows sudah bersih." -ForegroundColor DarkGray
} else {
    Write-Host "Runtime developer Windows dibersihkan. Data user aplikasi tidak disentuh." -ForegroundColor Green
}
