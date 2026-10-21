import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { relaunch } from "@tauri-apps/plugin-process";
import { open, save } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";
import type {
  AppUpdateInfo,
  AppUpdateProgress,
  AcceleratorDownloadPayload,
  BackendChoice,
  CudaDownloadPayload,
  HistoryDeleteOutcome,
  HistoryPageResult,
  ModelDownloadPayload,
  ModelInfo,
  ProgressPayload,
  SystemStatus,
  TranscriptRequest,
  TranscriptResult,
  VideoMetadata,
} from "../types";

let pendingAppUpdate: Update | null = null;
let activeAppUpdate: Update | null = null;
let installingAppUpdate = false;
let updateCheckGeneration = 0;
let pendingUpdateCheck: Promise<AppUpdateInfo | null> | null = null;
const UPDATE_CHECK_TIMEOUT_MS = 30_000;
const UPDATE_DOWNLOAD_TIMEOUT_MS = 15 * 60_000;

function appUpdateInfo(update: Update | null): AppUpdateInfo | null {
  if (!update) return null;
  return {
    version: update.version,
    notes: update.body?.trim() || null,
    date: update.date ?? null,
  };
}

async function closeAppUpdate(update: Update | null) {
  if (!update) return;
  try {
    await update.close();
  } catch (cause) {
    // A cleanup failure must not hide the original check/install error.
    console.warn("Gagal menutup resource update aplikasi.", cause);
  }
}

function acquirePendingAppUpdate(): Promise<AppUpdateInfo | null> {
  if (pendingUpdateCheck) return pendingUpdateCheck;
  const generation = updateCheckGeneration;
  const request = (async () => {
    const update = await check({ timeout: UPDATE_CHECK_TIMEOUT_MS });
    if (generation !== updateCheckGeneration || activeAppUpdate) {
      await closeAppUpdate(update);
      return appUpdateInfo(activeAppUpdate);
    }
    const previous = pendingAppUpdate;
    pendingAppUpdate = update;
    await closeAppUpdate(previous);
    return appUpdateInfo(update);
  })();
  pendingUpdateCheck = request;
  const clearRequest = () => {
    if (pendingUpdateCheck === request) pendingUpdateCheck = null;
  };
  // Use both handlers rather than an unobserved rejecting finally promise.
  void request.then(clearRequest, clearRequest);
  return request;
}

export function checkForAppUpdate(): Promise<AppUpdateInfo | null> {
  if (installingAppUpdate) return Promise.resolve(appUpdateInfo(activeAppUpdate ?? pendingAppUpdate));
  return acquirePendingAppUpdate();
}

export async function disposeAppUpdate() {
  updateCheckGeneration += 1;
  pendingUpdateCheck = null;
  const update = pendingAppUpdate;
  pendingAppUpdate = null;
  // An installing handle is owned exclusively by installAppUpdate's finally.
  await closeAppUpdate(update);
}

export async function installAppUpdate(onProgress: (progress: AppUpdateProgress) => void) {
  if (installingAppUpdate) throw new Error("APP_UPDATE_IN_PROGRESS");
  installingAppUpdate = true;
  let update: Update | null = null;
  try {
    if (!pendingAppUpdate) await acquirePendingAppUpdate();
    update = pendingAppUpdate;
    pendingAppUpdate = null;
    activeAppUpdate = update;
    if (!update) throw new Error("NO_UPDATE_AVAILABLE");

    await invoke("begin_app_update");
    try {
      await installPendingUpdate(update, onProgress);
    } finally {
      try {
        await invoke("end_app_update");
      } catch (cause) {
        console.warn("Gagal melepaskan reservation update aplikasi.", cause);
      }
    }
  } finally {
    await closeAppUpdate(update);
    activeAppUpdate = null;
    installingAppUpdate = false;
  }
}

async function installPendingUpdate(update: Update, onProgress: (progress: AppUpdateProgress) => void) {

  let downloadedBytes = 0;
  let totalBytes: number | null = null;
  onProgress({ downloadedBytes, totalBytes, percent: 0 });

  await update.downloadAndInstall((event: DownloadEvent) => {
    if (event.event === "Started") {
      totalBytes = event.data.contentLength ?? null;
    } else if (event.event === "Progress") {
      downloadedBytes += event.data.chunkLength;
    }

    onProgress({
      downloadedBytes,
      totalBytes,
      percent: totalBytes && totalBytes > 0 ? Math.min(100, (downloadedBytes / totalBytes) * 100) : 0,
    });
  }, { timeout: UPDATE_DOWNLOAD_TIMEOUT_MS });

  const isWindows = navigator.userAgent.toLowerCase().includes("windows");
  if (!isWindows) await relaunch();
}

export function getSystemStatus() {
  return invoke<SystemStatus>("system_status");
}

export function installAccelerator(backend: Exclude<BackendChoice, "auto" | "cpu" | "cuda">) {
  return invoke("install_accelerator", { backend });
}

export function listModels() {
  return invoke<ModelInfo[]>("list_models");
}

export function listHistory(beforeId: number | null = null) {
  return invoke<HistoryPageResult>("list_history", { beforeId });
}

export function inspectMedia(url: string, cookiesPath: string) {
  return invoke<VideoMetadata>("inspect_media", {
    url,
    cookiesPath: cookiesPath || null,
  });
}

export async function pickCookiesFile() {
  const selected = await open({
    multiple: false,
    directory: false,
    filters: [{ name: "Cookies", extensions: ["txt"] }],
  });
  return typeof selected === "string" ? selected : null;
}

export function openExternalUrl(url: string) {
  return openUrl(url);
}

export function revealAudioFile(path: string) {
  return invoke("reveal_audio", { path });
}

export function downloadModel(modelId: string, computeDeviceId: string | null = null) {
  return invoke("download_model", { modelId, computeDeviceId });
}

export function installCudaEngine() {
  return invoke("install_cuda_engine");
}

export function deleteModel(modelId: string) {
  return invoke("delete_model", { modelId });
}

export function resetUserData() {
  return invoke("reset_user_data");
}

export function startTranscription(request: TranscriptRequest) {
  return invoke<TranscriptResult>("start_transcription", { request });
}

export function cancelJob() {
  return invoke("cancel_job");
}

export function loadHistory(id: number) {
  return invoke<TranscriptResult>("load_history", { id });
}

export function deleteHistory(ids: number[]) {
  return invoke<HistoryDeleteOutcome>("delete_history", { ids });
}

export function subscribeToProgress(onProgress: (payload: ProgressPayload) => void) {
  return listen<ProgressPayload>("job-progress", (event) => onProgress(event.payload));
}

export function subscribeToModelDownload(onDownload: (payload: ModelDownloadPayload) => void) {
  return listen<ModelDownloadPayload>("model-download", (event) => onDownload(event.payload));
}

export function subscribeToCudaDownload(onDownload: (payload: CudaDownloadPayload) => void) {
  return listen<CudaDownloadPayload>("cuda-download", (event) => onDownload(event.payload));
}

export function subscribeToAcceleratorDownload(onDownload: (payload: AcceleratorDownloadPayload) => void) {
  return listen<AcceleratorDownloadPayload>("accelerator-download", (event) => onDownload(event.payload));
}

export async function exportTranscriptFile(result: TranscriptResult, kind: "txt" | "srt" | "vtt") {
  const source = kind === "txt" ? result.txtPath : kind === "srt" ? result.srtPath : result.vttPath;
  const safeTitle = result.title.replace(/[<>:"/\\|?*]+/g, "_").slice(0, 80);
  const target = await save({
    defaultPath: `${safeTitle}.${kind}`,
    filters: [{ name: kind.toUpperCase(), extensions: [kind] }],
  });
  if (target) {
    await invoke("copy_export", { source, target });
  }
}
