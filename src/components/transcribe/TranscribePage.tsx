import { ControlsCard } from "./ControlsCard";
import { ProgressCard } from "./ProgressCard";
import { SourceCard } from "./SourceCard";
import { TranscriptCard } from "./TranscriptCard";
import type {
  AppTab,
  BackendChoice,
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
  inspecting: boolean;
  metadata: VideoMetadata | null;
  progress: ProgressPayload;
  selectedModel: ModelInfo | undefined;
  canStart: boolean;
  backend: BackendChoice;
  system: SystemStatus | null;
  models: ModelInfo[];
  modelId: string;
  language: string;
  keepAudio: boolean;
  runtimeReady: boolean;
  downloadingModel: Record<string, number>;
  installingCuda: boolean;
  cudaDownloadPercent: number;
  vramWarning: string | null;
  result: TranscriptResult | null;
  copied: boolean;
  searchQuery: string;
  filteredSegments: Segment[];
  onTabChange: (tab: AppTab) => void;
  onUrlChange: (url: string) => void;
  onInspect: () => void;
  onCancel: () => void;
  onModelChange: (id: string) => void;
  onBackendChange: (backend: BackendChoice) => void;
  onLanguageChange: (language: string) => void;
  onKeepAudioChange: (keepAudio: boolean) => void;
  onDownloadModel: (id: string) => void;
  onInstallCuda: () => void;
  onCancelCuda: () => void;
  onStart: () => void;
  onCopy: () => void;
  onExport: (kind: "txt" | "srt" | "vtt") => void;
  onSearchChange: (query: string) => void;
};

export function TranscribePage({
  url,
  busy,
  inspecting,
  metadata,
  progress,
  selectedModel,
  canStart,
  backend,
  system,
  models,
  modelId,
  language,
  keepAudio,
  runtimeReady,
  downloadingModel,
  installingCuda,
  cudaDownloadPercent,
  vramWarning,
  result,
  copied,
  searchQuery,
  filteredSegments,
  onTabChange,
  onUrlChange,
  onInspect,
  onCancel,
  onModelChange,
  onBackendChange,
  onLanguageChange,
  onKeepAudioChange,
  onDownloadModel,
  onInstallCuda,
  onCancelCuda,
  onStart,
  onCopy,
  onExport,
  onSearchChange,
}: TranscribePageProps) {
  return (
    <div className="content-grid">
      <section className="primary-column">
        <SourceCard
          url={url}
          busy={busy}
          inspecting={inspecting}
          metadata={metadata}
          onUrlChange={onUrlChange}
          onInspect={onInspect}
          onOpenSettings={() => onTabChange("settings")}
        />
        {busy && <ProgressCard progress={progress} selectedModel={selectedModel} backend={backend} onCancel={onCancel} />}
        {result && !busy && (
          <TranscriptCard
            result={result}
            copied={copied}
            searchQuery={searchQuery}
            filteredSegments={filteredSegments}
            onCopy={onCopy}
            onExport={onExport}
            onSearchChange={onSearchChange}
          />
        )}
      </section>

      <aside className="control-column">
        <ControlsCard
          models={models}
          modelId={modelId}
          selectedModel={selectedModel}
          canStart={canStart}
          backend={backend}
          language={language}
          keepAudio={keepAudio}
          system={system}
          busy={busy}
          runtimeReady={runtimeReady}
          downloadingModel={downloadingModel}
          installingCuda={installingCuda}
          cudaDownloadPercent={cudaDownloadPercent}
          vramWarning={vramWarning}
          onModelChange={onModelChange}
          onBackendChange={onBackendChange}
          onLanguageChange={onLanguageChange}
          onKeepAudioChange={onKeepAudioChange}
          onDownloadModel={onDownloadModel}
          onInstallCuda={onInstallCuda}
          onCancelCuda={onCancelCuda}
          onStart={onStart}
        />
      </aside>
    </div>
  );
}
