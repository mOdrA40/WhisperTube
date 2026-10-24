import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { resolve } from "node:path";

// A tracked residual is not suppressed: print it on every run. New warnings,
// changed package versions/kinds, vulnerabilities and unavailable audits fail.
export function evaluateAudit(report, policy) {
  if (!report || report.error || !report.vulnerabilities || !report.warnings ||
      typeof report.vulnerabilities.found !== "boolean" ||
      !Number.isInteger(report.vulnerabilities.count) || report.vulnerabilities.count < 0 ||
      !Array.isArray(report.vulnerabilities.list) || typeof report.warnings !== "object" ||
      Array.isArray(report.warnings) || !Array.isArray(policy?.knownRisks) ||
      policy.knownRisks.some((risk) => !risk || ["id", "package", "version", "kind", "upstream", "resolution"]
        .some((key) => typeof risk[key] !== "string" || !risk[key].trim()))) {
    throw new Error("Cargo audit did not return a complete advisory report/policy.");
  }
  const failures = [];
  const known = [];
  if (report.vulnerabilities.found || report.vulnerabilities.count > 0 || report.vulnerabilities.list.length) {
    failures.push("Cargo reports vulnerabilities; tracked informational risks cannot authorize them.");
  }
  for (const [kind, warnings] of Object.entries(report.warnings)) {
    if (!Array.isArray(warnings)) throw new Error("Invalid Cargo audit warning list.");
    for (const warning of warnings) {
      const id = warning.advisory?.id;
      const name = warning.package?.name;
      const version = warning.package?.version;
      const label = `${id ?? "unknown advisory"}: ${name ?? "unknown package"}@${version ?? "unknown version"} (${kind})`;
      const entry = policy.knownRisks.find((risk) => risk.id === id && risk.package === name &&
        risk.version === version && risk.kind === kind);
      if (entry && policy.owner && entry.upstream && entry.resolution) known.push(label);
      else failures.push(`New or changed advisory needs review: ${label}`);
    }
  }
  return { known, failures };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const policy = JSON.parse(readFileSync(new URL("./cargo-advisory-policy.json", import.meta.url), "utf8"));
    const run = spawnSync("cargo", ["audit", "--file", "src-tauri/Cargo.lock", "--json"], {
      encoding: "utf8", timeout: 120_000, maxBuffer: 8 * 1024 * 1024, windowsHide: true,
    });
    if (run.error) throw run.error;
    if (run.stderr.trim()) console.error(run.stderr.trim());
    const result = evaluateAudit(JSON.parse(run.stdout), policy);
    for (const warning of result.known) console.warn(`OPEN upstream risk: ${warning}`);
    for (const failure of result.failures) console.error(failure);
    if (run.status !== 0 || result.failures.length) process.exitCode = 1;
    else console.log(`No vulnerability matches or new warnings; ${result.known.length} upstream risks remain open (owner: ${policy.owner}).`);
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}
