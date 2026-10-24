import {
  AlertCircle,
  AudioWaveform,
  Check,
  CircleStop,
  Cpu,
  Download,
  Gauge,
  LoaderCircle,
  Sparkles,
  Zap,
} from "lucide-react";
import { formatBytes, formatMemory } from "../../lib/format";
import type { AcceleratorInfo, BackendChoice, ModelDownloadPayload, ModelInfo, SystemStatus, TranscriptionResumeStage } from "../../types";
import { getAcceleratorCopy, getModelCopy, getRecommendation, useI18n } from "../../i18n";
import { CustomSelect, type SelectOption } from "../common/CustomSelect";

type ControlsCardProps = {
  models: ModelInfo[];
  modelId: string;
  selectedModel: ModelInfo | undefined;
  canStart: boolean;
  resumeStage: TranscriptionResumeStage | null;
  checkingResume: boolean;
  retryAvailable: boolean;
  modelDownloadBlocked: boolean;
  computeTargetId: string;
  language: string;
  keepAudio: boolean;
  system: SystemStatus | null;
  busy: boolean;
  operationActive: boolean;
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
  onInstallCuda: () => void;
  onCancelCuda: () => void;
  onCancelModel: () => void;
  onInstallAccelerator: (backend: Exclude<BackendChoice, "auto" | "cpu" | "cuda">) => void;
  onCancelAccelerator: () => void;
  onModelChange: (id: string) => void;
  onComputeTargetChange: (targetId: string) => void;
  onLanguageChange: (language: string) => void;
  onKeepAudioChange: (keepAudio: boolean) => void;
  onDownloadModel: (id: string) => void;
  onStart: () => void;
};

export function ControlsCard({
  models,
  modelId,
  selectedModel,
  canStart,
  resumeStage,
  checkingResume,
  retryAvailable,
  modelDownloadBlocked,
  computeTargetId,
  language,
  keepAudio,
  system,
  busy,
  operationActive,
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
  onInstallCuda,
  onCancelCuda,
  onCancelModel,
  onInstallAccelerator,
  onCancelAccelerator,
  onModelChange,
  onComputeTargetChange,
  onLanguageChange,
  onKeepAudioChange,
  onDownloadModel,
  onStart,
}: ControlsCardProps) {
  const { t } = useI18n();
  const controlsDisabled = busy || operationActive;
  const selectedDownload = selectedModel && downloadingModel[selectedModel.id];
  const modelDownloadActive = Object.keys(downloadingModel).length > 0;
  const languageOptions: SelectOption[] = [
    { value: "auto", label: t("language.auto") },
    { value: "id", label: t("language.id") },
    { value: "en", label: t("language.en") },
    { value: "zh", label: t("language.zh") },
    { value: "ja", label: t("language.ja") },
    { value: "ko", label: t("language.ko") },
  ];

  const computeTargetOptions: SelectOption[] = [
    {
      value: "auto",
      label: t("backend.auto"),
    },
    {
      value: "cpu",
      label: t("backend.cpu"),
    },
  ];
  computeTargetOptions.push(
    ...(system?.computeDevices ?? []).map((device) => ({
      value: device.id,
      label: `${device.backend.toUpperCase()} · ${device.name}${device.integrated ? " (iGPU)" : ""}`,
    })),
  );
  for (const accelerator of accelerators.filter((item) => item.supported && item.installed)) {
    if (accelerator.backend === "metal" && !computeTargetOptions.some((option) => option.value === "metal")) {
      computeTargetOptions.push({ value: "metal", label: getAcceleratorCopy(accelerator, t).label });
    }
  }
  const memoryRequirementKey: "controls.cudaRequirement" | "controls.memoryEstimate" =
    computeTargetId.startsWith("cuda") ||
    (computeTargetId === "auto" && system?.cudaInstallable && system.cudaEngine &&
      system.computeDevices.some((device) => device.backend === "cuda"))
      ? "controls.cudaRequirement"
      : "controls.memoryEstimate";
  return (
    <div className="card control-card sticky-card">
      <div className="card-title-row">
        <div>
          <span className="eyebrow">{t("controls.eyebrow")}</span>
          <h3>{t("controls.title")}</h3>
        </div>
        <Gauge size={20} />
      </div>

      <label className="field-label">{t("controls.model")}</label>
      <div className="model-options" role="radiogroup" aria-label={t("controls.model")}>
        {models.map((model) => {
          const isSelected = modelId === model.id;

          return (
            <button
              key={model.id}
              type="button"
              role="radio"
              aria-checked={isSelected}
              tabIndex={isSelected || (!models.some((item) => item.id === modelId) && model === models[0]) ? 0 : -1}
              className={isSelected ? "model-option selected" : "model-option"}
              onClick={() => onModelChange(model.id)}
              onKeyDown={(event) => {
                if (controlsDisabled || modelDownloadActive) return;
                const forward = event.key === "ArrowRight" || event.key === "ArrowDown";
                const backward = event.key === "ArrowLeft" || event.key === "ArrowUp";
                if (!forward && !backward && event.key !== "Home" && event.key !== "End") return;
                event.preventDefault();
                const options = Array.from(event.currentTarget.parentElement!.querySelectorAll<HTMLButtonElement>('[role="radio"]:not(:disabled)'));
                const index = options.indexOf(event.currentTarget);
                const next = event.key === "Home" ? 0 : event.key === "End" ? options.length - 1
                  : (index + (forward ? 1 : -1) + options.length) % options.length;
                const target = options[next];
                target?.focus();
                target?.click();
              }}
              disabled={controlsDisabled || modelDownloadActive}
            >
              <div className="radio-dot">
                <span />
              </div>
              <div className="model-copy">
                <strong>{getModelCopy(model, t).label}</strong>
                <span>
                  {getModelCopy(model, t).description} • {t(memoryRequirementKey, { memory: formatMemory(model.vramRequiredMb, t("hardware.notDetected")) })}
                </span>
              </div>
              <div className="model-state">
                {model.installed ? <Check size={15} /> : `${model.sizeMb} MB`}
              </div>
            </button>
          );
        })}
      </div>

      {selectedModel && !selectedModel.installed && (
        <button
          type="button"
          className="download-button"
          onClick={() => onDownloadModel(selectedModel.id)}
          disabled={selectedDownload !== undefined || modelDownloadActive || controlsDisabled || modelDownloadBlocked}
        >
          {selectedDownload !== undefined ? <LoaderCircle className="spin" size={17} /> : <Download size={17} />}
          {selectedDownload !== undefined
            ? (
              <span className="download-progress-copy">
                <span>{t("controls.downloadingModel", { percent: Math.round(selectedDownload.percent) })}</span>
                <small>
                  {formatBytes(selectedDownload.downloadedBytes, t("progress.unknownSize"))} / {selectedDownload.totalBytes > 0
                    ? formatBytes(selectedDownload.totalBytes)
                    : t("progress.unknownSize")}
                  {networkSpeedBytesPerSecond !== null && ` • ${formatBytes(networkSpeedBytesPerSecond)}/s`}
                </small>
              </span>
            )
            : t("controls.downloadModel", { model: getModelCopy(selectedModel, t).label })}
        </button>
      )}
      {selectedDownload !== undefined && (
        <button type="button" className="danger-ghost" onClick={onCancelModel}>
          <CircleStop size={16} /> {t("controls.cancelModel")}
        </button>
      )}

      {system?.cudaSupported && system.nvidia && !system.cudaEngine && (
        <>
          <div className="info-box">
            <Sparkles size={16} />
            <span>{t("controls.nvidiaHint")}</span>
          </div>
          {!system.cudaInstallable && (
            <small className="model-download-block-reason">
              {t("settings.cudaDriverIncompatible", {
                driver: system.nvidiaDriverVersion ?? t("hardware.notDetected"),
              })}
            </small>
          )}
          <button
            type="button"
            className="download-button"
            onClick={onInstallCuda}
            disabled={!system.cudaInstallable || installingCuda || installingAccelerator !== null || modelDownloadActive || controlsDisabled}
          >
            {installingCuda ? <LoaderCircle className="spin" size={17} /> : <Download size={17} />}
            {installingCuda
              ? (
                <span className="download-progress-copy">
                  <span>{t("controls.installingCuda", { percent: Math.round(cudaDownloadPercent) })}</span>
                  {networkSpeedBytesPerSecond !== null && <small>{formatBytes(networkSpeedBytesPerSecond)}/s</small>}
                </span>
              )
              : t("controls.installCuda")}
          </button>
          {installingCuda && (
            <button type="button" className="danger-ghost" onClick={onCancelCuda}>
              <CircleStop size={16} /> {t("controls.cancelCuda")}
            </button>
          )}
        </>
      )}

      {accelerators
        .filter((accelerator) => accelerator.supported && accelerator.downloadable && !accelerator.installed)
        .map((accelerator) => {
          const memoryBlocked = accelerator.backend === "vulkan" && !accelerator.memorySufficient;
          const compatibilityUnknown = accelerator.backend === "vulkan" && !accelerator.preflightChecked;
          return (
          <div key={accelerator.id}>
            <div className="info-box">
              <Download size={16} />
              <span>{t("controls.acceleratorInfo", { description: getAcceleratorCopy(accelerator, t).description })}</span>
            </div>
            <button
              type="button"
              className="download-button"
              onClick={() =>
                onInstallAccelerator(
                  accelerator.backend as Exclude<BackendChoice, "auto" | "cpu" | "cuda">
                )
              }
              disabled={memoryBlocked || installingAccelerator !== null || installingCuda || modelDownloadActive || controlsDisabled}
            >
              {installingAccelerator === accelerator.backend ? <LoaderCircle className="spin" size={17} /> : <Download size={17} />}
              {installingAccelerator === accelerator.backend
                ? (
                  <span className="download-progress-copy">
                    <span>{t("controls.installingAccelerator", {
                      accelerator: getAcceleratorCopy(accelerator, t).label,
                      percent: Math.round(acceleratorDownloadPercent),
                    })}</span>
                    {networkSpeedBytesPerSecond !== null && <small>{formatBytes(networkSpeedBytesPerSecond)}/s</small>}
                  </span>
                )
                : t("controls.installAccelerator", {
                    accelerator: getAcceleratorCopy(accelerator, t).label,
                  })}
            </button>
            {memoryBlocked && (
              <small className="model-download-block-reason">
                {t("settings.acceleratorRamTooLow", {
                  available: formatMemory(accelerator.memoryAvailableMb, t("hardware.notDetected")),
                  required: formatMemory(accelerator.memoryRequiredMb, t("hardware.notDetected")),
                })}
              </small>
            )}
            {compatibilityUnknown && (
              <small className="settings-note">
                {t("settings.acceleratorCompatibilityUnknown")}
              </small>
            )}
            {installingAccelerator === accelerator.backend && (
              <button type="button" className="danger-ghost" onClick={onCancelAccelerator}>
                <CircleStop size={16} /> {t("controls.cancelAccelerator")}
              </button>
            )}
          </div>
          );
        })}

      <div className="divider" />

      <label className="field-label">{t("controls.language")}</label>
      <CustomSelect
        value={language}
        options={languageOptions}
        onChange={onLanguageChange}
        disabled={controlsDisabled}
        ariaLabel={t("controls.language")}
      />

      <label className="field-label">{t("controls.computeBackend")}</label>
      <CustomSelect
        value={computeTargetId}
        options={computeTargetOptions}
        onChange={onComputeTargetChange}
        disabled={controlsDisabled}
        ariaLabel={t("controls.computeBackend")}
      />
      <div className="recommendation">
        <Sparkles size={14} /> {getRecommendation(system, t)}
      </div>

      <label className="toggle-row" htmlFor="keep-audio-toggle">
        <div>
          <strong>{t("controls.keepAudio")}</strong>
          <span>{t("controls.keepAudioHint")}</span>
        </div>
        <input
          id="keep-audio-toggle"
          type="checkbox"
          checked={keepAudio}
          onChange={(event) => onKeepAudioChange(event.target.checked)}
          disabled={controlsDisabled}
        />
      </label>

      {vramWarning && (
        <p className="setup-hint">
          <AlertCircle size={14} /> {vramWarning}
        </p>
      )}
      {acceleratorWarning && (
        <p className="setup-hint">
          <AlertCircle size={14} /> {acceleratorWarning}
        </p>
      )}

      <button type="button" className="start-button" disabled={!canStart} onClick={onStart}>
        {checkingResume ? <LoaderCircle className="spin" size={18} /> : <AudioWaveform size={18} />}
        {t(checkingResume ? "controls.checkingResume" : resumeStage ? "controls.resume" : retryAvailable ? "controls.retry" : "controls.transcribe")}
      </button>
      {resumeStage && <p className="setup-hint" role="status">
        {t(resumeStage === "audio" ? "controls.resumeAudioHint" : "controls.resumeDownloadHint")}
      </p>}
      {!resumeStage && !checkingResume && retryAvailable && <p className="setup-hint" role="status">
        {t("controls.retryHint")}
      </p>}

      {!runtimeReady && system && (
        <p className="setup-hint">
          <AlertCircle size={14} /> {t("controls.runtimeHint")}
        </p>
      )}
    </div>
  );
}
