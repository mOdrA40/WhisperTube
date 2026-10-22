import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve, sep } from "node:path";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

for (const platform of ["windows", "linux", "macos"]) {
  test(`distribution CMake policy: ${platform}`, () => {
    const root = mkdtempSync(join(tmpdir(), "whispertube-cmake-test-"));
    try {
      const recipe = fileURLToPath(new URL("./whisper-distribution.cmake", import.meta.url)).replaceAll("\\", "/");
      const script = join(root, "check.cmake");
      const rpath = platform === "macos" ? "@loader_path" : "$ORIGIN";
      writeFileSync(script, `
set(CMAKE_HOST_APPLE ${platform === "macos" ? "TRUE" : "FALSE"})
set(CMAKE_HOST_UNIX ${platform !== "windows" ? "TRUE" : "FALSE"})
set(GGML_NATIVE ON CACHE BOOL "poison old host-dependent cache")
set(GGML_AVX2 ON CACHE BOOL "poison old host-dependent cache")
include("${recipe}")
foreach(feature NATIVE SSE42 AVX AVX2 AVX_VNNI BMI2 FMA F16C AVX512 AVX512_VBMI AVX512_VNNI AVX512_BF16 AMX_TILE AMX_INT8 AMX_BF16 CPU_ALL_VARIANTS BACKEND_DL LLAMAFILE BLAS OPENMP)
  if(GGML_\${feature})
    message(FATAL_ERROR "Host-dependent feature remains enabled: \${feature}")
  endif()
endforeach()
if(NOT GGML_CPU_ARM_ARCH STREQUAL "armv8-a")
  message(FATAL_ERROR "ARM baseline changed")
endif()
if(NOT GGML_METAL_EMBED_LIBRARY)
  message(FATAL_ERROR "Metal shaders must not depend on the build tree")
endif()
${platform === "windows" ? "" : `if(NOT CMAKE_BUILD_WITH_INSTALL_RPATH OR NOT CMAKE_INSTALL_RPATH STREQUAL "${rpath}")
  message(FATAL_ERROR "Runtime RPATH is not sibling-relative")
endif()`}
${platform !== "macos" ? "" : `if(NOT CMAKE_BUILD_WITH_INSTALL_NAME_DIR OR NOT CMAKE_INSTALL_NAME_DIR STREQUAL "@rpath")
  message(FATAL_ERROR "Mach-O library identity still depends on the build tree")
endif()`}
`);
      const result = spawnSync("cmake", ["-P", script], { encoding: "utf8", timeout: 15_000 });
      assert.ifError(result.error);
      assert.equal(result.status, 0, result.stdout + result.stderr);
    } finally {
      assert.ok(resolve(root).startsWith(resolve(tmpdir()) + sep));
      rmSync(root, { recursive: true, force: true });
    }
  });
}
