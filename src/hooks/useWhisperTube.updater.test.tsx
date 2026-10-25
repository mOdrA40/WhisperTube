import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { I18nProvider } from "../i18n";
import { useWhisperTube } from "./useWhisperTube";
import { disposeAppUpdate } from "../services/tauri";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => undefined) }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn() }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn().mockResolvedValue(undefined) }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }));

beforeEach(async () => {
  await disposeAppUpdate();
  window.localStorage.clear();
  vi.clearAllMocks();
  vi.mocked(invoke).mockImplementation(async (command) => {
    switch (command) {
      case "system_status":
        return { ytDlp: true, ffmpeg: true, cpuEngine: true, cudaEngine: false,
          cudaSupported: false, nvidia: false, gpuName: null, gpuMemoryMb: null,
          gpuFreeMemoryMb: null, availableMemoryMb: 8192, nvidiaDriverVersion: null,
          cudaInstallable: false, cpuThreads: 8, recommendation: "CPU",
          recommendedModelId: "base", recommendedBackend: "auto", accelerators: [],
          computeDevices: [], jobStorageBytes: 0, jobStorageLimitBytes: 21474836480 } as never;
      case "list_models":
        return [{ id: "base", label: "Fast", description: "Fixture", sizeMb: 142,
          vramRequiredMb: 2048, installed: true }] as never;
      case "list_history":
        return { items: [], hasMore: false, totalCount: 0 } as never;
      case "begin_app_update":
      case "end_app_update":
        return undefined as never;
      default:
        throw new Error("Unexpected IPC command in updater regression: " + command);
    }
  });
});

afterEach(async () => {
  cleanup();
  await disposeAppUpdate();
  vi.restoreAllMocks();
});

it("clears a withdrawn update after failed download and a fresh no-update check", async () => {
  const download = vi.fn().mockRejectedValue(new Error("fixture download failed"));
  const close = vi.fn().mockResolvedValue(undefined);
  const update = { version: "0.3.0", body: "Fixture update", date: null,
    downloadAndInstall: download, close } as unknown as Update;
  // Startup discovers version 0.3.0. After the failed download, the retry's
  // real service fallback check receives null from the mocked native updater.
  vi.mocked(check).mockResolvedValueOnce(update).mockResolvedValueOnce(null);
  const { result } = renderHook(() => useWhisperTube(), { wrapper: I18nProvider });
  await waitFor(() => {
    expect(result.current.appUpdate?.version).toBe("0.3.0");
    expect(result.current.operationActive).toBe(false);
  });

  await act(async () => result.current.installAppUpdate());
  expect(download).toHaveBeenCalledOnce();
  expect(close).toHaveBeenCalledOnce();
  expect(result.current.appUpdateStatus).toBe("available");
  expect(result.current.appUpdateError).toBe("fixture download failed");

  await act(async () => result.current.installAppUpdate());
  expect(check).toHaveBeenCalledTimes(2);
  expect(check).toHaveBeenNthCalledWith(2, { timeout: 30_000 });
  // A withdrawn release must clear the stale offer without another download.
  expect(download).toHaveBeenCalledOnce();
  expect(vi.mocked(invoke).mock.calls.filter(([command]) => command === "begin_app_update")).toHaveLength(1);
  expect(vi.mocked(invoke).mock.calls.filter(([command]) => command === "end_app_update")).toHaveLength(1);
  expect(close).toHaveBeenCalledOnce();
  expect(result.current.appUpdateError).toBeNull();
  expect(result.current.appUpdateStatus).toBe("up-to-date");
  expect(result.current.appUpdate).toBeNull();
});
