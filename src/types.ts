export type AppTab = "transcribe" | "history" | "settings";
export type BackendChoice = "auto" | "cpu" | "cuda" | "metal" | "vulkan";

export type AppUpdateInfo = {
  version: string;
  notes: string | null;
  date: string | null;
};

export type AppUpdateStatus = "idle" | "checking" | "available" | "up-to-date" | "installing";

export type BackendErrorPayload = {
  code: string;
  detail?: string | null;
};

export type AppUpdateProgress = {
  downloadedBytes: number;
  totalBytes: number | null;
  percent: number;
};

export type AcceleratorInfo = {
  id: string;
  label: string;
  backend: Exclude<BackendChoice, "auto" | "cpu" | "cuda">;
  supported: boolean;
  installed: boolean;
  downloadable: boolean;
  memoryAvailableMb: number | null;
  memoryRequiredMb: number | null;
  memorySufficient: boolean;
  preflightChecked: boolean;
  preflightCompatible: boolean;
  description: string;
};

export type SystemStatus = {
  qwenBackends: BackendChoice[];
  ytDlp: boolean;
  ffmpeg: boolean;
  cpuEngine: boolean;
  cudaEngine: boolean;
  cudaSupported: boolean;
  nvidia: boolean;
  gpuName: string | null;
  gpuMemoryMb: number | null;
  gpuFreeMemoryMb: number | null;
  availableMemoryMb: number;
  nvidiaDriverVersion: string | null;
  cudaInstallable: boolean;
  cpuThreads: number;
  recommendation: string;
  recommendedModelId: string;
  recommendedBackend: BackendChoice;
  accelerators: AcceleratorInfo[];
  computeDevices: ComputeDeviceInfo[];
  jobStorageBytes: number;
  jobStorageLimitBytes: number;
};

export type ComputeDeviceInfo = {
  id: string;
  backend: Exclude<BackendChoice, "auto" | "cpu">;
  name: string;
  vendor: string;
  deviceIndex: number | null;
  integrated: boolean;
  totalMemoryMb: number | null;
  freeMemoryMb: number | null;
};

export type ModelInfo = {
  id: string;
  label: string;
  description: string;
  sizeMb: number;
  vramRequiredMb: number;
  installed: boolean;
};

export type VideoMetadata = {
  id: string;
  title: string;
  channel: string;
  duration: number;
  thumbnail: string | null;
  webpageUrl: string;
  availability: string | null;
  source: string;
};

export type ProgressStage =
  | "idle"
  | "downloading"
  | "converting"
  | "transcribing"
  | "finalizing"
  | "done"
  | "error";

export type ProgressPayload = {
  stage: ProgressStage;
  messageCode: string;
  percent: number;
  message: string;
  backend: string | null;
  downloadedBytes: number | null;
  totalBytes: number | null;
  networkBytesPerSecond: number | null;
  cpuUsagePercent: number | null;
  gpuUsagePercent: number | null;
};

export type ModelDownloadPayload = {
  id: string;
  phase?: "model" | "runtime";
  downloadedBytes: number;
  totalBytes: number;
  percent: number;
  bytesPerSecond: number | null;
};

export type CudaDownloadPayload = {
  downloadedBytes: number;
  totalBytes: number;
  percent: number;
  bytesPerSecond: number | null;
};

export type AcceleratorDownloadPayload = {
  backend: Exclude<BackendChoice, "auto" | "cpu" | "cuda">;
  downloadedBytes: number;
  totalBytes: number;
  percent: number;
  bytesPerSecond: number | null;
};

export type Segment = {
  from: string;
  to: string;
  text: string;
};

export type TranscriptResult = {
  historyId: number;
  title: string;
  channel: string;
  language: string;
  duration: number;
  sourceDuration?: number | null;
  model: string;
  backend: string;
  segments: Segment[];
  text: string;
  txtPath: string;
  srtPath: string;
  vttPath: string;
  audioPath: string | null;
  transcriptAvailable?: boolean;
};

export type HistoryFileKind = "audio" | "txt" | "srt" | "vtt";
export type HistoryFileScope = "audio" | "transcript";
export type HistoryFileInfo = { kind: HistoryFileKind; path: string; bytes: number | null; available: boolean; pendingDeletion: boolean };
export type HistoryFilesDeleteOutcome = { updatedIds: number[]; cleanupWarnings: string[] };

export type HistoryItem = {
  id: number;
  title: string;
  channel: string;
  sourceUrl: string;
  createdAt: string;
  duration: number;
  language: string;
  model: string;
  backend: string;
};

export type HistoryDeleteOutcome = {
  deletedIds: number[];
  cleanupWarnings: string[];
};

export type JobCacheEntry = {
  id: string;
  title: string;
  sourceUrl: string;
  bytes: number | null;
  modifiedAt: string;
};

export type HistoryPageResult = {
  items: HistoryItem[];
  hasMore: boolean;
  totalCount: number;
};

export type TranscriptionResumeStage = "download" | "audio";

export type TranscriptRequest = {
  url: string;
  title: string;
  channel: string;
  duration: number;
  cookiesPath: string;
  backend: BackendChoice;
  computeDeviceId: string | null;
  language: string;
  modelId: string;
  keepAudio: boolean;
};

export interface TranscriptionNotificationCopy {
  title: string;
  body: string;
}

export interface TranscriptionNotifications {
  success: TranscriptionNotificationCopy;
  completedWithNote: TranscriptionNotificationCopy;
  failed: TranscriptionNotificationCopy;
  cancelled: TranscriptionNotificationCopy;
}
