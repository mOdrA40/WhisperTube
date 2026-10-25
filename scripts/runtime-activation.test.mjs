// @vitest-environment node
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve, sep } from "node:path";
import { spawnSync } from "node:child_process";
import { beforeAll, test } from "vitest";
import { checkTestPrerequisites } from "./test-prerequisites.mjs";

beforeAll(() => checkTestPrerequisites());

const variants = [
  { name: "CPU", file: "setup-windows.ps1", headerEnd: "$YtDlpVersion", start: "$HadExistingCpu =", end: '\n    Write-Step "Installing JavaScript dependencies"', destination: "$CpuDir", stage: "$StagingCpu", backup: "$CpuBackup" },
  { name: "CUDA", file: "install-cuda-engine.ps1", headerEnd: "$WhisperVersion", start: "$HadExistingCuda =", end: '\n    Write-Host "`nCUDA engine installed.', destination: "$CudaDir", stage: "$Staging", backup: "$Backup" },
];

for (const variant of variants) {
  test.skipIf(process.platform !== "win32").each([
    "fresh", "replace", "backup-failure", "activation-failure", "rollback-failure", "destination-conflict", "backup-cleanup-failure",
  ])(`${variant.name} runtime activation: %s`, (mode) => {
    const root = mkdtempSync(join(tmpdir(), "whispertube-activation-"));
    try {
      const source = readFileSync(resolve("scripts", variant.file), "utf8").replaceAll("\r", "");
      const headerEnd = source.indexOf(variant.headerEnd);
      const start = source.indexOf(variant.start);
      const end = source.indexOf(variant.end, start);
      const finalizerStart = source.lastIndexOf("} finally {");
      assert.ok(headerEnd > 0 && start > headerEnd && end > start && finalizerStart > end);
      // Execute the production path declarations, activation and outer cleanup.
      // Downloads/executables are excluded; only rename/remove faults are injected.
      const header = source.slice(0, headerEnd);
      const activation = source.slice(start, end);
      const finalizer = source.slice(finalizerStart + "} finally {".length).trim().slice(0, -1);
      mkdirSync(join(root, "scripts"));
      const fixture = join(root, "scripts", "activation.ps1");
      writeFileSync(fixture, header + `
$FixtureDestination = ${variant.destination}
$FixtureStage = ${variant.stage}
$FixtureBackup = ${variant.backup}
$FixtureMode = '${mode}'
if ((Split-Path -Parent $FixtureStage) -ne $Runtime -or (Split-Path -Parent $FixtureBackup) -ne $Runtime) {
    throw 'Staging and backup must be siblings on the runtime volume, outside TEMP.'
}
New-Item -ItemType Directory -Force -Path $Runtime, $Temp, $FixtureStage | Out-Null
if ($FixtureMode -ne 'fresh') {
    New-Item -ItemType Directory -Force -Path $FixtureDestination | Out-Null
    Set-Content -LiteralPath (Join-Path $FixtureDestination 'engine.dll') -Value 'old engine'
    Set-Content -LiteralPath (Join-Path $FixtureDestination 'old-only.dll') -Value 'old dependency'
}
Set-Content -LiteralPath (Join-Path $FixtureStage 'engine.dll') -Value 'new engine'
Set-Content -LiteralPath (Join-Path $FixtureStage 'new-only.dll') -Value 'new dependency'
function Rename-Item {
    param([string]$LiteralPath, [string]$NewName)
    if ((Split-Path -Parent $LiteralPath) -ne $Runtime -or $NewName -match '[/\\\\]') {
        throw 'Activation must use sibling renames, never a cross-volume move.'
    }
    if ($FixtureMode -eq 'backup-failure' -and $LiteralPath -eq $FixtureDestination) { throw 'injected backup failure' }
    if ($LiteralPath -eq $FixtureStage -and $FixtureMode -in @('activation-failure', 'rollback-failure', 'destination-conflict')) {
        if ($FixtureMode -eq 'destination-conflict') {
            New-Item -ItemType Directory -Path $FixtureDestination | Out-Null
            Set-Content -LiteralPath (Join-Path $FixtureDestination 'foreign.txt') -Value 'do not delete'
        }
        throw 'injected activation failure'
    }
    if ($FixtureMode -eq 'rollback-failure' -and $LiteralPath -eq $FixtureBackup) { throw 'injected rollback failure' }
    & (Get-Command Rename-Item -CommandType Cmdlet) -LiteralPath $LiteralPath -NewName $NewName
}
function Remove-Item {
    param([string]$LiteralPath, [switch]$Recurse, [switch]$Force)
    if ($FixtureMode -eq 'backup-cleanup-failure' -and $LiteralPath -eq $FixtureBackup) { throw 'injected cleanup failure' }
    & (Get-Command Remove-Item -CommandType Cmdlet) @PSBoundParameters
}
$failure = $null
try {
${activation}
} catch {
    $failure = $_.Exception.Message
} finally {
${finalizer}
}
function Read-FixtureFile($Directory, $Name) {
    $file = Join-Path $Directory $Name
    if (Test-Path -LiteralPath $file) { return (Get-Content -LiteralPath $file -Raw).Trim() }
    return $null
}
$result = @{
    failure = $failure
    activeEngine = Read-FixtureFile $FixtureDestination 'engine.dll'
    oldDependency = Read-FixtureFile $FixtureDestination 'old-only.dll'
    newDependency = Read-FixtureFile $FixtureDestination 'new-only.dll'
    foreignFile = Read-FixtureFile $FixtureDestination 'foreign.txt'
    backupEngine = Read-FixtureFile $FixtureBackup 'engine.dll'
    backupDependency = Read-FixtureFile $FixtureBackup 'old-only.dll'
    backupPath = $FixtureBackup
    stageRemains = Test-Path -LiteralPath $FixtureStage
    tempRemains = Test-Path -LiteralPath $Temp
}
Write-Output ('RESULT=' + ($result | ConvertTo-Json -Compress))
`);
      const run = spawnSync("pwsh", ["-NoProfile", "-NonInteractive", "-File", fixture], {
        encoding: "utf8", timeout: 15_000, windowsHide: true,
        env: { ...process.env, TEMP: join(root, "download-temp"), TMP: join(root, "download-temp") },
      });
      assert.ifError(run.error);
      assert.equal(run.status, 0, run.stdout + run.stderr);
      const line = run.stdout.split(/\r?\n/).find((line) => line.startsWith("RESULT="));
      assert.ok(line, run.stdout + run.stderr);
      const result = JSON.parse(line.slice(7));
      assert.equal(result.stageRemains, false, run.stdout + run.stderr);
      assert.equal(result.tempRemains, false);
      if (["fresh", "replace", "backup-cleanup-failure"].includes(mode)) {
        assert.equal(result.failure, null);
        assert.equal(result.activeEngine, "new engine");
        assert.equal(result.newDependency, "new dependency");
        assert.equal(result.oldDependency, null);
      } else {
        assert.ok(result.failure);
      }
      if (["backup-failure", "activation-failure"].includes(mode)) {
        assert.equal(result.activeEngine, "old engine");
        assert.equal(result.oldDependency, "old dependency");
        assert.equal(result.newDependency, null);
      }
      if (["rollback-failure", "destination-conflict", "backup-cleanup-failure"].includes(mode)) {
        assert.equal(result.backupEngine, "old engine");
        assert.equal(result.backupDependency, "old dependency");
        assert.ok((result.failure ?? run.stdout).includes(result.backupPath), "retained backup path must be reported");
      } else {
        assert.equal(result.backupEngine, null);
      }
      if (mode === "destination-conflict") {
        assert.equal(result.foreignFile, "do not delete");
        assert.equal(result.activeEngine, null);
      }
    } finally {
      assert.ok(resolve(root).startsWith(resolve(tmpdir()) + sep));
      rmSync(root, { recursive: true, force: true });
    }
  });
}
