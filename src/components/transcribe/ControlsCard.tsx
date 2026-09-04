import {
  AlertCircle,
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
import type { AcceleratorInfo, BackendChoice, ModelDownloadPayload, ModelInfo, SystemStatus } from "../../types";
import { getAcceleratorCopy, getModelCopy, getRecommendation, useI18n } from "../../i18n";
import { CustomSelect, type SelectOption } from "../common/CustomSelect";

type ControlsCardProps = {
  models: ModelInfo[];
  modelId: string;
  selectedModel: ModelInfo | undefined;
  canStart: boolean;
  backend: BackendChoice;
  language: string;
  keepAudio: boolean;
  system: SystemStatus | null;
  busy: boolean;
  runtimeReady: boolean;
  downloadingModel: Record<string, ModelDownloadPayload>;
  accelerators: AcceleratorInfo[];
  installingCuda: boolean;
  cudaDownloadPercent: number;
  installingAccelerator: Exclude<BackendChoice, "auto" | "cpu" | "cuda"> | null;
  acceleratorDownloadPercent: number;
  vramWarning: string | null;
  acceleratorWarning: string | null;
  onInstallCuda: () => void;
  onCancelCuda: () => void;
  onCancelModel: () => void;
  onInstallAccelerator: (backend: Exclude<BackendChoice, "auto" | "cpu" | "cuda">) => void;
  onCancelAccelerator: () => void;
  onModelChange: (id: string) => void;
  onBackendChange: (backend: BackendChoice) => void;
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
  backend,
  language,
  keepAudio,
  system,
  busy,
  runtimeReady,
  downloadingModel,
  accelerators,
  installingCuda,
  cudaDownloadPercent,
  installingAccelerator,
  acceleratorDownloadPercent,
  vramWarning,
  acceleratorWarning,
  onInstallCuda,
  onCancelCuda,
  onCancelModel,
  onInstallAccelerator,
  onCancelAccelerator,
  onModelChange,
  onBackendChange,
  onLanguageChange,
  onKeepAudioChange,
  onDownloadModel,
  onStart,
}: ControlsCardProps) {
  const { t } = useI18n();
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

  const backendOptions: SelectOption[] = [
    {
      value: "auto",
      label: t("backend.auto"),
    },
    {
      value: "cpu",
      label: t("backend.cpu"),
    },
    ...accelerators.filter((acc) => acc.supported).map((acc) => ({
      value: acc.backend,
      label: acc.installed
        ? getAcceleratorCopy(acc, t).label
        : t("backend.notInstalled", { accelerator: getAcceleratorCopy(acc, t).label }),
      disabled: !acc.installed,
    })),
  ];
  if (system?.cudaSupported) {
    backendOptions.push({
      value: "cuda",
      label: system.cudaEngine && system.nvidia ? t("backend.cuda") : t("backend.cudaUnavailable"),
      disabled: !system.cudaEngine || !system.nvidia,
    });
  }

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
      <div className="model-options">
        {models.map((model) => {
          const isSelected = modelId === model.id;

          return (
            <button
              key={model.id}
              type="button"
              className={isSelected ? "model-option selected" : "model-option"}
              onClick={() => onModelChange(model.id)}
              disabled={busy || modelDownloadActive}
            >
              <div className="radio-dot">
                <span />
              </div>
              <div className="model-copy">
                <strong>{getModelCopy(model, t).label}</strong>
                <span>
                  {getModelCopy(model, t).description} • {t("controls.cudaRequirement", { memory: formatMemory(model.vramRequiredMb, t("hardware.notDetected")) })}
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
          disabled={selectedDownload !== undefined || modelDownloadActive || busy}
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
          <button
            type="button"
            className="download-button"
            onClick={onInstallCuda}
            disabled={installingCuda || installingAccelerator !== null || modelDownloadActive || busy}
          >
            {installingCuda ? <LoaderCircle className="spin" size={17} /> : <Download size={17} />}
            {installingCuda
              ? t("controls.installingCuda", { percent: Math.round(cudaDownloadPercent) })
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
        .map((accelerator) => (
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
              disabled={installingAccelerator !== null || installingCuda || modelDownloadActive || busy}
            >
              {installingAccelerator === accelerator.backend ? <LoaderCircle className="spin" size={17} /> : <Download size={17} />}
              {installingAccelerator === accelerator.backend
                ? t("controls.installingAccelerator", {
                    accelerator: getAcceleratorCopy(accelerator, t).label,
                    percent: Math.round(acceleratorDownloadPercent),
                  })
                : t("controls.installAccelerator", {
                    accelerator: getAcceleratorCopy(accelerator, t).label,
                  })}
            </button>
            {installingAccelerator === accelerator.backend && (
              <button type="button" className="danger-ghost" onClick={onCancelAccelerator}>
                <CircleStop size={16} /> {t("controls.cancelAccelerator")}
              </button>
            )}
          </div>
        ))}

      <div className="divider" />

      <label className="field-label">{t("controls.language")}</label>
      <CustomSelect
        value={language}
        options={languageOptions}
        onChange={onLanguageChange}
        disabled={busy}
      />

      <label className="field-label">{t("controls.computeBackend")}</label>
      <CustomSelect
        value={backend}
        options={backendOptions}
        onChange={(val) => onBackendChange(val as BackendChoice)}
        disabled={busy}
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
          disabled={busy}
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
        <Sparkles size={18} /> {t("controls.transcribe")}
      </button>

      {!runtimeReady && (
        <p className="setup-hint">
          <AlertCircle size={14} /> {t("controls.runtimeHint")}
        </p>
      )}
    </div>
  );
}
