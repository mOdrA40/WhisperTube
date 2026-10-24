import assert from "node:assert/strict";
import { test } from "node:test";
import { evaluateAudit } from "./check-cargo-advisories.mjs";

const risk = { id: "RUSTSEC-fixture", package: "inherited", version: "1.0.0", kind: "unmaintained",
  upstream: "upstream fixture", resolution: "compatible upstream upgrade" };
const policy = { owner: "maintainer", knownRisks: [risk] };
const warning = (overrides = {}) => ({ advisory: { id: risk.id }, package: { name: risk.package, version: risk.version }, ...overrides });
const report = (warnings = {}) => ({ vulnerabilities: { found: false, count: 0, list: [] }, warnings });

test("known residual remains visible and resolving it does not fail the gate", () => {
  assert.deepEqual(evaluateAudit(report({ unmaintained: [warning()] }), policy), {
    known: ["RUSTSEC-fixture: inherited@1.0.0 (unmaintained)"], failures: [],
  });
  assert.deepEqual(evaluateAudit(report(), policy), { known: [], failures: [] });
});

test("new advisory, changed version and changed kind cannot inherit an old review", () => {
  for (const warnings of [
    { unmaintained: [warning({ advisory: { id: "RUSTSEC-new" } })] },
    { unmaintained: [warning({ package: { name: risk.package, version: "1.0.1" } })] },
    { unsound: [warning()] },
  ]) assert.equal(evaluateAudit(report(warnings), policy).failures.length, 1);
});

test("a known warning can never authorize a vulnerability", () => {
  const input = report({ unmaintained: [warning()] });
  for (const value of [{ found: true }, { count: 1 }, { list: [warning()] }]) {
    assert.equal(evaluateAudit({ ...input, vulnerabilities: { ...input.vulnerabilities, ...value } }, policy).failures.length, 1);
  }
});

test("incomplete audit output fails closed and incomplete risk ownership is rejected", () => {
  for (const input of [null, {}, { error: "offline" }, { ...report(), warnings: { notice: null } },
    { ...report(), warnings: [] }, { ...report(), vulnerabilities: { list: [] } }]) {
    assert.throws(() => evaluateAudit(input, policy));
  }
  assert.equal(evaluateAudit(report({ unmaintained: [warning()] }), { ...policy, owner: "" }).failures.length, 1);
  assert.throws(() => evaluateAudit(report(), { ...policy, knownRisks: [{}] }));
});
