import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { I18nProvider, getProgressMessage, getRecommendation, useI18n } from "./i18n";
import type { SystemStatus } from "./types";

describe("resume progress copy", () => {
  it.each(["en", "id", "zh"] as const)("requires a visible device before recommending CUDA in %s", (language) => {
    const { result } = renderHook(useI18n, { wrapper: I18nProvider });
    act(() => result.current.setLanguage(language));
    const system = {
      gpuName: "NVIDIA GPU", nvidia: true, cudaSupported: true, cudaInstallable: true,
      cudaEngine: true, recommendedModelId: "base", gpuFreeMemoryMb: 4096,
      computeDevices: [], accelerators: [], cpuThreads: 8,
    } as unknown as SystemStatus;
    expect(getRecommendation(system, result.current.t)).toBe(result.current.t("hardware.recommendation.cudaNoDevice"));
    system.computeDevices = [{ id: "cuda:0", backend: "cuda", name: "NVIDIA GPU", vendor: "nvidia",
      deviceIndex: 0, integrated: false, totalMemoryMb: 8192, freeMemoryMb: 4096 }];
    expect(getRecommendation(system, result.current.t)).toBe(result.current.t("hardware.recommendation.cudaReady", {
      gpu: "NVIDIA GPU", vram: "4.0 GB", model: result.current.t("model.fast.label"),
    }));
  });
  it.each(["en", "id", "zh"] as const)("distinguishes installed and available GPU accelerators in %s", (language) => {
    const { result } = renderHook(useI18n, { wrapper: I18nProvider });
    act(() => result.current.setLanguage(language));
    for (const backend of ["vulkan", "metal"] as const) {
      const system = {
        gpuName: "GPU", nvidia: false, recommendedModelId: "base", cpuThreads: 8,
        accelerators: [{ backend, supported: true, installed: true }],
      } as SystemStatus;
      expect(getRecommendation(system, result.current.t)).toBe(result.current.t("hardware.recommendation.gpuInstalled", { model: result.current.t("model.fast.label") }));
      system.accelerators[0].installed = false;
      expect(getRecommendation(system, result.current.t)).toBe(result.current.t("hardware.recommendation.gpu"));
    }
  });
  it.each([
    ["en", "Resuming", "completed progress", "after completion"],
    ["id", "Melanjutkan", "disimpan otomatis", "setelah selesai"],
    ["zh", "继续", "自动保存", "完成后"],
  ] as const)("explains saved progress and audio retention in %s", (language, resume, checkpoint, retention) => {
    const { result } = renderHook(useI18n, { wrapper: I18nProvider });
    act(() => result.current.setLanguage(language));
    const progress = { stage: "transcribing" as const, backend: "cpu", message: "native raw message" };
    expect(getProgressMessage({ ...progress, messageCode: "resuming_transcription" }, "auto", result.current.t)).toContain(resume);
    expect(getProgressMessage({ ...progress, messageCode: "checkpoint_progress" }, "auto", result.current.t)).toContain(checkpoint);
    expect(result.current.t("controls.keepAudio")).toContain(retention);
    expect(result.current.t("settings.jobCacheHint")).toContain("24");
  });
});
