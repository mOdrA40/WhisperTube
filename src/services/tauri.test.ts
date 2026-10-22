import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { save } from "@tauri-apps/plugin-dialog";
import type { TranscriptResult } from "../types";
import { checkForAppUpdate, disposeAppUpdate, deleteModel, discardJobCache, downloadModel, exportTranscriptFile, inspectMedia, installAppUpdate, listHistory, listJobCache, startTranscription } from "./tauri";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn().mockResolvedValue(undefined) }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn(), open: vi.fn() }));

describe("Tauri IPC contracts", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(invoke).mockResolvedValue(undefined);
  });
  afterEach(() => disposeAppUpdate());

  it("uses explicit cache IDs for selective cleanup IPC", async () => {
    await listJobCache();
    await discardJobCache("cache-id");
    expect(invoke).toHaveBeenNthCalledWith(1, "list_job_cache");
    expect(invoke).toHaveBeenNthCalledWith(2, "discard_job_cache", { id: "cache-id" });
  });

  it.each([["a".repeat(79) + "😀suffix", "a".repeat(79)], ["😀".repeat(90), "😀".repeat(40)], ["日".repeat(90), "日".repeat(80)], ["judul 日本語 e\u0301", "judul 日本語 e\u0301"]]) (
    "keeps export filenames valid Unicode across the length boundary: %s", async (title, expected) => {
      vi.mocked(save).mockResolvedValue("C:/exports/transcript.txt");
      const result = { title, txtPath: "C:/jobs/job/transcript.txt" } as TranscriptResult;
      await exportTranscriptFile(result, "txt");
      const options = vi.mocked(save).mock.calls.at(-1)![0]!;
      const filename = options.defaultPath!;
      expect(filename).not.toMatch(/[\uD800-\uDFFF]/u);
      expect(filename).toBe(expected + ".txt");
      expect(new TextEncoder().encode(filename).length).toBeLessThanOrEqual(255);
      expect(invoke).toHaveBeenCalledWith("copy_export", { source: result.txtPath, target: "C:/exports/transcript.txt" });
    },
  );

  it("uses the backend's camelCase argument names", async () => {
    await downloadModel("base", null);
    await deleteModel("base");
    await listHistory(42);
    await inspectMedia("https://www.youtube.com/watch?v=test", "");
    const request = {
      url: "https://www.youtube.com/watch?v=test",
      title: "Test",
      channel: "Channel",
      duration: 60,
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
      cookiesPath: null,
    });
    expect(invoke).toHaveBeenNthCalledWith(5, "start_transcription", { request });
  });

  it("reserves and releases the shared operation around app updates", async () => {
    const update = {
      downloadAndInstall: vi.fn().mockResolvedValue(undefined),
      close: vi.fn().mockResolvedValue(undefined),
    } as unknown as Update;
    vi.mocked(check).mockResolvedValue(update);

    await installAppUpdate(vi.fn());

    expect(invoke).toHaveBeenNthCalledWith(1, "begin_app_update");
    expect(update.downloadAndInstall).toHaveBeenCalledOnce();
    expect(check).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(update.downloadAndInstall).toHaveBeenCalledWith(expect.any(Function), { timeout: 900_000 });
    expect(invoke).toHaveBeenNthCalledWith(2, "end_app_update");
    expect(relaunch).toHaveBeenCalledOnce();
  });
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (cause: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function updateFixture(version = "0.3.0") {
  return {
    version,
    body: " Update notes ",
    close: vi.fn().mockResolvedValue(undefined),
    downloadAndInstall: vi.fn().mockResolvedValue(undefined),
  } as unknown as Update;
}

describe("update resource ownership", () => {
  beforeEach(async () => {
    await disposeAppUpdate();
    vi.resetAllMocks();
    vi.mocked(invoke).mockResolvedValue(undefined);
    vi.mocked(relaunch).mockResolvedValue(undefined);
  });
  afterEach(async () => {
    await disposeAppUpdate();
    vi.restoreAllMocks();
  });

  it("closes replaced handles, including an available-to-no-update transition", async () => {
    const first = updateFixture();
    const second = updateFixture("0.4.0");
    vi.mocked(check).mockResolvedValueOnce(first).mockResolvedValueOnce(second).mockResolvedValueOnce(null);
    expect(await checkForAppUpdate()).toMatchObject({ version: "0.3.0", notes: "Update notes" });
    await checkForAppUpdate();
    expect(first.close).toHaveBeenCalledOnce();
    expect(second.close).not.toHaveBeenCalled();
    expect(await checkForAppUpdate()).toBeNull();
    expect(second.close).toHaveBeenCalledOnce();
    await disposeAppUpdate();
    expect(second.close).toHaveBeenCalledOnce();
  });

  it("coalesces concurrent checks into one owned handle", async () => {
    const response = deferred<Update>();
    const update = updateFixture();
    vi.mocked(check).mockReturnValue(response.promise);
    const first = checkForAppUpdate();
    const second = checkForAppUpdate();
    expect(first).toBe(second);
    expect(check).toHaveBeenCalledOnce();
    response.resolve(update);
    await first;
    await disposeAppUpdate();
    expect(update.close).toHaveBeenCalledOnce();
  });

  it("closes a late disposed check without replacing the next owner's update", async () => {
    const oldResponse = deferred<Update>();
    const oldUpdate = updateFixture("0.3.0");
    const currentUpdate = updateFixture("0.4.0");
    vi.mocked(check).mockReturnValueOnce(oldResponse.promise).mockResolvedValueOnce(currentUpdate);
    const oldCheck = checkForAppUpdate();
    await disposeAppUpdate();
    await checkForAppUpdate();
    oldResponse.resolve(oldUpdate);
    await oldCheck;
    expect(oldUpdate.close).toHaveBeenCalledOnce();
    expect(currentUpdate.close).not.toHaveBeenCalled();
    await installAppUpdate(vi.fn());
    expect(currentUpdate.downloadAndInstall).toHaveBeenCalledOnce();
    expect(currentUpdate.close).toHaveBeenCalledOnce();
  });

  it("preserves a usable handle on check failure and releases it on unmount", async () => {
    const update = updateFixture();
    vi.mocked(check).mockResolvedValueOnce(update).mockRejectedValueOnce(new Error("check failed"));
    await checkForAppUpdate();
    await expect(checkForAppUpdate()).rejects.toThrow("check failed");
    expect(update.close).not.toHaveBeenCalled();
    await disposeAppUpdate();
    expect(update.close).toHaveBeenCalledOnce();
  });

  it("keeps the installing handle alive across checks, duplicate installs and unmount", async () => {
    const download = deferred<void>();
    const update = updateFixture();
    vi.mocked(update.downloadAndInstall).mockReturnValue(download.promise);
    vi.mocked(check).mockResolvedValue(update);
    await checkForAppUpdate();
    const installation = installAppUpdate(vi.fn());
    await vi.waitFor(() => expect(update.downloadAndInstall).toHaveBeenCalledOnce());
    await checkForAppUpdate();
    await expect(installAppUpdate(vi.fn())).rejects.toThrow("APP_UPDATE_IN_PROGRESS");
    await disposeAppUpdate();
    expect(check).toHaveBeenCalledOnce();
    expect(update.close).not.toHaveBeenCalled();
    download.resolve();
    await installation;
    expect(update.close).toHaveBeenCalledOnce();
    expect(invoke).toHaveBeenCalledWith("end_app_update");
  });

  it("releases failed downloads and obtains a fresh handle for retry", async () => {
    const failed = updateFixture();
    const retry = updateFixture();
    vi.mocked(failed.downloadAndInstall).mockRejectedValue(new Error("download failed"));
    vi.mocked(check).mockResolvedValueOnce(failed).mockResolvedValueOnce(retry);
    await expect(installAppUpdate(vi.fn())).rejects.toThrow("download failed");
    expect(failed.close).toHaveBeenCalledOnce();
    expect(invoke).toHaveBeenCalledWith("end_app_update");
    await installAppUpdate(vi.fn());
    expect(retry.downloadAndInstall).toHaveBeenCalledOnce();
    expect(retry.close).toHaveBeenCalledOnce();
  });

  it("releases the reservation and native handle after a download timeout", async () => {
    const update = updateFixture();
    vi.mocked(update.downloadAndInstall).mockRejectedValueOnce(new Error("request timed out"));
    vi.mocked(check).mockResolvedValueOnce(update);
    await expect(installAppUpdate(vi.fn())).rejects.toThrow("request timed out");
    expect(update.downloadAndInstall).toHaveBeenCalledWith(expect.any(Function), { timeout: 900_000 });
    expect(invoke).toHaveBeenCalledWith("end_app_update");
    expect(update.close).toHaveBeenCalledOnce();
    expect(relaunch).not.toHaveBeenCalled();
  });

  it("closes a refresh result that arrives during installation without closing the active handle", async () => {
    const active = updateFixture();
    const late = updateFixture("0.4.0");
    const refresh = deferred<Update>();
    const download = deferred<void>();
    vi.mocked(active.downloadAndInstall).mockReturnValue(download.promise);
    vi.mocked(check).mockResolvedValueOnce(active).mockReturnValueOnce(refresh.promise);
    await checkForAppUpdate();
    const checking = checkForAppUpdate();
    const installation = installAppUpdate(vi.fn());
    await vi.waitFor(() => expect(active.downloadAndInstall).toHaveBeenCalledOnce());
    refresh.resolve(late);
    await checking;
    expect(late.close).toHaveBeenCalledOnce();
    expect(active.close).not.toHaveBeenCalled();
    download.resolve();
    await installation;
    expect(active.close).toHaveBeenCalledOnce();
  });

  it("releases a handle when reservation fails without releasing someone else's operation", async () => {
    const update = updateFixture();
    vi.mocked(check).mockResolvedValue(update);
    vi.mocked(invoke).mockRejectedValueOnce(new Error("operation_conflict"));
    await expect(installAppUpdate(vi.fn())).rejects.toThrow("operation_conflict");
    expect(update.downloadAndInstall).not.toHaveBeenCalled();
    expect(update.close).toHaveBeenCalledOnce();
    expect(invoke).not.toHaveBeenCalledWith("end_app_update");
  });

  it("reports cleanup failures while preserving the original installation error", async () => {
    const update = updateFixture();
    const warning = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    vi.mocked(check).mockResolvedValue(update);
    vi.mocked(update.downloadAndInstall).mockRejectedValue(new Error("download failed"));
    vi.mocked(update.close).mockRejectedValue(new Error("close failed"));
    await expect(installAppUpdate(vi.fn())).rejects.toThrow("download failed");
    expect(warning).toHaveBeenCalledWith("Gagal menutup resource update aplikasi.", expect.any(Error));
  });

  it("handles no-update and failed fallback checks without leaving installation busy", async () => {
    vi.mocked(check).mockResolvedValueOnce(null).mockRejectedValueOnce(new Error("offline")).mockResolvedValueOnce(null);
    await expect(installAppUpdate(vi.fn())).rejects.toThrow("NO_UPDATE_AVAILABLE");
    await expect(installAppUpdate(vi.fn())).rejects.toThrow("offline");
    expect(await checkForAppUpdate()).toBeNull();
    expect(invoke).not.toHaveBeenCalled();
  });
});
