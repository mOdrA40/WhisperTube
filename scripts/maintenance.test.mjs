// @vitest-environment node
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, copyFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { beforeAll, test } from "vitest";
import { checkTestPrerequisites } from "./test-prerequisites.mjs";

// Also validate when Vitest is invoked directly instead of through npm.
beforeAll(() => checkTestPrerequisites());

test("missing PowerShell gives an actionable prerequisite error instead of silently skipping tests", () => {
  assert.throws(() => checkTestPrerequisites("win32", () => ({
    error: Object.assign(new Error("spawnSync pwsh ENOENT"), { code: "ENOENT" }),
    status: null,
  })), /PowerShell 7\+.*winget install.*pwsh --version/);
});

test("an unusable PowerShell fails prerequisites and a supported runner passes", () => {
  for (const status of [1, null]) {
    assert.throws(() => checkTestPrerequisites("win32", () => ({ status })), /version probe failed/);
  }
  assert.doesNotThrow(() => checkTestPrerequisites("win32", () => ({ status: 0 })));
});

test("non-Windows tests do not require a PowerShell installation", () => {
  for (const platform of ["linux", "darwin"]) {
    checkTestPrerequisites(platform, () => { throw new Error("PowerShell must not be probed"); });
  }
});

test.skipIf(process.platform !== "win32")("prerequisite CLI exits unsuccessfully with installation instructions when pwsh is absent", () => {
  const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => key.toLowerCase() !== "path"));
  env.PATH = "";
  const result = spawnSync(process.execPath, [resolve("scripts/test-prerequisites.mjs")], {
    env, encoding: "utf8", timeout: 15_000,
  });
  assert.ifError(result.error);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /PowerShell 7\+.*winget install.*pwsh --version/);
  assert.match(result.stderr, /ENOENT/);
});

function fixture(run) {
  const root = mkdtempSync(join(tmpdir(), "whispertube-maintenance-"));
  try {
    mkdirSync(join(root, "scripts"));
    mkdirSync(join(root, "src-tauri", "src"), { recursive: true });
    for (const name of ["sync-accelerator-hashes.ps1", "build-windows.ps1", "run-dev.ps1"]) {
      copyFileSync(resolve("scripts", name), join(root, "scripts", name));
    }
    copyFileSync(resolve("src-tauri/src/accelerators.rs"), join(root, "src-tauri/src/accelerators.rs"));
    run(root);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

function powershell(root, source) {
  const result = spawnSync("pwsh", ["-NoProfile", "-NonInteractive", "-EncodedCommand",
    Buffer.from("$ErrorActionPreference = 'Stop'\n" + source, "utf16le").toString("base64")], {
    cwd: root, encoding: "utf8", timeout: 20_000,
  });
  assert.ifError(result.error);
  return { code: result.status, output: result.stdout + result.stderr };
}

const fakeRelease = `
$global:RequestedUrls = @()
function Invoke-RestMethod {
  param($Uri, $Headers, $TimeoutSec)
  $global:RequestedUrls += $Uri
  if ($Uri -like 'https://fixture.invalid/sidecars/*') { return ('a' * 64) }
  $assets = @()
  foreach ($name in @('macos-arm64-metal', 'macos-x64-metal', 'linux-x64-vulkan', 'windows-x64-vulkan')) {
    $base = "whispertube-$name"
    $assets += @{ name = "$base.zip"; digest = ('sha256:' + ('a' * 64)) }
    $assets += @{ name = "$base.sha256"; browser_download_url = "https://fixture.invalid/sidecars/$base" }
  }
  return @{ assets = $assets }
}
`;

test.skipIf(process.platform !== "win32")("hash synchronization derives the catalog identity and writes matching pins", () => fixture((root) => {
  const original = readFileSync(join(root, "src-tauri/src/accelerators.rs"), "utf8");
  const repository = original.match(/const RELEASE_REPOSITORY:\s*&str\s*=\s*"([^"]+)";/)[1];
  const tag = original.match(/const RELEASE_TAG:\s*&str\s*=\s*"([^"]+)";/)[1];
  const result = powershell(root, fakeRelease + `
& ./scripts/sync-accelerator-hashes.ps1 -Apply
Write-Output ("REQUESTED_API=" + $global:RequestedUrls[0])
`);
  assert.equal(result.code, 0, result.output);
  assert.ok(result.output.includes("REQUESTED_API=https://api.github.com/repos/" + repository + "/releases/tags/" + tag));
  const updated = readFileSync(join(root, "src-tauri/src/accelerators.rs"), "utf8");
  assert.equal((updated.match(/Some\("a{64}"\)/g) ?? []).length, 4);
  assert.ok(updated.includes('"' + tag + '"'));
}));

test.skipIf(process.platform !== "win32")("cross-release Apply fails before any request or write", () => fixture((root) => {
  const path = join(root, "src-tauri/src/accelerators.rs");
  const original = readFileSync(path, "utf8");
  const result = powershell(root, `
function Invoke-RestMethod { throw 'NETWORK_MUST_NOT_RUN' }
& ./scripts/sync-accelerator-hashes.ps1 -ReleaseTag 'accelerators-other' -Apply
`);
  assert.notEqual(result.code, 0);
  assert.match(result.output, /-Apply harus memakai release katalog/);
  assert.ok(!result.output.includes("NETWORK_MUST_NOT_RUN"));
  assert.equal(readFileSync(path, "utf8"), original);
}));

test.skipIf(process.platform !== "win32")("read-only synchronization never changes the catalog", () => fixture((root) => {
  const path = join(root, "src-tauri/src/accelerators.rs");
  const original = readFileSync(path, "utf8");
  const result = powershell(root, fakeRelease + "& ./scripts/sync-accelerator-hashes.ps1");
  assert.equal(result.code, 0, result.output);
  assert.equal(readFileSync(path, "utf8"), original);
}));

test.skipIf(process.platform !== "win32")("failed npm build does not print success", () => fixture((root) => {
  for (const path of ["yt-dlp.exe", "ffmpeg.exe", "cpu/whisper-cli.exe", "cpu/whisper.dll"]) {
    const file = join(root, "src-tauri/runtime/windows", path);
    mkdirSync(resolve(file, ".."), { recursive: true });
    writeFileSync(file, "");
  }
  const result = powershell(root, `
$env:TAURI_CREATE_UPDATER_ARTIFACTS = 'false'
function npm { $global:LASTEXITCODE = 17 }
& ./scripts/build-windows.ps1
`);
  assert.notEqual(result.code, 0);
  assert.match(result.output, /Build aplikasi gagal/);
  assert.ok(!result.output.includes("Build selesai."));
}));

test.skipIf(process.platform !== "win32")("failed npm ci prevents starting development", () => fixture((root) => {
  const result = powershell(root, `
function npm {
  if ($args[0] -eq 'ci') { $global:LASTEXITCODE = 19 }
  else { throw 'DEV_MUST_NOT_RUN' }
}
& ./scripts/run-dev.ps1
`);
  assert.notEqual(result.code, 0);
  assert.match(result.output, /Instalasi dependency gagal/);
  assert.ok(!result.output.includes("DEV_MUST_NOT_RUN"));
}));

test.skipIf(process.platform !== "win32")("failed development command propagates its exit code", () => fixture((root) => {
  mkdirSync(join(root, "node_modules"));
  mkdirSync(join(root, "src-tauri/runtime/windows"), { recursive: true });
  writeFileSync(join(root, "src-tauri/runtime/windows/yt-dlp.exe"), "");
  const result = powershell(root, `
function npm { $global:LASTEXITCODE = 23 }
& ./scripts/run-dev.ps1
`);
  assert.notEqual(result.code, 0);
  assert.match(result.output, /Development server gagal/);
}));
