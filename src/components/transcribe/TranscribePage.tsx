import { ControlsCard } from "./ControlsCard";
import { ProgressCard } from "./ProgressCard";
import { SourceCard } from "./SourceCard";
import { TranscriptCard } from "./TranscriptCard";
import type {
  AppTab,
  AcceleratorInfo,
  BackendChoice,
  ModelDownloadPayload,
  ModelInfo,
  ProgressPayload,
  Segment,
  SystemStatus,
  TranscriptResult,
  VideoMetadata,
} from "../../types";

type TranscribePageProps = {
  url: string;
  busy: boolean;
  operationActive: boolean;
  inspecting: boolean;
  cancellingInspection: boolean;
  metadata: VideoMetadata | null;
  hasResult: boolean;
  progress: ProgressPayload;
  selectedModel: ModelInfo | undefined;
  canStart: boolean;
  modelDownloadBlocked: boolean;
  backend: BackendChoice;
  computeTargetId: string;
  system: SystemStatus | null;
  models: ModelInfo[];
  modelId: string;
  language: string;
  keepAudio: boolean;
  runtimeReady: boolean;
  downloadingModel: Record<string, ModelDownloadPayload>;
  accelerators: AcceleratorInfo[];
  installingCuda: boolean;
  cudaDownloadPercent: number;
  installingAccelerator: Exclude<BackendChoice, "auto" | "cpu" | "cuda"> | null;
  acceleratorDownloadPercent: number;
  networkSpeedBytesPerSecond: number | null;
  vramWarning: string | null;
  acceleratorWarning: string | null;
  result: TranscriptResult | null;
  copied: boolean;
  searchQuery: string;
  filteredSegments: Segment[];
  onTabChange: (tab: AppTab) => void;
  onUrlChange: (url: string) => void;
  onInspect: () => void;
  onClear: () => void;
  onCancelInspection: () => void;
  onCancelTranscription: () => void;
  onModelChange: (id: string) => void;
  onComputeTargetChange: (targetId: string) => void;
  onLanguageChange: (language: string) => void;
  onKeepAudioChange: (keepAudio: boolean) => void;
  onDownloadModel: (id: string) => void;
  onCancelModel: () => void;
  onInstallCuda: () => void;
  onCancelCuda: () => void;
  onInstallAccelerator: (backend: Exclude<BackendChoice, "auto" | "cpu" | "cuda">) => void;
  onCancelAccelerator: () => void;
  onStart: () => void;
  onCopy: () => void;
  onExport: (kind: "txt" | "srt" | "vtt") => void;
  onRevealAudio: () => void;
  onSearchChange: (query: string) => void;
};

export function TranscribePage({
  url,
  busy,
  operationActive,
  inspecting,
  cancellingInspection,
  metadata,
  hasResult,
  progress,
  selectedModel,
  canStart,
  modelDownloadBlocked,
  backend,
  computeTargetId,
  system,
  models,
  modelId,
  language,
  keepAudio,
  runtimeReady,
  downloadingModel,
  accelerators,
  installingCuda,
  cudaDownloadPercent,
  installingAccelerator,
  acceleratorDownloadPercent,
  networkSpeedBytesPerSecond,
  vramWarning,
  acceleratorWarning,
  result,
  copied,
  searchQuery,
  filteredSegments,
  onTabChange,
  onUrlChange,
  onInspect,
  onClear,
  onCancelInspection,
  onCancelTranscription,
  onModelChange,
  onComputeTargetChange,
  onLanguageChange,
  onKeepAudioChange,
  onDownloadModel,
  onCancelModel,
  onInstallCuda,
  onCancelCuda,
  onInstallAccelerator,
  onCancelAccelerator,
  onStart,
  onCopy,
  onExport,
  onRevealAudio,
  onSearchChange,
}: TranscribePageProps) {
  return (
    <div className="transcribe-layout-grid">
      <section className="transcribe-main-stream">
        <SourceCard
          url={url}
          busy={busy || operationActive}
          inspecting={inspecting}
          cancelling={cancellingInspection}
          metadata={metadata}
          hasResult={hasResult}
          onUrlChange={onUrlChange}
          onInspect={onInspect}
          onCancel={onCancelInspection}
          onClear={onClear}
          onOpenSettings={() => onTabChange("settings")}
        />
        {busy && <ProgressCard progress={progress} selectedModel={selectedModel} backend={backend} networkSpeedBytesPerSecond={networkSpeedBytesPerSecond} onCancel={onCancelTranscription} />}
        {result && !busy && (
          <TranscriptCard
            result={result}
            copied={copied}
            searchQuery={searchQuery}
            filteredSegments={filteredSegments}
            onCopy={onCopy}
            onExport={onExport}
            onRevealAudio={onRevealAudio}
            operationActive={operationActive}
            onSearchChange={onSearchChange}
          />
        )}
      </section>

      <aside className="transcribe-sidebar-stream">
        <ControlsCard
          models={models}
          modelId={modelId}
          selectedModel={selectedModel}
          canStart={canStart}
          modelDownloadBlocked={modelDownloadBlocked}
          computeTargetId={computeTargetId}
          language={language}
          keepAudio={keepAudio}
          system={system}
          busy={busy}
          operationActive={operationActive}
          runtimeReady={runtimeReady}
          downloadingModel={downloadingModel}
          accelerators={accelerators}
          installingCuda={installingCuda}
          cudaDownloadPercent={cudaDownloadPercent}
          installingAccelerator={installingAccelerator}
          acceleratorDownloadPercent={acceleratorDownloadPercent}
          networkSpeedBytesPerSecond={networkSpeedBytesPerSecond}
          vramWarning={vramWarning}
          acceleratorWarning={acceleratorWarning}
          onModelChange={onModelChange}
          onComputeTargetChange={onComputeTargetChange}
          onLanguageChange={onLanguageChange}
          onKeepAudioChange={onKeepAudioChange}
          onDownloadModel={onDownloadModel}
          onCancelModel={onCancelModel}
          onInstallCuda={onInstallCuda}
          onCancelCuda={onCancelCuda}
          onInstallAccelerator={onInstallAccelerator}
          onCancelAccelerator={onCancelAccelerator}
          onStart={onStart}
        />
      </aside>
    </div>
  );
}
