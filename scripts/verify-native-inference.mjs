import { createHash } from "node:crypto";
import { createReadStream, mkdtempSync, readFileSync, realpathSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

export function validateInferenceResult(result) {
  if (!result || !Array.isArray(result.transcription) || result.transcription.length > 100) {
    throw new Error("Native inference did not produce a bounded transcript array.");
  }
  for (const segment of result.transcription) {
    if (!segment || typeof segment.text !== "string" || !Number.isFinite(segment.offsets?.from) ||
        !Number.isFinite(segment.offsets?.to) || segment.offsets.from < 0 || segment.offsets.to < segment.offsets.from) {
      throw new Error("Native inference produced an invalid transcript segment.");
    }
  }
}

async function verifyModel(path) {
  const catalog = readFileSync(new URL("../src-tauri/src/models.rs", import.meta.url), "utf8");
  const expected = /id:\s*"base"[\s\S]*?sha256:\s*"([a-f0-9]{64})"/.exec(catalog)?.[1];
  const metadata = statSync(path);
  if (!expected || !metadata.isFile() || metadata.size === 0 || metadata.size > 256 * 1024 * 1024) {
    throw new Error("Expected a pinned base model of bounded size.");
  }
  const hash = createHash("sha256");
  let size = 0;
  for await (const chunk of createReadStream(path)) {
    size += chunk.length;
    if (size > 256 * 1024 * 1024) throw new Error("Model exceeds smoke-test size limit.");
    hash.update(chunk);
  }
  if (size !== metadata.size || hash.digest("hex") !== expected) throw new Error("Smoke-test model checksum mismatch.");
}

export async function verifyNativeInference(model) {
  await verifyModel(model);
  const platform = process.platform === "win32" ? "windows" : process.platform === "darwin" ? "macos" : "linux";
  const engine = resolve("src-tauri/runtime", platform, "cpu", process.platform === "win32" ? "whisper-cli.exe" : "whisper-cli");
  const temporaryRoot = realpathSync(tmpdir());
  const scratch = mkdtempSync(join(temporaryRoot, "whispertube-inference-"));
  try {
    const wav = Buffer.alloc(44 + 32000);
    wav.write("RIFF", 0); wav.writeUInt32LE(wav.length - 8, 4); wav.write("WAVEfmt ", 8);
    wav.writeUInt32LE(16, 16); wav.writeUInt16LE(1, 20); wav.writeUInt16LE(1, 22);
    wav.writeUInt32LE(16000, 24); wav.writeUInt32LE(32000, 28);
    wav.writeUInt16LE(2, 32); wav.writeUInt16LE(16, 34); wav.write("data", 36); wav.writeUInt32LE(32000, 40);
    const input = join(scratch, "silence.wav");
    const output = join(scratch, "probe");
    writeFileSync(input, wav);
    // Explicit CPU: this check must not accidentally use a runner GPU.
    const run = spawnSync(engine, ["-m", resolve(model), "-f", input, "-ng", "-t", "1", "-nt", "-oj", "-of", output], {
      cwd: dirname(engine), encoding: "utf8", timeout: 120_000, maxBuffer: 1024 * 1024, windowsHide: true,
    });
    if (run.error || run.status !== 0) throw new Error(`Native CPU inference failed: ${run.error?.message ?? run.stderr}`);
    const json = `${output}.json`;
    if (statSync(json).size > 1024 * 1024) throw new Error("Smoke-test JSON exceeds limit.");
    validateInferenceResult(JSON.parse(readFileSync(json, "utf8")));
    console.log("Shipping CPU runtime completed a real one-second inference with the pinned base model.");
  } finally {
    if (dirname(realpathSync(scratch)) !== temporaryRoot || !basename(scratch).startsWith("whispertube-inference-")) {
      throw new Error("Refusing cleanup outside smoke-test scratch.");
    }
    rmSync(scratch, { recursive: true, force: true });
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    if (process.argv.length !== 3) throw new Error("Usage: node scripts/verify-native-inference.mjs <ggml-base.bin>");
    await verifyNativeInference(process.argv[2]);
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}
