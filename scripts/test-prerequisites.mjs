import { spawnSync } from "node:child_process";
import { pathToFileURL } from "node:url";

export function checkTestPrerequisites(platform = process.platform, run = spawnSync) {
  if (platform !== "win32") return;
  const result = run("pwsh", ["-NoProfile", "-NonInteractive", "-Command",
    "if ($PSVersionTable.PSVersion.Major -ge 7) { exit 0 } else { exit 1 }"], {
    encoding: "utf8", timeout: 10_000,
  });
  if (result.error || result.status !== 0) {
    throw new Error(
      "Windows regression tests require PowerShell 7+ (pwsh) on PATH. " +
      "Install it with: winget install --id Microsoft.PowerShell --source winget. " +
      "Open a new terminal, verify pwsh --version, then rerun npm run test:frontend." +
      (result.error ? ` Probe failed: ${result.error.message}` : " The pwsh version probe failed."),
    );
  }
}

if (process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url) {
  try {
    checkTestPrerequisites();
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
