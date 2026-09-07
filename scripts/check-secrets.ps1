[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

$patterns = @(
    '(?i)(api[_-]?key|access[_-]?token|client[_-]?secret|password|private[_-]?key)\s*[:=]\s*["''][^"'']{12,}["'']',
    '(?i)-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----',
    '(?i)\b(?:gh[pousr]_[A-Za-z0-9_]{20,}|sk-[A-Za-z0-9]{20,})\b'
)

$trackedFiles = @(git ls-files)
$matches = @()
foreach ($file in $trackedFiles) {
    if (-not $file -or -not (Test-Path -LiteralPath $file -PathType Leaf)) {
        continue
    }
    if ((Get-Item -LiteralPath $file).Length -gt 5MB) {
        continue
    }
    $content = Get-Content -LiteralPath $file -Raw -ErrorAction Stop
    if ($null -eq $content) {
        $content = ''
    }
    foreach ($pattern in $patterns) {
        if ([regex]::IsMatch($content, $pattern)) {
            $matches += $file
            break
        }
    }
}

if ($matches.Count -gt 0) {
    $matches | ForEach-Object { "Potential secret in $_" }
    throw 'Potential committed secret detected. Review the file and rotate any exposed credential before merging.'
}

Write-Output 'Tracked-file secret pattern scan passed.'
