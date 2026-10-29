import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve, sep } from "node:path";
import { spawnSync } from "node:child_process";

const bash = process.platform === "win32" ? "C:/Program Files/Git/bin/bash.exe" : "bash";
function run(script) {
  const root = mkdtempSync(join(tmpdir(), "whispertube-publication-"));
  try {
    const result = spawnSync(bash, ["--noprofile", "--norc", "-s"], {
      input: `set -euo pipefail\ncd "$FIXTURE_ROOT"\n${script}`,
      env: { ...process.env, FIXTURE_ROOT: root.replaceAll("\\", "/"),
        ACTIVATION_HELPER: resolve("scripts/activate-runtime.sh").replaceAll("\\", "/") },
      encoding: "utf8", timeout: 20_000,
    });
    assert.ifError(result.error);
    assert.equal(result.status, 0, result.stdout + result.stderr);
    return result.stdout;
  } finally {
    assert.ok(resolve(root).startsWith(resolve(tmpdir()) + sep));
    rmSync(root, {recursive:true, force:true});
  }
}

for (const failure of ["none", "backup", "publish", "restore"]) {
  test(`runtime publication preserves recoverable files: ${failure}`, () => {
    run(`
source "$ACTIVATION_HELPER"
mkdir current staged
printf old > current/engine
printf new > staged/engine
mv() {
  if [[ "${failure}" == backup && "$1" == current ]]; then return 1; fi
  if [[ "${failure}" == publish || "${failure}" == restore ]]; then
    if [[ "$1" == staged ]]; then return 1; fi
  fi
  if [[ "${failure}" == restore && "$1" == current.backup.* ]]; then return 1; fi
  command mv "$@"
}
if activate_runtime staged current; then
  [[ "${failure}" == none ]]
  [[ "$(cat current/engine)" == new ]]
  [[ ! -e staged ]]
else
  [[ "${failure}" != none ]]
  [[ "$(cat staged/engine)" == new ]]
  if [[ "${failure}" == restore ]]; then
    [[ "$(cat current.backup.*/engine)" == old ]]
  else
    [[ "$(cat current/engine)" == old ]]
  fi
fi
`);
  });
}

test("release manifest serves signed Debian and AppImage packages separately", () => {
  const workflow = readFileSync(".github/workflows/build-application-bundles.yml", "utf8");
  const start = workflow.indexOf("          jq -n ");
  const end = workflow.indexOf("          jq . packages/latest.json", start);
  assert.ok(start >= 0 && end > start);
  const generator = workflow.slice(start, end).replace(/^          /gm, "");
  const output = run(`
mkdir packages
windows_asset=app.exe
mac_x64_asset=app-x64.app.tar.gz
mac_arm64_asset=app-arm64.app.tar.gz
linux_asset=app.AppImage
linux_deb_asset=app.deb
for f in "$windows_asset" "$mac_x64_asset" "$mac_arm64_asset" "$linux_asset" "$linux_deb_asset"; do printf fixture-signature > "$f.sig"; done
version=0.2.1
pub_date=2026-10-29T00:00:00Z
release_base=https://example.invalid/releases/v0.2.1
${generator}
cat packages/latest.json
`);
  const { platforms } = JSON.parse(output);
  // Keep older AppImage clients compatible; current Debian clients prefer -deb.
  assert.deepEqual(platforms["linux-x86_64"], platforms["linux-x86_64-appimage"]);
  assert.ok(platforms["linux-x86_64-deb"].url.endsWith(".deb"));
  assert.ok(platforms["linux-x86_64-appimage"].url.endsWith(".AppImage"));
  assert.equal(Object.keys(platforms).length, 6);
  for (const target of Object.values(platforms)) assert.equal(target.signature, "fixture-signature");
});

for (const platform of ["linux", "macos"]) {
  test(`FFmpeg staging preserves the active binary after partial copy failure: ${platform}`, () => {
    const source = readFileSync(`scripts/setup-${platform}.sh`, "utf8");
    const start = source.indexOf('STAGED_FFMPEG="$(mktemp ');
    const end = source.indexOf('STAGED_FFMPEG=""', start);
    assert.ok(start >= 0 && end > start);
    const block = source.slice(start, end);
    run(`
mkdir runtime build build/bin
printf old > runtime/ffmpeg
printf new > build/bin/ffmpeg
export RUNTIME="$PWD/runtime" ffmpeg_prefix="$PWD/build"
cp() { printf partial > "$3"; return 1; }
export -f cp
stage_ffmpeg() {
${block}
}
export -f stage_ffmpeg
if bash -e -c stage_ffmpeg; then
  echo "Partial copy incorrectly published" >&2
  exit 1
fi
[[ "$(cat runtime/ffmpeg)" == old ]]
`);
  });
}
