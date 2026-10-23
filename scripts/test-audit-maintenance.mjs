import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, rmSync, copyFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve, sep } from "node:path";
import { spawnSync } from "node:child_process";
import { test } from "node:test";

const workflow = (name) => readFileSync(resolve(`.github/workflows/${name}.yml`), "utf8");
test("accelerator publication cannot become the application updater's Latest release", () => {
  const source = workflow("build-accelerator-packs");
  assert.match(source, /gh release create[^]*?--latest=false/);
  assert.match(source, /verify-runtime-loadpaths\.mjs/);
});

test("stable application publication is Latest and prereleases are excluded", () => {
  const source = workflow("build-application-bundles");
  assert.match(source, /prerelease_args=\(--latest=true\)/);
  assert.match(source, /if \[\[ "\$GITHUB_REF_NAME" == \*-\* \]\]; then\s+prerelease_args=\(--prerelease --latest=false\)/);
  assert.match(source, /gh release create[^]*?"\$\{prerelease_args\[@\]\}"/);
});

test("Windows diagnostic finds development-only CUDA without executing the binary", () => {
  const root = mkdtempSync(join(tmpdir(), "whispertube-diagnostic-"));
  try {
    mkdirSync(join(root, "scripts"));
    copyFileSync(resolve("scripts/diagnose-windows.ps1"), join(root, "scripts/diagnose-windows.ps1"));
    const dev = join(root, "src-tauri/runtime-dev/windows/cuda");
    mkdirSync(dev, { recursive: true });
    writeFileSync(join(dev, "whisper-cli.exe"), "fixture; must never execute");
    const result = spawnSync("pwsh", ["-NoProfile", "-Command", 'function Get-Command { param($Name) return $null }; & $env:WHISPERTUBE_DIAGNOSTIC_FIXTURE'], {
      env: { ...process.env, WHISPERTUBE_DIAGNOSTIC_FIXTURE: join(root, "scripts/diagnose-windows.ps1") },
      encoding: "utf8", timeout: 15_000,
    });
    assert.ifError(result.error);
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /\[OK\]\s+whisper CUDA \(development; file presence only\)/);
    assert.ok(result.stdout.includes(join(dev, "whisper-cli.exe")), result.stdout);
    assert.doesNotMatch(result.stdout, /\[MISS\]\s+whisper CUDA/);
  } finally {
    assert.ok(resolve(root).startsWith(resolve(tmpdir()) + sep));
    rmSync(root, { recursive: true, force: true });
  }
});
