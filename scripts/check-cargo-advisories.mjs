import { mkdtempSync, readFileSync, rmdirSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import { join, resolve } from "node:path";

export function restoreVendoredUpdaterSource(lockfile, origin) {
  const updater = /^(\[\[package\]\]\r?\nname = "tauri-plugin-updater"\r?\nversion = "([^"\r\n]+)"\r?\n)(?=dependencies =)/gm;
  const matches = [...lockfile.matchAll(updater)];
  if (matches.length !== 1 || matches[0][2] !== origin?.version || !/^[a-f0-9]{64}$/.test(origin.checksum)) {
    throw new Error("Vendored updater does not match its registry identity in the advisory policy.");
  }
  const [match] = matches;
  const newline = match[1].endsWith("\r\n") ? "\r\n" : "\n";
  return lockfile.replace(updater, `${match[1]}source = "registry+https://github.com/rust-lang/crates.io-index"${newline}checksum = "${origin.checksum}"${newline}`);
}

function auditWithRegistryIdentity(origin) {
  // RustSec does not match registry advisories against path packages.
  const lockfile = readFileSync("src-tauri/Cargo.lock", "utf8");
  const audited = restoreVendoredUpdaterSource(lockfile, origin);
  const directory = mkdtempSync(join(tmpdir(), "whispertube-cargo-audit-"));
  const path = join(directory, "Cargo.lock");
  try {
    writeFileSync(path, audited);
    return spawnSync("cargo", ["audit", "--file", path, "--json"], {
      encoding: "utf8", timeout: 120_000, maxBuffer: 8 * 1024 * 1024, windowsHide: true,
    });
  } finally {
    rmSync(path, { force: true });
    rmdirSync(directory);
  }
}

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
    const run = auditWithRegistryIdentity(policy.vendoredUpdater);
    if (run.error) throw run.error;
    if (run.stderr.trim()) console.error(run.stderr.trim());
    const result = evaluateAudit(JSON.parse(run.stdout), policy);
    if (policy.knownRisks.some((risk) => risk.lockfileOnly)) {
      const tree = spawnSync("cargo", ["tree", "--locked", "--manifest-path", "src-tauri/Cargo.toml",
        "--all-features", "--target", "all", "--prefix", "none", "--format", "{p}"], {
        encoding: "utf8", timeout: 120_000, maxBuffer: 8 * 1024 * 1024, windowsHide: true,
      });
      if (tree.error) throw tree.error;
      if (tree.status !== 0) throw new Error(`Failed to verify enabled dependencies: ${tree.stderr.trim()}`);
      const packages = new Set(tree.stdout.split(/\r?\n/).map((line) => line.split(" ")[0]));
      for (const risk of policy.knownRisks.filter((risk) => risk.lockfileOnly)) {
        if (packages.has(risk.package)) result.failures.push(`Lockfile-only dependency is enabled: ${risk.package}`);
      }
    }
    for (const warning of result.known) console.warn(`OPEN upstream risk: ${warning}`);
    for (const failure of result.failures) console.error(failure);
    if (run.status !== 0 || result.failures.length) process.exitCode = 1;
    else console.log(`No vulnerability matches or new warnings; ${result.known.length} upstream risks remain open (owner: ${policy.owner}).`);
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}
