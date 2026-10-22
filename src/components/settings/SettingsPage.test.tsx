import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../../i18n";
import type { SystemStatus } from "../../types";
import { SettingsPage } from "./SettingsPage";

type SettingsPageProps = Parameters<typeof SettingsPage>[0];

function renderSettingsPage(overrides: Partial<SettingsPageProps> = {}) {
  const props: SettingsPageProps = {
    jobCache: null,
    onLoadJobCache: vi.fn().mockResolvedValue(undefined),
    onDiscardJobCache: vi.fn().mockResolvedValue(undefined),
    cookiesPath: "",
    system: null,
    models: [{
      id: "base",
      label: "Fast",
      description: "Lightweight",
      sizeMb: 142,
      vramRequiredMb: 2048,
      installed: true,
    }],
    modelDownloadBlockReasons: {},
    busy: false,
    resettingData: false,
    historyTotalCount: 3,
    downloadingModel: {},
    accelerators: [],
    installingCuda: false,
    cudaDownloadPercent: 0,
    installingAccelerator: null,
    acceleratorDownloadPercent: 0,
    networkSpeedBytesPerSecond: null,
    appUpdate: null,
    appUpdateStatus: "idle",
    appUpdateProgress: { downloadedBytes: 0, totalBytes: null, percent: 0 },
    appUpdateError: null,
    updateChecking: false,
    onSelectCookiesFile: vi.fn(),
    onClearCookiesFile: vi.fn(),
    onOpenUrl: vi.fn(),
    onDownloadModel: vi.fn(),
    onCancelModel: vi.fn(),
    onRemoveModel: vi.fn().mockResolvedValue(undefined),
    onResetUserData: vi.fn().mockResolvedValue(undefined),
    onRefresh: vi.fn(),
    onInstallCuda: vi.fn(),
    onCancelCuda: vi.fn(),
    onInstallAccelerator: vi.fn(),
    onCancelAccelerator: vi.fn(),
    onCheckForUpdate: vi.fn(),
    onInstallAppUpdate: vi.fn(),
    ...overrides,
  };

  return render(
    <I18nProvider>
      <SettingsPage {...props} />
    </I18nProvider>,
  );
}

describe("SettingsPage local data reset", () => {
  it("requires explicit confirmation, keeps data on cancel, and surfaces reset failures", async () => {
    const onResetUserData = vi.fn().mockResolvedValue(undefined);

    renderSettingsPage({ onResetUserData });

    fireEvent.click(screen.getByRole("button", { name: "Reset WhisperTube data" }));
    expect(screen.getByRole("dialog")).toBeTruthy();
    expect(screen.getByText("3 saved history item(s) and files in transcription jobs")).toBeTruthy();
    expect(onResetUserData).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Keep my data" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(onResetUserData).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Delete model" }));
    expect(screen.getByRole("dialog")).toBeTruthy();
    expect(screen.getByText(/Delete the Fast model\?/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Keep model" }));
    expect(screen.queryByRole("dialog")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Reset WhisperTube data" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete permanently" }));
    await waitFor(() => expect(onResetUserData).toHaveBeenCalledOnce());
    expect(screen.queryByRole("dialog")).toBeNull();

    onResetUserData.mockRejectedValueOnce(new Error("Database sedang terkunci"));
    fireEvent.click(screen.getByRole("button", { name: "Reset WhisperTube data" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete permanently" }));
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("Database sedang terkunci"));
    expect(screen.getByRole("dialog")).toBeTruthy();
  });

  it("renders complete compute-device names for long names and multiple vendors", () => {
    const system: SystemStatus = {
      ytDlp: false,
      ffmpeg: false,
      cpuEngine: true,
      cudaEngine: true,
      cudaSupported: true,
      nvidia: true,
      gpuName: "NVIDIA GeForce RTX 4050 Laptop GPU",
      gpuMemoryMb: 6144,
      gpuFreeMemoryMb: 5632,
      availableMemoryMb: 8192,
      nvidiaDriverVersion: "551.61",
      cudaInstallable: true,
      cpuThreads: 16,
      recommendation: "",
      recommendedModelId: "base",
      recommendedBackend: "auto",
      accelerators: [],
      computeDevices: [
        {
          id: "cuda:0",
          backend: "cuda",
          name: "NVIDIA GeForce RTX 4050 Laptop GPU",
          vendor: "nvidia",
          deviceIndex: 0,
          integrated: false,
          totalMemoryMb: 6144,
          freeMemoryMb: 5632,
        },
        {
          id: "vulkan:0",
          backend: "vulkan",
          name: "AMD Radeon 780M Graphics (very long vendor device name)",
          vendor: "amd",
          deviceIndex: 0,
          integrated: true,
          totalMemoryMb: null,
          freeMemoryMb: null,
        },
        {
          id: "vulkan:1",
          backend: "vulkan",
          name: "Intel Arc Graphics with an intentionally long adapter name",
          vendor: "intel",
          deviceIndex: 1,
          integrated: false,
          totalMemoryMb: null,
          freeMemoryMb: null,
        },
      ],
      jobStorageBytes: 0,
      jobStorageLimitBytes: 20 * 1024 * 1024 * 1024,
    };

    renderSettingsPage({ system });

    for (const device of system.computeDevices) {
      const label = `${device.id}: ${device.name}`;
      expect(screen.getByTitle(label).textContent).toBe(label);
    }
  });

  it("disables Vulkan installation when available RAM is below the preflight floor", () => {
    const system: SystemStatus = {
      ytDlp: true,
      ffmpeg: true,
      cpuEngine: true,
      cudaEngine: false,
      cudaSupported: false,
      nvidia: false,
      gpuName: "AMD Radeon 780M Graphics",
      gpuMemoryMb: null,
      gpuFreeMemoryMb: null,
      availableMemoryMb: 1024,
      nvidiaDriverVersion: null,
      cudaInstallable: false,
      cpuThreads: 8,
      recommendation: "",
      recommendedModelId: "base",
      recommendedBackend: "auto",
      accelerators: [{
        id: "vulkan",
        label: "Vulkan",
        backend: "vulkan",
        supported: true,
        installed: false,
        downloadable: true,
        memoryAvailableMb: 1024,
        memoryRequiredMb: 2560,
        memorySufficient: false,
        preflightChecked: true,
        preflightCompatible: false,
        description: "GPU acceleration",
      }],
      computeDevices: [],
      jobStorageBytes: 0,
      jobStorageLimitBytes: 20 * 1024 * 1024 * 1024,
    };

    renderSettingsPage({ system, accelerators: system.accelerators });

    const installButton = screen.getByRole("button", { name: "Install" });
    expect((installButton as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText(/Vulkan disabled: 1\.0 GB RAM available/)).toBeTruthy();
  });
});
