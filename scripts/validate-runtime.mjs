import { lstatSync, readdirSync, realpathSync, statSync } from "node:fs";
import { isAbsolute, join, relative, sep } from "node:path";

const root = join(process.cwd(), "src-tauri", "runtime", process.platform === "win32" ? "windows" : process.platform === "darwin" ? "macos" : "linux");

function filesUnder(directory, root = directory) {
  if (!lstatSafe(directory)?.isDirectory()) return [];
  const files = [];
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isSymbolicLink()) {
      const resolved = realpathSafe(path);
      if (!resolved) throw new Error(`Symbolic link runtime rusak: ${path}`);
      const relativeTarget = relative(root, resolved);
      if (isAbsolute(relativeTarget) || relativeTarget === ".." || relativeTarget.startsWith(`..${sep}`)) {
        throw new Error(`Symbolic link runtime keluar dari root: ${path} -> ${resolved}`);
      }
      if (!statSafe(resolved)?.isFile()) {
        throw new Error(`Symbolic link runtime bukan regular file: ${path} -> ${resolved}`);
      }
      files.push(path);
    } else if (entry.isDirectory()) {
      files.push(...filesUnder(path, root));
    } else if (entry.isFile()) {
      files.push(path);
    }
  }
  return files;
}

function statSafe(path) {
  try {
    return statSync(path);
  } catch {
    return null;
  }
}

function lstatSafe(path) {
  try {
    return lstatSync(path);
  } catch {
    return null;
  }
}

function realpathSafe(path) {
  try {
    return realpathSync(path);
  } catch {
    return null;
  }
}

function allowed(relativePath) {
  const value = relativePath.split(sep).join("/");
  if (value === ".gitkeep") return true;
  if (process.platform === "win32" && (value === "ffmpeg.exe" || value === "yt-dlp.exe")) return true;
  if (process.platform !== "win32" && (value === "ffmpeg" || value === "yt-dlp")) return true;
  if (process.platform === "win32") {
    return value === "cpu/.gitkeep"
      || value === "cpu/whisper-cli.exe"
      || value === "cpu/whisper.dll"
      || value === "cuda/.gitkeep"
      || /^cpu\/ggml.*\.dll$/i.test(value);
  }
  if (process.platform === "darwin") {
    return value === "cpu/.gitkeep"
      || value === "cpu/whisper-cli"
      || value === "metal/.gitkeep"
      || value === "metal/whisper-cli"
      || /^cpu\/(?:libwhisper|libggml|ggml)/.test(value)
      || /^metal\/(?:libwhisper|libggml|ggml)/.test(value);
  }
  return value === "cpu/.gitkeep"
    || value === "cpu/whisper-cli"
    || /^cpu\/(?:libwhisper|libggml|ggml)/.test(value);
}

let unexpected;
let runtimeFiles;
try {
  const rootForContainment = realpathSafe(root) ?? root;
  runtimeFiles = filesUnder(root, rootForContainment)
    .map((path) => relative(root, path).split(sep).join("/"));
  unexpected = runtimeFiles.filter((path) => !allowed(path));
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}

const requiredFiles = process.platform === "win32"
  ? ["ffmpeg.exe", "yt-dlp.exe", "cpu/whisper-cli.exe", "cpu/whisper.dll"]
  : process.platform === "darwin"
    ? ["ffmpeg", "yt-dlp", "cpu/whisper-cli", "metal/whisper-cli"]
    : ["ffmpeg", "yt-dlp", "cpu/whisper-cli"];
const missingFiles = requiredFiles.filter((path) => !runtimeFiles.includes(path));
const libraryPatterns = process.platform === "win32"
  ? [/^cpu\/ggml.*\.dll$/i]
  : process.platform === "darwin"
    ? [/^cpu\/(?:libwhisper|libggml|ggml)/, /^metal\/(?:libwhisper|libggml|ggml)/]
    : [/^cpu\/(?:libwhisper|libggml|ggml)/];
const missingLibraries = libraryPatterns
  .filter((pattern) => !runtimeFiles.some((path) => pattern.test(path)))
  .map((pattern) => pattern.toString());

if (missingFiles.length > 0 || missingLibraries.length > 0) {
  console.error(`Runtime ${process.platform} belum lengkap untuk build:`);
  for (const path of missingFiles) console.error(`- file wajib: ${path}`);
  for (const pattern of missingLibraries) console.error(`- library wajib cocok dengan: ${pattern}`);
  console.error("Jalankan setup runtime platform terlebih dahulu.");
  process.exit(1);
}

if (unexpected.length > 0) {
  console.error(`Runtime ${process.platform} berisi file yang tidak diizinkan untuk bundle:`);
  for (const path of unexpected) console.error(`- ${path}`);
  console.error("Jalankan setup runtime ulang untuk membuat bundle minimal.");
  process.exit(1);
}

console.log(`Runtime ${process.platform} bundle allowlist OK.`);
