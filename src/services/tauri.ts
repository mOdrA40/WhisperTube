import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { relaunch } from "@tauri-apps/plugin-process";
import { open, save } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";
import type {
  AppUpdateInfo,
  AppUpdateProgress,
  BrowserChoice,
  BrowserInfo,
  AcceleratorDownloadPayload,
  BackendChoice,
  CudaDownloadPayload,
  HistoryItem,
  ModelDownloadPayload,
  ModelInfo,
  ProgressPayload,
  SystemStatus,
  TranscriptRequest,
  TranscriptResult,
  VideoMetadata,
} from "../types";

let pendingAppUpdate: Update | null = null;

export async function checkForAppUpdate(): Promise<AppUpdateInfo | null> {
  const update = await check();
  pendingAppUpdate = update;
  if (!update) return null;

  return {
    version: update.version,
    notes: update.body?.trim() || null,
    date: update.date ?? null,
  };
}

export async function installAppUpdate(onProgress: (progress: AppUpdateProgress) => void) {
  const update = pendingAppUpdate ?? await check();
  if (!update) throw new Error("NO_UPDATE_AVAILABLE");

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
  });

  pendingAppUpdate = null;
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

export function listBrowsers() {
  return invoke<BrowserInfo[]>("list_browsers");
}

export function listHistory() {
  return invoke<HistoryItem[]>("list_history");
}

export function inspectMedia(url: string, browser: BrowserChoice, browserProfile: string, cookiesPath: string) {
  return invoke<VideoMetadata>("inspect_media", {
    url,
    browser,
    profile: browserProfile || null,
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

export function downloadModel(modelId: string) {
  return invoke("download_model", { modelId });
}

export function installCudaEngine() {
  return invoke("install_cuda_engine");
}

export function deleteModel(modelId: string) {
  return invoke("delete_model", { modelId });
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
  return invoke("delete_history", { ids });
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
