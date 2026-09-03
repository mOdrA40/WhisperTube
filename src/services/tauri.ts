import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { save } from "@tauri-apps/plugin-dialog";
import type {
  BrowserChoice,
  BrowserInfo,
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

export function getSystemStatus() {
  return invoke<SystemStatus>("system_status");
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

export function inspectYoutube(url: string, browser: BrowserChoice, browserProfile: string) {
  return invoke<VideoMetadata>("inspect_youtube", { url, browser, profile: browserProfile || null });
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

export function subscribeToProgress(onProgress: (payload: ProgressPayload) => void) {
  return listen<ProgressPayload>("job-progress", (event) => onProgress(event.payload));
}

export function subscribeToModelDownload(onDownload: (payload: ModelDownloadPayload) => void) {
  return listen<ModelDownloadPayload>("model-download", (event) => onDownload(event.payload));
}

export function subscribeToCudaDownload(onDownload: (payload: CudaDownloadPayload) => void) {
  return listen<CudaDownloadPayload>("cuda-download", (event) => onDownload(event.payload));
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
