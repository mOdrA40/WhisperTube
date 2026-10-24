import assert from "node:assert/strict";
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { test } from "node:test";

const hook = readFileSync(resolve("src-tauri/windows/hooks.nsh"), "utf8");
const applicationRoot = "$LOCALAPPDATA\\app.whispertube.local";
const ownedDirectories = ["models", "jobs", "runtime", "cookie-jars"];
const databaseFiles = ["whispertube.db", "whispertube.db-journal", "whispertube.db-wal", "whispertube.db-shm"];

test("uninstall cleanup is opt-in and never removes the complete application data root", () => {
  assert.ok(hook.includes('StrCmp $WhisperTubeRemoveUserData "1" 0 whispertube_uninstall_done'));
  assert.ok(!hook.includes(`RMDir /r "${applicationRoot}"`));
  for (const name of ownedDirectories) {
    assert.ok(hook.includes(`RMDir /r "${applicationRoot}\\${name}"`), name);
    assert.ok(hook.includes(`IfFileExists "${applicationRoot}\\${name}\\*.*" whispertube_cleanup_failed`), name);
  }
  for (const name of databaseFiles) {
    assert.ok(hook.includes(`Delete "${applicationRoot}\\${name}"`), name);
    assert.ok(hook.includes(`IfFileExists "${applicationRoot}\\${name}" whispertube_cleanup_failed`), name);
  }
});

const quoteNsis = (value) => value.replaceAll("$", () => "$$").replaceAll('"', () => '$\\"');
// Compiler paths are literal; unlike runtime strings, $0 must not be doubled.
const quoteNsisCompiler = (value) => value.replaceAll('"', () => '$\\"');
const quotePowerShell = (value) => "'" + value.replaceAll("'", "''") + "'";

function isolatedBuiltinCleanup(appData, roamingData) {
  // Inspect the template embedded in the exact installed CLI, rather than a
  // handwritten imitation that could miss a second deletion path.
  const binary = readFileSync(resolve("node_modules/@tauri-apps/cli-win32-x64-msvc/cli.win32-x64-msvc.node"));
  const template = binary.toString("utf8").replaceAll("\r", "");
  const start = template.indexOf("; Delete app data if the checkbox is selected");
  const end = template.indexOf("!ifmacrodef NSIS_HOOK_POSTUNINSTALL", start);
  assert.ok(start >= 0 && end > start, "Review the installed CLI's changed uninstall template");
  let cleanup = template.slice(start, end);
  assert.match(cleanup, /\$DeleteAppDataCheckboxState = 1/);
  assert.match(cleanup, /\$UpdateMode <> 1/);
  assert.ok(cleanup.includes('RmDir /r "$LOCALAPPDATA\\${BUNDLEID}"'));
  // Fixture execution must never touch the registry or either real app-data
  // directory. Keep the template's decision/ordering and redirect deletion.
  cleanup = cleanup.replace(/^\s*DeleteReg(?:Key|Value)[^\n]*$/gm, "; registry operation excluded from isolated fixture");
  cleanup = cleanup.replaceAll('$LOCALAPPDATA\\${BUNDLEID}', () => quoteNsis(appData))
    .replaceAll('$APPDATA\\${BUNDLEID}', () => quoteNsis(roamingData));
  assert.doesNotMatch(cleanup, /DeleteReg|\$LOCALAPPDATA|\$APPDATA|\$\{BUNDLEID\}/);
  return cleanup;
}

for (const mode of ["keep", "remove", "update", "unset", "locked-cookie", "locked-database"]) {
  test(`NSIS hook in isolated storage: ${mode}`, { skip: process.platform !== "win32" }, () => {
    const root = mkdtempSync(join(tmpdir(), "whispertube-$0-uninstall-test-"));
    try {
      const appData = join(root, "app-data");
      const roamingData = join(root, "roaming-data");
      mkdirSync(roamingData);
      writeFileSync(join(roamingData, "keep.txt"), "roaming canary");
      for (const name of [...ownedDirectories, "WebView"]) mkdirSync(join(appData, name), { recursive: true });
      const cookie = join(appData, "cookie-jars", "crashed-operation", "cookies.txt");
      mkdirSync(resolve(cookie, ".."), { recursive: true });
      writeFileSync(cookie, "internal synthetic session");
      for (const name of databaseFiles) writeFileSync(join(appData, name), "database fixture");
      writeFileSync(join(root, "external-cookies.txt"), "original export");
      writeFileSync(join(appData, "WebView", "keep.txt"), "webview");
      writeFileSync(join(appData, "config.json"), "config");
      const warning = /MessageBox MB_OK\|MB_ICONEXCLAMATION \\\r?\n\s*"[^"\r\n]+"/;
      assert.match(hook, warning);
      // Redirect only the owned storage root. Replace the warning dialog with
      // an observable canary so a silent test cannot leave a modal window open.
      const isolatedHook = hook.replaceAll(applicationRoot, () => quoteNsis(appData)).replace(warning,
        'FileOpen $0 "$EXEDIR\\cleanup-warning.txt" w\n    FileWrite $0 "cleanup failed"\n    FileClose $0');
      assert.ok(!isolatedHook.includes(applicationRoot));
      writeFileSync(join(root, "hooks.nsh"), isolatedHook);
      const executable = join(root, "cleanup.exe");
      const script = join(root, "cleanup.nsi");
      writeFileSync(script, `Unicode true
Name "WhisperTube cleanup fixture"
OutFile "${quoteNsisCompiler(executable)}"
RequestExecutionLevel user
SilentInstall silent
!include LogicLib.nsh
Var DeleteAppDataCheckboxState
Var UpdateMode
!include "${quoteNsisCompiler(join(root, "hooks.nsh"))}"
Section
  StrCpy $DeleteAppDataCheckboxState "${mode === "unset" ? "" : mode === "keep" ? "0" : "1"}"
  StrCpy $UpdateMode "${mode === "update" ? "1" : "0"}"
  !insertmacro NSIS_HOOK_PREUNINSTALL
  ${isolatedBuiltinCleanup(appData, roamingData)}
  !insertmacro NSIS_HOOK_POSTUNINSTALL
SectionEnd
`);
      const compiler = [process.env.NSIS_MAKENSIS, join(process.env.LOCALAPPDATA ?? "", "tauri", "NSIS", "makensis.exe")]
        .find((candidate) => candidate && existsSync(candidate)) ?? "makensis.exe";
      const build = spawnSync(compiler, ["/V2", script], { encoding: "utf8", timeout: 30_000 });
      assert.ifError(build.error);
      assert.equal(build.status, 0, build.stdout + build.stderr);
      const locked = mode === "locked-cookie" ? cookie : mode === "locked-database" ? join(appData, "whispertube.db") : null;
      const run = locked
        ? spawnSync("pwsh", ["-NoProfile", "-NonInteractive", "-Command", `
$ErrorActionPreference = 'Stop'
$fixtureLock = [System.IO.File]::Open(${quotePowerShell(locked)}, 'Open', 'Read', 'None')
try { $fixtureProcess = Start-Process -FilePath ${quotePowerShell(executable)} -WindowStyle Hidden -Wait -PassThru; exit $fixtureProcess.ExitCode }
finally { $fixtureLock.Dispose() }
`], { encoding: "utf8", timeout: 20_000 })
        : spawnSync(executable, [], { encoding: "utf8", timeout: 20_000, windowsHide: true });
      assert.ifError(run.error);
      assert.equal(run.status, 0, run.stdout + run.stderr);
      assert.equal(readFileSync(join(root, "external-cookies.txt"), "utf8"), "original export");
      assert.equal(readFileSync(join(appData, "WebView", "keep.txt"), "utf8"), "webview");
      assert.equal(readFileSync(join(appData, "config.json"), "utf8"), "config");
      assert.equal(readFileSync(join(roamingData, "keep.txt"), "utf8"), "roaming canary");
      const keep = ["keep", "update", "unset"].includes(mode);
      assert.equal(existsSync(cookie), keep || mode === "locked-cookie");
      assert.equal(existsSync(join(root, "cleanup-warning.txt")), locked !== null);
      for (const name of databaseFiles) assert.equal(existsSync(join(appData, name)), keep || mode === "locked-database", name);
    } finally {
      const resolved = resolve(root);
      assert.ok(resolved.startsWith(resolve(tmpdir()) + "\\"));
      rmSync(resolved, { recursive: true, force: true });
    }
  });
}
