import { createHash } from "node:crypto";
import { closeSync, openSync, readFileSync, readSync, statSync } from "node:fs";
import { join } from "node:path";

export const runtimePins = JSON.parse(readFileSync(new URL("../src-tauri/runtime-pins.json", import.meta.url), "utf8"));
const MAX_EXECUTABLE_BYTES = 128 * 1024 * 1024;

// Hash the bytes without executing an old or modified executable.
export function assertRuntimeDigest(path, expected) {
  const metadata = statSync(path);
  if (!metadata.isFile() || metadata.size === 0 || metadata.size > MAX_EXECUTABLE_BYTES) {
    throw new Error(`Runtime bukan executable reguler dengan ukuran aman: ${path}`);
  }
  const hash = createHash("sha256");
  const file = openSync(path, "r");
  try {
    const buffer = Buffer.alloc(64 * 1024);
    let total = 0;
    let count;
    while ((count = readSync(file, buffer, 0, buffer.length, null)) > 0) {
      total += count;
      if (total > MAX_EXECUTABLE_BYTES) throw new Error("Runtime melebihi batas ukuran aman.");
      hash.update(buffer.subarray(0, count));
    }
    if (total !== metadata.size || hash.digest("hex") !== expected) {
      throw new Error(`Checksum runtime tidak cocok: ${path}. Jalankan setup runtime platform ulang sebelum build.`);
    }
  } finally {
    closeSync(file);
  }
}

export function verifyPinnedYtDlp(root, platform) {
  const pin = runtimePins.ytDlp.platforms[platform];
  if (!pin) throw new Error(`Platform runtime tidak didukung: ${platform}`);
  assertRuntimeDigest(join(root, pin.file), pin.sha256);
}
