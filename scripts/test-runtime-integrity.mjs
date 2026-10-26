import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { assertRuntimeDigest, runtimePins, verifyPinnedYtDlp } from "./runtime-integrity.mjs";

test("runtime digest accepts exact bytes and rejects old, changed, empty and absent files without execution", () => {
  const temporaryRoot = realpathSync(tmpdir());
  const root = mkdtempSync(join(temporaryRoot, "runtime-digest-"));
  try {
    const file = join(root, "downloader");
    const bytes = Buffer.from("approved executable fixture; never execute");
    const digest = createHash("sha256").update(bytes).digest("hex");
    writeFileSync(file, bytes);
    assertRuntimeDigest(file, digest);
    for (const wrong of [Buffer.from("old executable"), Buffer.concat([bytes, Buffer.from([0])]), Buffer.alloc(0)]) {
      writeFileSync(file, wrong);
      assert.throws(() => assertRuntimeDigest(file, digest));
    }
    assert.throws(() => assertRuntimeDigest(join(root, "missing"), digest));
    for (const [platform, pin] of Object.entries(runtimePins.ytDlp.platforms)) {
      writeFileSync(join(root, pin.file), "outdated local cache");
      assert.throws(() => verifyPinnedYtDlp(root, platform), /Checksum runtime tidak cocok/);
    }
    assert.throws(() => verifyPinnedYtDlp(root, "unsupported"), /tidak didukung/);
  } finally {
    assert.equal(dirname(realpathSync(root)), temporaryRoot);
    rmSync(root, { recursive: true, force: true });
  }
});

test("all setup versions, hashes and asset names match the shared packaging pins", () => {
  for (const [platform, pin] of Object.entries(runtimePins.ytDlp.platforms)) {
    const extension = platform === "windows" ? "ps1" : "sh";
    const setup = readFileSync(new URL(`./setup-${platform}.${extension}`, import.meta.url), "utf8");
    assert.ok(setup.includes(`"${runtimePins.ytDlp.version}"`));
    assert.ok(setup.includes(`"${pin.sha256}"`));
    assert.ok(setup.includes(`/${pin.asset}"`));
    assert.match(pin.sha256, /^[a-f0-9]{64}$/);
  }
  const config = JSON.parse(readFileSync(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8"));
  assert.match(config.build.beforeBuildCommand, /^node scripts\/validate-runtime\.mjs &&/);
  assert.equal(config.build.beforeBundleCommand, "node scripts/validate-runtime.mjs");
});

test("standalone Tauri bundling rejects an outdated runtime before packaging", () => {
  const temporaryRoot = realpathSync(tmpdir());
  const root = mkdtempSync(join(temporaryRoot, "tauri-bundle-integrity-"));
  try {
    const platform = process.platform === "win32" ? "windows" : process.platform === "darwin" ? "macos" : "linux";
    const native = join(root, "src-tauri");
    mkdirSync(join(root, "scripts"));
    mkdirSync(join(root, "dist"));
    mkdirSync(join(native, "src"), { recursive: true });
    const runtime = join(native, "runtime", platform);
    mkdirSync(join(runtime, "cpu"), { recursive: true });
    for (const name of ["validate-runtime.mjs", "runtime-integrity.mjs"]) {
      copyFileSync(resolve("scripts", name), join(root, "scripts", name));
    }
    copyFileSync(resolve("src-tauri/runtime-pins.json"), join(native, "runtime-pins.json"));
    const production = JSON.parse(readFileSync(resolve("src-tauri/tauri.conf.json"), "utf8"));
    writeFileSync(join(native, "tauri.conf.json"), JSON.stringify({
      productName: "IntegrityFixture", version: "0.0.0", identifier: "test.whispertube.integrity",
      build: { frontendDist: "../dist", beforeBundleCommand: production.build.beforeBundleCommand },
      bundle: { active: true, resources: ["runtime/**/*"] },
    }));
    writeFileSync(join(native, "Cargo.toml"), '[package]\nname="integrity-fixture"\nversion="0.0.0"\nedition="2021"\n');
    writeFileSync(join(native, "src/main.rs"), "fn main() {}\n");
    writeFileSync(join(root, "dist/index.html"), "fixture");
    const files = platform === "windows" ? ["yt-dlp.exe", "ffmpeg.exe", "cpu/whisper-cli.exe", "cpu/whisper.dll", "cpu/ggml.dll"]
      : platform === "macos" ? ["yt-dlp", "ffmpeg", "cpu/whisper-cli", "cpu/libggml.dylib", "metal/whisper-cli", "metal/libggml.dylib"]
      : ["yt-dlp", "ffmpeg", "cpu/whisper-cli", "cpu/libggml.so"];
    for (const file of files) {
      mkdirSync(dirname(join(runtime, file)), { recursive: true });
      writeFileSync(join(runtime, file), "outdated fixture; never execute");
    }
    const target = join(native, "target/release");
    mkdirSync(target, { recursive: true });
    writeFileSync(join(target, platform === "windows" ? "integrity-fixture.exe" : "integrity-fixture"), "fixture; never execute");
    const run = spawnSync(process.execPath, [resolve("node_modules/@tauri-apps/cli/tauri.js"), "bundle", "--bundles",
      platform === "windows" ? "nsis" : platform === "macos" ? "app" : "deb"], {
      cwd: root, encoding: "utf8", timeout: 30_000, maxBuffer: 1024 * 1024,
      env: { ...process.env, CARGO_NET_OFFLINE: "true", CARGO_TARGET_DIR: join(native, "target"), TAURI_CONFIG: "{}" }, windowsHide: true,
    });
    assert.ifError(run.error);
    assert.notEqual(run.status, 0);
    assert.match(run.stdout + run.stderr, /Checksum runtime tidak cocok/);
  } finally {
    assert.equal(dirname(realpathSync(root)), temporaryRoot);
    rmSync(root, { recursive: true, force: true });
  }
});
