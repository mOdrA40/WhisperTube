export type AppTab = "transcribe" | "history" | "settings";
export type BrowserChoice = "none" | "chrome" | "edge" | "firefox" | "brave" | "chromium" | "opera" | "vivaldi" | "whale" | "safari";
export type BackendChoice = "auto" | "cpu" | "cuda" | "metal" | "vulkan";

export type AcceleratorInfo = {
  id: string;
  label: string;
  backend: Exclude<BackendChoice, "auto" | "cpu" | "cuda">;
  supported: boolean;
  installed: boolean;
  downloadable: boolean;
  description: string;
};

export type BrowserProfile = {
  id: string;
  label: string;
  isDefault: boolean;
};

export type BrowserInfo = {
  id: string;
  label: string;
  profiles: BrowserProfile[];
};

export type SystemStatus = {
  ytDlp: boolean;
  ffmpeg: boolean;
  cpuEngine: boolean;
  cudaEngine: boolean;
  cudaSupported: boolean;
  nvidia: boolean;
  gpuName: string | null;
  gpuMemoryMb: number | null;
  gpuFreeMemoryMb: number | null;
  cpuThreads: number;
  recommendation: string;
  recommendedModelId: string;
  recommendedBackend: BackendChoice;
  accelerators: AcceleratorInfo[];
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
  model: string;
  backend: string;
  segments: Segment[];
  text: string;
  txtPath: string;
  srtPath: string;
  vttPath: string;
  audioPath: string | null;
};

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

export type TranscriptRequest = {
  url: string;
  title: string;
  channel: string;
  duration: number;
  browser: BrowserChoice;
  browserProfile: string;
  cookiesPath: string;
  backend: BackendChoice;
  language: string;
  modelId: string;
  keepAudio: boolean;
};
