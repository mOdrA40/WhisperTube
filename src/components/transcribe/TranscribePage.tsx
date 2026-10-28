import { ControlsCard } from "./ControlsCard";
import { ProgressCard } from "./ProgressCard";
import { SourceCard } from "./SourceCard";
import { TranscriptCard } from "./TranscriptCard";
import { useState } from "react";
import { useI18n } from "../../i18n";
import { WhisperTubeLogo } from "../common/WhisperTubeLogo";
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
  TranscriptionResumeStage,
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
  transcriptionElapsedMs: number | null;
  selectedModel: ModelInfo | undefined;
  canStart: boolean;
  resumeStage: TranscriptionResumeStage | null;
  checkingResume: boolean;
  retryAvailable: boolean;
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
  onImportAudio?: () => void;
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
  transcriptionElapsedMs,
  selectedModel,
  canStart,
  resumeStage,
  checkingResume,
  retryAvailable,
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
  onImportAudio,
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
  const { t } = useI18n();
  const [configurationExpanded, setConfigurationExpanded] = useState(false);
  const reading = Boolean(result && !busy);
  const componentSetupActive = Object.keys(downloadingModel).length > 0 || installingCuda || installingAccelerator !== null;
  const configurationVisible = configurationExpanded || componentSetupActive;
  return (
    <div className={`transcribe-workspace ${reading ? "is-reading" : busy ? "is-processing" : "is-preparing"}`}>
      <div className="source-workspace">
        <SourceCard
          url={url}
          busy={busy || operationActive}
          inspecting={inspecting}
          cancelling={cancellingInspection}
          metadata={metadata}
          hasResult={hasResult}
          onUrlChange={onUrlChange}
          onInspect={onInspect}
          onImportAudio={onImportAudio}
          onCancel={onCancelInspection}
          onClear={onClear}
          onOpenSettings={() => onTabChange("settings")}
        />
      </div>
      <section className="transcribe-main-stream" aria-label={t("workspace.document")}>
        {!busy && !result && <div className="workspace-empty">
          <WhisperTubeLogo size={56} />
          <h2>{t("workspace.emptyTitle")}</h2>
          <p>{t("workspace.emptyHint")}</p>
          <ol className="workspace-steps">
            <li><span>1</span>{t("source.checkVideo")}</li>
            <li><span>2</span>{t("controls.title")}</li>
            <li><span>3</span>{t("controls.transcribe")}</li>
          </ol>
        </div>}
        {busy && <ProgressCard progress={progress} selectedModel={selectedModel} backend={backend} networkSpeedBytesPerSecond={networkSpeedBytesPerSecond} transcriptionElapsedMs={transcriptionElapsedMs} onCancel={onCancelTranscription} />}
        {result && !busy && (
          <TranscriptCard
            result={result}
            transcriptionElapsedMs={transcriptionElapsedMs}
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
        {(reading || busy) && <button type="button" className="configuration-toggle secondary-button"
          aria-expanded={configurationVisible} aria-controls="transcription-configuration" disabled={componentSetupActive}
          onClick={() => setConfigurationExpanded((expanded) => !expanded)}>
          {t(componentSetupActive ? "workspace.componentSetup" : configurationExpanded ? "workspace.hideConfiguration" : "workspace.showConfiguration")}
        </button>}
        <div id="transcription-configuration" className="configuration-body" hidden={(reading || busy) && !configurationVisible}>
        <ControlsCard
          models={models}
          modelId={modelId}
          selectedModel={selectedModel}
          canStart={canStart}
          resumeStage={resumeStage}
          checkingResume={checkingResume}
          retryAvailable={retryAvailable}
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
        </div>
      </aside>
    </div>
  );
}
