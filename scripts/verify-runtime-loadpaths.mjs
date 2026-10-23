import { readdirSync } from "node:fs";
import { basename, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

export function validateLoadPaths(platform, name, dynamic, libraries, siblings) {
  const fail = (message) => { throw new Error(`${name}: ${message}`); };
  if (platform === "linux") {
    const needed = [...dynamic.matchAll(/\(NEEDED\).*?\[([^\]]+)\]/g)].map((match) => match[1]);
    const paths = [...dynamic.matchAll(/\((?:RUNPATH|RPATH)\).*?\[([^\]]*)\]/g)].flatMap((match) => match[1].split(":"));
    if (paths.some((path) => path !== "$ORIGIN")) fail("RPATH must contain only $ORIGIN, without build paths or empty CWD entries");
    if (needed.some((dependency) => dependency.includes("/"))) fail("absolute/relative DT_NEEDED path is not relocatable");
    if (name === "whisper-cli" && !needed.some((value) => /^libwhisper/.test(value))) fail("missing whisper dependency metadata");
    if (needed.some((dependency) => /^lib(?:gomp|omp)\./.test(dependency))) fail("external OpenMP runtime is forbidden by the distribution policy");
    for (const dependency of needed.filter((value) => /^lib(?:whisper|ggml)/.test(value))) {
      if (!paths.includes("$ORIGIN")) fail("missing sibling-relative RPATH");
      if (!siblings.has(dependency)) fail(`missing sibling library ${dependency}`);
    }
  } else if (platform === "darwin") {
    const paths = [...dynamic.matchAll(/cmd LC_RPATH\s+cmdsize \d+\s+path (.+?) \(offset/g)].map((match) => match[1]);
    if (paths.some((path) => path !== "@loader_path")) fail("LC_RPATH must use @loader_path");
    for (const line of libraries.split("\n").slice(1)) {
      const dependency = /^\s+(.+?) \(compatibility version/.exec(line)?.[1];
      if (!dependency) continue;
      if (/\/lib(?:gomp|omp)(?:\.|\/)/.test(dependency)) fail("external OpenMP runtime is forbidden by the distribution policy");
      if (dependency.startsWith("/usr/lib/") || dependency.startsWith("/System/Library/")) continue;
      const leaf = /^(?:@rpath|@loader_path)\/([^/]+)$/.exec(dependency)?.[1];
      if (!leaf || !siblings.has(leaf)) fail(`non-system dependency is not an included sibling: ${dependency}`);
      if (dependency.startsWith("@rpath/") && !paths.includes("@loader_path")) fail("missing @loader_path RPATH for @rpath dependency");
    }
    if (name === "whisper-cli" && !libraries.includes("@rpath/libwhisper") && !libraries.includes("@loader_path/libwhisper")) fail("missing whisper dependency metadata");
  } else {
    fail(`unsupported load-path inspection platform: ${platform}`);
  }
}

export function verifyRuntimeLoadPaths(directory, platform = process.platform) {
  if (platform === "win32") return; // Windows resolves DLLs beside the executable.
  const files = readdirSync(directory);
  const siblings = new Set(files);
  if (!siblings.has("whisper-cli")) throw new Error("whisper-cli is missing from the stage");
  const run = (tool, args) => {
    const output = spawnSync(tool, args, { encoding: "utf8", timeout: 15_000 });
    if (output.error) throw output.error;
    if (output.status !== 0) throw new Error(`${tool} failed for ${args.at(-1)}: ${output.stderr}`);
    return output.stdout;
  };
  for (const name of files.filter((file) => file === "whisper-cli" || /\.so(?:\.|$)|\.dylib$/.test(file))) {
    const file = join(directory, name);
    const dynamic = platform === "linux" ? run("readelf", ["-d", file]) : run("otool", ["-l", file]);
    const libraries = platform === "darwin" ? run("otool", ["-L", file]) : "";
    validateLoadPaths(platform, name, dynamic, libraries, siblings);
  }
  console.log(`Verified sibling library paths: ${basename(directory)} (${platform})`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (!process.argv[2]) throw new Error("Usage: node scripts/verify-runtime-loadpaths.mjs <stage-directory>");
  verifyRuntimeLoadPaths(resolve(process.argv[2]));
}
