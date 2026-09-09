import { act, renderHook, waitFor } from "@testing-library/react";
import { type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import type { ModelInfo, SystemStatus, TranscriptResult, VideoMetadata } from "../types";
import { useWhisperTube } from "./useWhisperTube";

const services = vi.hoisted(() => ({
  getSystemStatus: vi.fn(),
  listModels: vi.fn(),
  listHistory: vi.fn(),
  inspectMedia: vi.fn(),
  startTranscription: vi.fn(),
  deleteHistory: vi.fn(),
  resetUserData: vi.fn(),
  checkForAppUpdate: vi.fn(),
  subscribeToProgress: vi.fn(),
  subscribeToModelDownload: vi.fn(),
  subscribeToCudaDownload: vi.fn(),
  subscribeToAcceleratorDownload: vi.fn(),
}));

vi.mock("../services/tauri", () => ({
  ...services,
  cancelJob: vi.fn(),
  deleteModel: vi.fn(),
  downloadModel: vi.fn(),
  exportTranscriptFile: vi.fn(),
  installAccelerator: vi.fn(),
  installCudaEngine: vi.fn(),
  installAppUpdate: vi.fn(),
  loadHistory: vi.fn(),
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
    services.listModels.mockResolvedValue(models);
    services.listHistory.mockResolvedValue({ items: [], hasMore: false, totalCount: 0 });
    services.inspectMedia.mockResolvedValue(metadata);
    services.startTranscription.mockResolvedValue(transcript);
    services.checkForAppUpdate.mockResolvedValue(null);
    const unlisten = vi.fn();
    services.subscribeToProgress.mockResolvedValue(unlisten);
    services.subscribeToModelDownload.mockResolvedValue(unlisten);
    services.subscribeToCudaDownload.mockResolvedValue(unlisten);
    services.subscribeToAcceleratorDownload.mockResolvedValue(unlisten);
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

  it("does not report a user-cancelled metadata inspection as an error", async () => {
    services.inspectMedia.mockRejectedValueOnce(new Error("Pemeriksaan metadata dibatalkan"));
    const { result } = renderHook(() => useWhisperTube(), { wrapper });
    await waitFor(() => expect(result.current.system).toEqual(system));

    act(() => result.current.setUrl(metadata.webpageUrl));
    await act(async () => result.current.inspectVideo());

    expect(result.current.error).toBeNull();
  });

  it("reconciles history after a delete that changed storage but returned a cleanup error", async () => {
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
    services.deleteHistory.mockRejectedValueOnce(new Error("History sudah dihapus dari database, tetapi file sementara gagal dibersihkan"));
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
    expect(result.current.error).toContain("History sudah dihapus dari database");
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
});
