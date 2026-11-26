import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, writeFileSync, rmSync, unlinkSync } from "node:fs";
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
set(CMAKE_HOST_WIN32 ${platform === "windows" ? "TRUE" : "FALSE"})
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

test("Windows Vulkan resolves every SDK input from the pinned directory", () => {
  const root = mkdtempSync(join(tmpdir(), "whispertube-vulkan-sdk-test-"));
  try {
    const pinned = join(root, "pinned SDK").replaceAll("\\", "/");
    const other = join(root, "other SDK").replaceAll("\\", "/");
    for (const sdk of [pinned, other]) {
      for (const directory of ["Bin", "Include/vulkan", "Lib"]) {
        mkdirSync(join(sdk, directory), { recursive: true });
      }
      for (const file of ["Bin/glslc.exe", "Include/vulkan/vulkan.h", "Lib/vulkan-1.lib"]) {
        writeFileSync(join(sdk, file), "");
      }
    }
    const recipe = fileURLToPath(new URL("./whisper-distribution.cmake", import.meta.url)).replaceAll("\\", "/");
    const cache = join(root, "initial.cmake");
    writeFileSync(cache, `set(CMAKE_HOST_WIN32 TRUE)\ninclude("${recipe}")\n`);
    writeFileSync(join(root, "CMakeLists.txt"), `
cmake_minimum_required(VERSION 3.24)
project(verify_vulkan_sdk NONE)
list(APPEND CMAKE_PREFIX_PATH "$ENV{VULKAN_SDK}")
find_package(Vulkan REQUIRED COMPONENTS glslc)
if(NOT Vulkan_GLSLC_EXECUTABLE STREQUAL "${pinned}/Bin/glslc.exe"
   OR NOT Vulkan_INCLUDE_DIR STREQUAL "${pinned}/Include"
   OR NOT Vulkan_LIBRARY STREQUAL "${pinned}/Lib/vulkan-1.lib")
    message(FATAL_ERROR "FindVulkan selected a different SDK")
endif()
`);
    const configure = (directory) => spawnSync("cmake", [
      "-S", root, "-B", join(root, directory),
      `-DCMAKE_PREFIX_PATH=${other}`,
      `-DVulkan_GLSLC_EXECUTABLE:FILEPATH=${other}/Bin/glslc.exe`,
      "-C", cache,
    ], {
      env: { ...process.env, VULKAN_SDK: pinned, WHISPERTUBE_WINDOWS_VULKAN_SDK: pinned },
      encoding: "utf8", timeout: 15_000,
    });
    const selected = configure("selected");
    assert.ifError(selected.error);
    assert.equal(selected.status, 0, selected.stdout + selected.stderr);

    unlinkSync(join(pinned, "Bin", "glslc.exe"));
    const incomplete = configure("incomplete");
    assert.ifError(incomplete.error);
    assert.notEqual(incomplete.status, 0);
    assert.match(incomplete.stderr, /Pinned Windows Vulkan SDK file is missing: Bin\/glslc\.exe/);
  } finally {
    assert.ok(resolve(root).startsWith(resolve(tmpdir()) + sep));
    rmSync(root, { recursive: true, force: true });
  }
});
