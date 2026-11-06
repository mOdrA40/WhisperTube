import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { icoImages, verifyWindowsIcon } from "./verify-windows-icons.mjs";

const config = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8"));
const icon = readFileSync("src-tauri/icons/icon.ico");
const frameCount = icoImages(icon).length;
const native = resolve("src-tauri");
const compiler = [process.env.NSIS_MAKENSIS, join(process.env.LOCALAPPDATA ?? "", "tauri/NSIS/makensis.exe")]
  .find((path) => path && existsSync(path)) ?? "makensis.exe";
const quoteNsis = (path) => path.replaceAll("$", () => "$$").replaceAll('"', () => '$\\"');
// Compiler paths are literal; $0 escaping applies only to runtime strings.
const quoteNsisCompiler = (path) => path.replaceAll('"', () => '$\\"');

test("ICO and PE truncation are rejected instead of accepting incomplete branding", () => {
  assert.ok(frameCount > 1, "Windows branding must include multiple display sizes");
  for (const size of [0, 5, 20, icon.length - 1]) assert.throws(() => icoImages(icon.subarray(0, size)));
  assert.throws(() => verifyWindowsIcon(Buffer.from("not a PE"), icon));
});

for (const branded of [false, true]) {
  test(`native NSIS installer and uninstaller branding: ${branded ? "application icon" : "reject defaults"}`,
    { skip: process.platform !== "win32" }, () => {
      const root = mkdtempSync(join(realpathSync(tmpdir()), "whispertube-$0-icons-"));
      try {
        const output = join(root, "branding.exe");
        const installed = join(root, "isolated");
        mkdirSync(installed);
        const installerIcon = resolve(native, config.bundle.windows.nsis.installerIcon);
        const uninstallerIcon = resolve(native, config.bundle.windows.nsis.uninstallerIcon);
        const script = join(root, "branding.nsi");
        writeFileSync(script, `Unicode true
Name "WhisperTube branding fixture"
OutFile "${quoteNsisCompiler(output)}"
InstallDir "${quoteNsis(installed)}"
RequestExecutionLevel user
SilentInstall silent
${branded ? `Icon "${quoteNsisCompiler(installerIcon)}"\nUninstallIcon "${quoteNsisCompiler(uninstallerIcon)}"` : ""}
!define MAINBINARYNAME "branding-fixture"
Var DeleteAppDataCheckboxState
Var UpdateMode
!include "${quoteNsisCompiler(resolve(native, "windows/hooks.nsh"))}"
; Compile the real hook, but do not notify the user's Shell during tests.
Function CompilePostinstallHook
  !insertmacro NSIS_HOOK_POSTINSTALL
FunctionEnd
Section
  SetOutPath "$INSTDIR"
  WriteUninstaller "$INSTDIR\\uninstall.exe"
SectionEnd
Section "Uninstall"
SectionEnd
`);
        const build = spawnSync(compiler, ["/V2", script], { encoding: "utf8", timeout: 30_000, windowsHide: true });
        assert.ifError(build.error);
        assert.equal(build.status, 0, build.stdout + build.stderr);
        // This fixture writes only its uninstaller in a fresh temporary folder;
        // it has no registry, shortcuts, application-data or executable payload.
        const run = spawnSync(output, [], { encoding: "utf8", timeout: 15_000, windowsHide: true });
        assert.ifError(run.error);
        assert.equal(run.status, 0, run.stdout + run.stderr);
        for (const path of [output, join(installed, "uninstall.exe")]) {
          if (branded) assert.equal(verifyWindowsIcon(readFileSync(path), icon), frameCount);
          else assert.throws(() => verifyWindowsIcon(readFileSync(path), icon), /tidak cocok/);
        }
      } finally {
        assert.equal(dirname(realpathSync(root)), realpathSync(tmpdir()));
        rmSync(root, { recursive: true, force: true });
      }
    });
}
