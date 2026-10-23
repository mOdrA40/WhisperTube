import assert from "node:assert/strict";
import { test } from "node:test";
import { validateLoadPaths } from "./verify-runtime-loadpaths.mjs";

const siblings = new Set(["whisper-cli", "libwhisper.so.1", "libggml.so", "libwhisper.1.dylib"]);
const elf = (path) => `0 (NEEDED) Shared library: [libwhisper.so.1]\n0 (RUNPATH) Library runpath: [${path}]`;
const macho = (path) => `cmd LC_RPATH\ncmdsize 48\npath ${path} (offset 12)`;
const dylibs = "whisper-cli:\n\t@rpath/libwhisper.1.dylib (compatibility version 1.0.0, current version 1.9.2)\n\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1.0.0)";

test("accepts portable ELF/Mach-O sibling dependencies", () => {
  validateLoadPaths("linux", "whisper-cli", elf("$ORIGIN"), "", siblings);
  validateLoadPaths("darwin", "whisper-cli", macho("@loader_path"), dylibs, siblings);
});

test("rejects the published legacy Linux and macOS build paths", () => {
  assert.throws(() => validateLoadPaths("linux", "whisper-cli", elf("/home/runner/work/_temp/whisper.cpp/build/bin:"), "", siblings), /RPATH/);
  assert.throws(() => validateLoadPaths("darwin", "whisper-cli", macho("/Users/runner/work/_temp/whisper.cpp/build/bin"), dylibs, siblings), /LC_RPATH/);
});

test("rejects missing sibling paths, missing libraries and OpenMP dependencies", () => {
  assert.throws(() => validateLoadPaths("linux", "whisper-cli", "0 (NEEDED) Shared library: [libwhisper.so.1]", "", siblings), /missing sibling-relative/);
  assert.throws(() => validateLoadPaths("linux", "whisper-cli", elf("$ORIGIN"), "", new Set()), /missing sibling library/);
  assert.throws(() => validateLoadPaths("linux", "whisper-cli", elf("$ORIGIN") + "\n0 (NEEDED) Shared library: [libgomp.so.1]", "", siblings), /OpenMP/);
  assert.throws(() => validateLoadPaths("linux", "whisper-cli", "0 (NEEDED) Shared library: [/tmp/libggml.so]", "", siblings), /DT_NEEDED/);
  assert.throws(() => validateLoadPaths("darwin", "whisper-cli", "", dylibs, siblings), /missing @loader_path/);
  assert.throws(() => validateLoadPaths("darwin", "whisper-cli", macho("@loader_path"), dylibs.replace("@rpath/", "/tmp/"), siblings), /not an included sibling/);
  assert.throws(() => validateLoadPaths("darwin", "whisper-cli", macho("@loader_path"), dylibs, new Set()), /not an included sibling/);
});
