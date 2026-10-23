import { act, renderHook, waitFor } from "@testing-library/react";
import { type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import type { ModelInfo, SystemStatus, TranscriptResult, VideoMetadata } from "../types";
import { useWhisperTube } from "./useWhisperTube";

const services = vi.hoisted(() => ({
  getSystemStatus: vi.fn(),
  getTranscriptionResumeStage: vi.fn(),
  cancelJob: vi.fn(),
  listModels: vi.fn(),
  listHistory: vi.fn(),
  listJobCache: vi.fn(),
  discardJobCache: vi.fn(),
  inspectMedia: vi.fn(),
  startTranscription: vi.fn(),
  deleteHistory: vi.fn(),
  deleteHistoryFiles: vi.fn(),
  listHistoryFiles: vi.fn(),
  revealHistoryFile: vi.fn(),
  deleteModel: vi.fn(),
  loadHistory: vi.fn(),
  resetUserData: vi.fn(),
  checkForAppUpdate: vi.fn(),
  disposeAppUpdate: vi.fn().mockResolvedValue(undefined),
  installAppUpdate: vi.fn(),
  subscribeToProgress: vi.fn(),
  subscribeToModelDownload: vi.fn(),
  subscribeToCudaDownload: vi.fn(),
  subscribeToAcceleratorDownload: vi.fn(),
}));

vi.mock("../services/tauri", () => ({
  ...services,
  downloadModel: vi.fn(),
  exportTranscriptFile: vi.fn(),
  installAccelerator: vi.fn(),
  installCudaEngine: vi.fn(),
  pickCookiesFile: vi.fn(),
  revealAudioFile: vi.fn(),
}));

const system: SystemStatus = {
  ytDlp: true,
  ffmpeg: true,
  cpuEngine: true,
  cudaEngine: false,
  cudaSupported: false,
  nvidia: false,
  gpuName: null,
  gpuMemoryMb: null,
  gpuFreeMemoryMb: null,
  availableMemoryMb: 8192,
  nvidiaDriverVersion: null,
  cudaInstallable: false,
  cpuThreads: 8,
  recommendation: "CPU",
  recommendedModelId: "base",
  recommendedBackend: "auto",
  accelerators: [],
  computeDevices: [],
  jobStorageBytes: 0,
  jobStorageLimitBytes: 20 * 1024 * 1024 * 1024,
};

const models: ModelInfo[] = [{
  id: "base",
  label: "Fast",
  description: "Test model",
  sizeMb: 100,
  vramRequiredMb: 0,
  installed: true,
}];

const metadata: VideoMetadata = {
  id: "video-id",
  title: "Test video",
  channel: "Test channel",
  duration: 60,
  thumbnail: null,
  webpageUrl: "https://www.youtube.com/watch?v=test",
  availability: null,
  source: "YouTube",
};

const transcript: TranscriptResult = {
  historyId: 1,
  title: metadata.title,
  channel: metadata.channel,
  language: "id",
  duration: metadata.duration,
  model: "base",
  backend: "cpu",
  segments: [],
  text: "Transkripsi selesai",
  txtPath: "C:/tmp/transcript.txt",
  srtPath: "C:/tmp/transcript.srt",
  vttPath: "C:/tmp/transcript.vtt",
  audioPath: null,
};

function wrapper({ children }: { children: ReactNode }) {
  return <I18nProvider>{children}</I18nProvider>;
}

describe("useWhisperTube", () => {
  beforeEach(() => {
    window.localStorage.clear();
    vi.clearAllMocks();
    services.getSystemStatus.mockResolvedValue(system);
    services.getTranscriptionResumeStage.mockReset().mockResolvedValue(null);
    services.cancelJob.mockResolvedValue(undefined);
    services.listModels.mockResolvedValue(models);
    services.listHistory.mockResolvedValue({ items: [], hasMore: false, totalCount: 0 });
    services.listJobCache.mockResolvedValue([]);
    services.discardJobCache.mockResolvedValue(undefined);
    services.inspectMedia.mockResolvedValue(metadata);
    services.startTranscription.mockResolvedValue(transcript);
    services.loadHistory.mockResolvedValue(transcript);
    services.deleteHistory.mockResolvedValue({ deletedIds: [1], cleanupWarnings: [] });
    services.deleteHistoryFiles.mockResolvedValue({ updatedIds: [1], cleanupWarnings: [] });
    services.listHistoryFiles.mockResolvedValue([]);
    services.revealHistoryFile.mockResolvedValue(undefined);
    services.checkForAppUpdate.mockResolvedValue(null);
    const unlisten = vi.fn();
    services.subscribeToProgress.mockResolvedValue(unlisten);
    services.subscribeToModelDownload.mockResolvedValue(unlisten);
    services.subscribeToCudaDownload.mockResolvedValue(unlisten);
    services.subscribeToAcceleratorDownload.mockResolvedValue(unlisten);
  });

  it("disposes updater ownership when the application hook unmounts", async () => {
    const { result, unmount } = renderHook(() => useWhisperTube(), { wrapper });
    await waitFor(() => expect(result.current.system).toEqual(system));
    expect(services.disposeAppUpdate).not.toHaveBeenCalled();
    unmount();
    expect(services.disposeAppUpdate).toHaveBeenCalledOnce();
  });

  it.each(["RAM tidak cukup", "Job dibatalkan.", "Proses melewati batas waktu"])("offers persisted resume after transcription fails: %s", async (message) => {
    const { result } = renderHook(() => useWhisperTube(), { wrapper });
    await waitFor(() => expect(result.current.system).toEqual(system));
    act(() => result.current.setUrl(metadata.webpageUrl));
    await act(async () => result.current.inspectVideo());
    expect(result.current.resumeStage).toBeNull();
    services.getTranscriptionResumeStage.mockResolvedValue("audio");
    services.startTranscription.mockRejectedValueOnce(new Error(message));
    await act(async () => result.current.startTranscription());
    await waitFor(() => expect(result.current.resumeStage).toBe("audio"));
    expect(result.current.canStart).toBe(true);
    act(() => result.current.setLanguage("en"));
    expect(result.current.resumeStage).toBe("audio"); // Audio is reusable with different inference settings.
  });

  it("waits for a manually cancelled job to settle before offering resume", async () => {
    let rejectJob!: (error: Error) => void;
    services.startTranscription.mockReturnValueOnce(new Promise((_, reject) => { rejectJob = reject; }));
    const { result } = renderHook(() => useWhisperTube(), { wrapper });
    await waitFor(() => expect(result.current.system).toEqual(system));
    act(() => result.current.setUrl(metadata.webpageUrl));
    await act(async () => result.current.inspectVideo());
    let job!: Promise<void>;
    act(() => { job = result.current.startTranscription(); });
    services.getTranscriptionResumeStage.mockResolvedValue("audio");
    await act(async () => result.current.cancelJob());
    expect(services.cancelJob).toHaveBeenCalledOnce();
    expect(result.current.busy).toBe(true);
    expect(result.current.resumeStage).toBeNull();
    await act(async () => { rejectJob(new Error("Job dibatalkan.")); await job; });
    await waitFor(() => expect(result.current.resumeStage).toBe("audio"));
  });

  it("finds saved downloads after reopening and clears resume after cache discard", async () => {
    services.getTranscriptionResumeStage.mockResolvedValue("download");
    const first = renderHook(() => useWhisperTube(), { wrapper });
    await waitFor(() => expect(first.result.current.system).toEqual(system));
    act(() => first.result.current.setUrl(metadata.webpageUrl));
    await act(async () => first.result.current.inspectVideo());
    expect(first.result.current.resumeStage).toBe("download");
    first.unmount();
    const { result } = renderHook(() => useWhisperTube(), { wrapper });
    await waitFor(() => expect(result.current.system).toEqual(system));
    act(() => result.current.setUrl(metadata.webpageUrl));
    await act(async () => result.current.inspectVideo());
    expect(result.current.resumeStage).toBe("download");
    services.getTranscriptionResumeStage.mockResolvedValue(null);
    await act(async () => result.current.discardJobCache("unfinished"));
    await waitFor(() => expect(result.current.resumeStage).toBeNull());
  });

  it("does not promise resume after a failure that saved no reusable files", async () => {
    const { result } = renderHook(() => useWhisperTube(), { wrapper });
    await waitFor(() => expect(result.current.system).toEqual(system));
    act(() => result.current.setUrl(metadata.webpageUrl));
    await act(async () => result.current.inspectVideo());
    services.startTranscription.mockRejectedValueOnce(new Error("RAM tidak cukup"));
    await act(async () => result.current.startTranscription());
    expect(result.current.resumeStage).toBeNull();
    expect(result.current.retryAvailable).toBe(true);
    expect(result.current.canStart).toBe(true);
    await act(async () => result.current.startTranscription());
    expect(result.current.retryAvailable).toBe(false);
    expect(result.current.result).toEqual(transcript);
  });

  it("ignores a late resume lookup for a different video", async () => {
    let finishOld!: (stage: string) => void;
    services.getTranscriptionResumeStage.mockReturnValueOnce(new Promise((resolve) => { finishOld = resolve; }));
    const { result } = renderHook(() => useWhisperTube(), { wrapper });
    await waitFor(() => expect(result.current.system).toEqual(system));
    act(() => result.current.setUrl(metadata.webpageUrl));
    await act(async () => result.current.inspectVideo());
    expect(result.current.checkingResume).toBe(true);
    expect(result.current.canStart).toBe(false);
    services.inspectMedia.mockResolvedValueOnce({ ...metadata, webpageUrl: "https://www.youtube.com/watch?v=other" });
    act(() => result.current.setUrl("https://www.youtube.com/watch?v=other"));
    await act(async () => result.current.inspectVideo());
    await act(async () => finishOld("audio"));
    expect(result.current.resumeStage).toBeNull();
    expect(result.current.checkingResume).toBe(false);
  });

  it("discards only the selected unfinished job and refreshes storage without resetting models/history", async () => {
    const entry = { id: "unfinished", title: "Video", sourceUrl: "https://www.youtube.com/watch?v=test", bytes: 1024, modifiedAt: "2026-10-22T00:00:00Z" };
    services.listJobCache.mockResolvedValue([entry]);
    const { result } = renderHook(() => useWhisperTube(), { wrapper });
    await waitFor(() => expect(result.current.system).toEqual(system));
    await act(async () => result.current.loadJobCache());
    expect(result.current.jobCache).toEqual([entry]);
    await act(async () => result.current.discardJobCache(entry.id));
    expect(services.discardJobCache).toHaveBeenCalledWith(entry.id);
    expect(result.current.jobCache).toEqual([]);
    expect(services.resetUserData).not.toHaveBeenCalled();
    expect(services.deleteHistory).not.toHaveBeenCalled();
    expect(services.deleteModel).not.toHaveBeenCalled();
    expect(services.getSystemStatus).toHaveBeenCalledTimes(2);
  });

  it("does not let a late update check overwrite the installing status", async () => {
    const update = { version: "0.3.0", notes: null, date: null };
    services.checkForAppUpdate.mockResolvedValueOnce(update);
    let finishCheck!: (value: typeof update) => void;
    let finishInstall!: () => void;
    const { result } = renderHook(() => useWhisperTube(), { wrapper });
    await waitFor(() => expect(result.current.appUpdate).toEqual(update));
    services.checkForAppUpdate.mockReturnValueOnce(new Promise((resolve) => { finishCheck = resolve; }));
    services.installAppUpdate.mockReturnValueOnce(new Promise<void>((resolve) => { finishInstall = resolve; }));
    let checking!: Promise<void>;
    let installing!: Promise<void>;
    act(() => { checking = result.current.checkForAppUpdate(); });
    act(() => { installing = result.current.installAppUpdate(); });
    expect(result.current.appUpdateStatus).toBe("installing");
    await act(async () => { finishCheck(update); await checking; });
    expect(result.current.appUpdateStatus).toBe("installing");
    await act(async () => { finishInstall(); await installing; });
    expect(result.current.appUpdateStatus).toBe("up-to-date");
  });

  it("keeps a completed transcription when the following history refresh fails", async () => {
    const { result } = renderHook(() => useWhisperTube(), { wrapper });
    await waitFor(() => expect(result.current.system).toEqual(system));

    act(() => result.current.setUrl(metadata.webpageUrl));
    await act(async () => result.current.inspectVideo());
    services.listHistory.mockRejectedValueOnce(new Error("History tidak tersedia"));

    await act(async () => result.current.startTranscription());

    expect(result.current.result).toEqual(transcript);
    expect(result.current.error).toContain("operation succeeded");
    expect(result.current.error).toContain("History tidak tersedia");
  });

  it.each(["audio", "transcript"] as const)("updates an open result after deleting %s without deleting its history", async (scope) => {
    const { result } = renderHook(() => useWhisperTube(), { wrapper });
    await waitFor(() => expect(result.current.system).toEqual(system));
    act(() => result.current.setUrl(metadata.webpageUrl));
    await act(async () => result.current.inspectVideo());
    await act(async () => result.current.startTranscription());
    const updated = scope === "audio" ? { ...transcript, audioPath: null, transcriptAvailable: true }
      : { ...transcript, text: "", segments: [], txtPath: "", srtPath: "", vttPath: "", transcriptAvailable: false };
    services.loadHistory.mockResolvedValueOnce(updated);
    await act(async () => result.current.deleteHistoryFiles([1], scope));
    expect(services.deleteHistoryFiles).toHaveBeenCalledWith([1], scope);
    expect(services.deleteHistory).not.toHaveBeenCalled();
    expect(result.current.result).toEqual(updated);
  });

  it("drops stale open text after a committed file deletion even if reloading fails", async () => {
    const { result } = renderHook(() => useWhisperTube(), { wrapper });
    await waitFor(() => expect(result.current.system).toEqual(system));
    act(() => result.current.setUrl(metadata.webpageUrl));
    await act(async () => result.current.inspectVideo());
    await act(async () => result.current.startTranscription());
    services.loadHistory.mockRejectedValueOnce(new Error("Reload failed"));
    await act(async () => result.current.deleteHistoryFiles([1], "transcript"));
    expect(result.current.result).toBeNull();
    expect(result.current.error).toContain("Reload failed");
  });

  it("does not block Auto on CUDA VRAM when CUDA is not installable", async () => {
    const autoFallbackSystem: SystemStatus = {
      ...system,
      cudaEngine: true,
      cudaSupported: true,
      cudaInstallable: false,
      nvidia: true,
      gpuName: "NVIDIA GeForce RTX 2050",
      gpuMemoryMb: 4096,
      gpuFreeMemoryMb: 1024,
      accelerators: [{
        id: "vulkan",
        label: "Vulkan",
        backend: "vulkan",
        supported: true,
        installed: true,
        downloadable: true,
        memoryAvailableMb: 8192,
        memoryRequiredMb: 2560,
        memorySufficient: true,
        preflightChecked: false,
        preflightCompatible: true,
        description: "GPU acceleration",
      }],
      computeDevices: [{
        id: "vulkan:0",
        backend: "vulkan",
        name: "NVIDIA GeForce RTX 2050",
        vendor: "nvidia",
        deviceIndex: 0,
        integrated: false,
        totalMemoryMb: null,
        freeMemoryMb: null,
      }],
    };
    services.getSystemStatus.mockResolvedValueOnce(autoFallbackSystem);
    services.listModels.mockResolvedValueOnce([{ ...models[0], vramRequiredMb: 2048 }]);

    const { result } = renderHook(() => useWhisperTube(), { wrapper });
    await waitFor(() => expect(result.current.system).toEqual(autoFallbackSystem));
    act(() => result.current.setUrl(metadata.webpageUrl));
    await act(async () => result.current.inspectVideo());

    expect(result.current.canStart).toBe(true);
  });

  it("does not report a user-cancelled metadata inspection as an error", async () => {
    services.inspectMedia.mockRejectedValueOnce(new Error("Pemeriksaan metadata dibatalkan"));
    const { result } = renderHook(() => useWhisperTube(), { wrapper });
    await waitFor(() => expect(result.current.system).toEqual(system));

    act(() => result.current.setUrl(metadata.webpageUrl));
    await act(async () => result.current.inspectVideo());

    expect(result.current.error).toBeNull();
  });

  it.each([0, 1024, null])("lets Auto fall back when installable CUDA has insufficient or unknown VRAM (%s)", async (free) => {
    services.getSystemStatus.mockResolvedValue({ ...system, cudaEngine: true, cudaSupported: true,
      cudaInstallable: true, nvidia: true, gpuFreeMemoryMb: free, gpuMemoryMb: free });
    services.listModels.mockResolvedValue([{ ...models[0], vramRequiredMb: 4096 }]);
    const { result } = renderHook(() => useWhisperTube(), { wrapper });
    await waitFor(() => expect(result.current.system?.cudaInstallable).toBe(true));
    act(() => result.current.setUrl(metadata.webpageUrl));
    await act(async () => result.current.inspectVideo());
    expect(result.current.canStart).toBe(true);
    await act(async () => result.current.startTranscription());
    expect(services.startTranscription).toHaveBeenCalledOnce();
  });

  it.each([0, null])("distinguishes exhausted CUDA VRAM from unreadable memory (%s)", async (free) => {
    services.getSystemStatus.mockResolvedValue({ ...system, recommendedBackend: "cuda", cudaEngine: true,
      cudaSupported: true, cudaInstallable: true, nvidia: true, gpuFreeMemoryMb: free });
    services.listModels.mockResolvedValue([{ ...models[0], vramRequiredMb: 4096 }]);
    const { result } = renderHook(() => useWhisperTube(), { wrapper });
    await waitFor(() => expect(result.current.system?.cudaInstallable).toBe(true));
    expect(result.current.vramWarning).toContain(free === 0 ? "but only 0.0 GB is available" : "VRAM could not be read");
    expect(result.current.canStart).toBe(false);
  });

  it("keeps model deletion busy until it settles and releases busy on failure", async () => {
    let fail!: (error: Error) => void;
    services.deleteModel.mockReturnValueOnce(new Promise((_, reject) => { fail = reject; }));
    const { result } = renderHook(() => useWhisperTube(), { wrapper });
    await waitFor(() => expect(result.current.system).toEqual(system));
    let removing!: Promise<void>;
    act(() => { removing = result.current.removeModel("base"); });
    expect(result.current.operationActive).toBe(true);
    await act(async () => {
      fail(new Error("File terkunci"));
      await expect(removing).rejects.toThrow("File terkunci");
    });
    expect(result.current.operationActive).toBe(false);
    expect(result.current.error).toContain("File terkunci");
  });

  it("clears an active result after committed deletion even when file cleanup is deferred", async () => {
    services.deleteHistory.mockResolvedValueOnce({ deletedIds: [1], cleanupWarnings: ["Pembersihan tertunda"] });
    const { result } = renderHook(() => useWhisperTube(), { wrapper });
    await waitFor(() => expect(result.current.system).toEqual(system));
    act(() => result.current.setUrl(metadata.webpageUrl));
    await act(async () => result.current.inspectVideo());
    await act(async () => result.current.startTranscription());
    expect(result.current.result).toEqual(transcript);
    await act(async () => result.current.deleteHistory([1]));
    expect(result.current.result).toBeNull();
    expect(result.current.error).toContain("Pembersihan tertunda");
  });

  it("preserves an active result when deletion fails before commit", async () => {
    services.deleteHistory.mockRejectedValueOnce(new Error("Database terkunci"));
    const { result } = renderHook(() => useWhisperTube(), { wrapper });
    await waitFor(() => expect(result.current.system).toEqual(system));
    act(() => result.current.setUrl(metadata.webpageUrl));
    await act(async () => result.current.inspectVideo());
    await act(async () => result.current.startTranscription());
    await act(async () => { await expect(result.current.deleteHistory([1])).rejects.toThrow("Database terkunci"); });
    expect(result.current.result).toEqual(transcript);
    expect(result.current.error).toContain("Database terkunci");
  });

  it("refreshes history after a rejected database mutation", async () => {
    const historyItem = {
      id: 1,
      title: "Test video",
      channel: "Test channel",
      sourceUrl: metadata.webpageUrl,
      createdAt: "2026-09-08T00:00:00.000Z",
      duration: 60,
      language: "id",
      model: "base",
      backend: "cpu",
    };
    services.listHistory.mockResolvedValueOnce({ items: [historyItem], hasMore: false, totalCount: 1 });
    services.deleteHistory.mockRejectedValueOnce(new Error("Database tidak tersedia"));
    const { result } = renderHook(() => useWhisperTube(), { wrapper });
    await waitFor(() => expect(result.current.history).toEqual([historyItem]));
    services.listHistory.mockResolvedValueOnce({ items: [], hasMore: false, totalCount: 0 });

    await act(async () => {
      try {
        await result.current.deleteHistory([historyItem.id]);
      } catch {
        // The mutation error is intentional; the test verifies reconciliation still occurred.
      }
    });

    expect(result.current.history).toEqual([]);
    expect(result.current.error).toContain("Database tidak tersedia");
  });

  it("surfaces a manual refresh failure instead of leaking an unhandled rejection", async () => {
    const { result } = renderHook(() => useWhisperTube(), { wrapper });
    await waitFor(() => expect(result.current.system).toEqual(system));
    services.listHistory.mockRejectedValueOnce(new Error("History sedang dikunci"));

    await act(async () => {
      await result.current.refreshSystem();
    });

    expect(result.current.error).toContain("History sedang dikunci");
  });

  it("drops the active result after a partially successful reset", async () => {
    const { result } = renderHook(() => useWhisperTube(), { wrapper });
    await waitFor(() => expect(result.current.system).toEqual(system));
    act(() => result.current.setUrl(metadata.webpageUrl));
    await act(async () => result.current.inspectVideo());
    await act(async () => result.current.startTranscription());
    expect(result.current.result).toEqual(transcript);
    services.resetUserData.mockRejectedValueOnce({ code: "backend_error", detail: "Reset data tidak selesai sepenuhnya: runtime terkunci" });
    services.listHistory.mockResolvedValue({ items: [], hasMore: false, totalCount: 0 });
    await act(async () => {
      await expect(result.current.resetUserData()).rejects.toMatchObject({ code: "backend_error" });
    });
    expect(result.current.result).toBeNull();
    expect(result.current.historyTotalCount).toBe(0);
    expect(result.current.jobCache).toBeNull();
    expect(result.current.error).toContain("runtime terkunci");
  });

  it("explains the JS prerequisite if the runtime disappears after inspection", async () => {
    const { result } = renderHook(() => useWhisperTube(), { wrapper });
    await waitFor(() => expect(result.current.system).toEqual(system));
    act(() => result.current.setUrl(metadata.webpageUrl));
    await act(async () => result.current.inspectVideo());
    services.startTranscription.mockRejectedValueOnce({ code: "media_source_js_runtime", detail: "native detail" });
    await act(async () => result.current.startTranscription());
    expect(result.current.error).toContain("Node.js 22+");
    expect(result.current.error).toContain("Deno 2.3+");
    expect(result.current.error).not.toContain("native detail");
  });

  it("preserves the result when reset is rejected before mutation", async () => {
    const { result } = renderHook(() => useWhisperTube(), { wrapper });
    await waitFor(() => expect(result.current.system).toEqual(system));
    act(() => result.current.setUrl(metadata.webpageUrl));
    await act(async () => result.current.inspectVideo());
    await act(async () => result.current.startTranscription());
    services.resetUserData.mockRejectedValueOnce({ code: "operation_conflict", detail: "Operasi lain aktif" });
    await act(async () => {
      await expect(result.current.resetUserData()).rejects.toMatchObject({ code: "operation_conflict" });
    });
    expect(result.current.result).toEqual(transcript);
    expect(result.current.error).toContain("Operasi lain aktif");
  });
});
