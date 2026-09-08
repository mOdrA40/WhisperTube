import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { deleteModel, downloadModel, inspectMedia, installAppUpdate, listHistory, startTranscription } from "./tauri";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn().mockResolvedValue(undefined) }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn() }));

describe("Tauri IPC contracts", () => {
  beforeEach(() => vi.clearAllMocks());

  it("uses the backend's camelCase argument names", async () => {
    await downloadModel("base", null);
    await deleteModel("base");
    await listHistory(42);
    await inspectMedia("https://www.youtube.com/watch?v=test", "none", "", "");
    const request = {
      url: "https://www.youtube.com/watch?v=test",
      title: "Test",
      channel: "Channel",
      duration: 60,
      browser: "none" as const,
      browserProfile: "",
      cookiesPath: "",
      backend: "cpu" as const,
      computeDeviceId: null,
      language: "en",
      modelId: "base",
      keepAudio: false,
    };
    await startTranscription(request);

    expect(invoke).toHaveBeenNthCalledWith(1, "download_model", { modelId: "base", computeDeviceId: null });
    expect(invoke).toHaveBeenNthCalledWith(2, "delete_model", { modelId: "base" });
    expect(invoke).toHaveBeenNthCalledWith(3, "list_history", { beforeId: 42 });
    expect(invoke).toHaveBeenNthCalledWith(4, "inspect_media", {
      url: "https://www.youtube.com/watch?v=test",
      browser: "none",
      profile: null,
      cookiesPath: null,
    });
    expect(invoke).toHaveBeenNthCalledWith(5, "start_transcription", { request });
  });

  it("reserves and releases the shared operation around app updates", async () => {
    const update = {
      downloadAndInstall: vi.fn().mockResolvedValue(undefined),
    } as unknown as Update;
    vi.mocked(check).mockResolvedValue(update);

    await installAppUpdate(vi.fn());

    expect(invoke).toHaveBeenNthCalledWith(1, "begin_app_update");
    expect(update.downloadAndInstall).toHaveBeenCalledOnce();
    expect(invoke).toHaveBeenNthCalledWith(2, "end_app_update");
    expect(relaunch).toHaveBeenCalledOnce();
  });
});
