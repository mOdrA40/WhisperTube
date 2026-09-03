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
import { formatMemory } from "../../lib/format";
import type { AcceleratorInfo, BackendChoice, ModelInfo, SystemStatus } from "../../types";
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
  downloadingModel: Record<string, number>;
  accelerators: AcceleratorInfo[];
  installingCuda: boolean;
  cudaDownloadPercent: number;
  installingAccelerator: Exclude<BackendChoice, "auto" | "cpu" | "cuda"> | null;
  acceleratorDownloadPercent: number;
  vramWarning: string | null;
  acceleratorWarning: string | null;
  onInstallCuda: () => void;
  onCancelCuda: () => void;
  onInstallAccelerator: (backend: Exclude<BackendChoice, "auto" | "cpu" | "cuda">) => void;
  onCancelAccelerator: () => void;
  onModelChange: (id: string) => void;
  onBackendChange: (backend: BackendChoice) => void;
  onLanguageChange: (language: string) => void;
  onKeepAudioChange: (keepAudio: boolean) => void;
  onDownloadModel: (id: string) => void;
  onStart: () => void;
};

const languageOptions: SelectOption[] = [
  { value: "auto", label: "Auto detect" },
  { value: "id", label: "Indonesian" },
  { value: "en", label: "English" },
  { value: "zh", label: "Chinese" },
  { value: "ja", label: "Japanese" },
  { value: "ko", label: "Korean" },
];

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
  onInstallAccelerator,
  onCancelAccelerator,
  onModelChange,
  onBackendChange,
  onLanguageChange,
  onKeepAudioChange,
  onDownloadModel,
  onStart,
}: ControlsCardProps) {
  const selectedDownload = selectedModel && downloadingModel[selectedModel.id];

  const backendOptions: SelectOption[] = [
    {
      value: "auto",
      label: "Auto — recommended",
    },
    {
      value: "cpu",
      label: "CPU",
    },
    {
      value: "cuda",
      label: system?.cudaEngine && system?.nvidia ? "NVIDIA CUDA" : "NVIDIA CUDA — not available",
      disabled: !system?.cudaEngine || !system?.nvidia,
    },
    ...accelerators.map((acc) => ({
      value: acc.backend,
      label: acc.installed ? acc.label : `${acc.label} — not installed`,
      disabled: !acc.installed,
    })),
  ];

  return (
    <div className="card control-card sticky-card">
      <div className="card-title-row">
        <div>
          <span className="eyebrow">Transcription</span>
          <h3>Quality & compute</h3>
        </div>
        <Gauge size={20} />
      </div>

      <label className="field-label">Model</label>
      <div className="model-options">
        {models.map((model) => {
          const isSelected = modelId === model.id;

          return (
            <button
              key={model.id}
              type="button"
              className={isSelected ? "model-option selected" : "model-option"}
              onClick={() => onModelChange(model.id)}
              disabled={busy}
            >
              <div className="radio-dot">
                <span />
              </div>
              <div className="model-copy">
                <strong>{model.label}</strong>
                <span>
                  {model.description} • CUDA ≥ {formatMemory(model.vramRequiredMb)}
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
          disabled={selectedDownload !== undefined}
        >
          {selectedDownload !== undefined ? <LoaderCircle className="spin" size={17} /> : <Download size={17} />}
          {selectedDownload !== undefined
            ? `Downloading ${Math.round(selectedDownload)}%`
            : `Download ${selectedModel.label}`}
        </button>
      )}

      {system?.nvidia && !system.cudaEngine && (
        <>
          <div className="info-box">
            <Sparkles size={16} />
            <span>NVIDIA terdeteksi. Pasang CUDA agar transkripsi memakai GPU, bukan CPU.</span>
          </div>
          <button
            type="button"
            className="download-button"
            onClick={onInstallCuda}
            disabled={installingCuda || installingAccelerator !== null || busy}
          >
            {installingCuda ? <LoaderCircle className="spin" size={17} /> : <Download size={17} />}
            {installingCuda ? `Installing CUDA ${Math.round(cudaDownloadPercent)}%` : "Install CUDA acceleration (~437 MB)"}
          </button>
          {installingCuda && (
            <button type="button" className="danger-ghost" onClick={onCancelCuda}>
              <CircleStop size={16} /> Cancel CUDA download
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
              <span>{accelerator.description}. Download dari accelerator release project.</span>
            </div>
            <button
              type="button"
              className="download-button"
              onClick={() =>
                onInstallAccelerator(
                  accelerator.backend as Exclude<BackendChoice, "auto" | "cpu" | "cuda">
                )
              }
              disabled={installingAccelerator !== null || installingCuda || busy}
            >
              {installingAccelerator === accelerator.backend ? <LoaderCircle className="spin" size={17} /> : <Download size={17} />}
              {installingAccelerator === accelerator.backend
                ? `Installing ${accelerator.label} ${Math.round(acceleratorDownloadPercent)}%`
                : `Install ${accelerator.label}`}
            </button>
            {installingAccelerator === accelerator.backend && (
              <button type="button" className="danger-ghost" onClick={onCancelAccelerator}>
                <CircleStop size={16} /> Cancel accelerator download
              </button>
            )}
          </div>
        ))}

      <div className="divider" />

      <label className="field-label">Language</label>
      <CustomSelect
        value={language}
        options={languageOptions}
        onChange={onLanguageChange}
        disabled={busy}
      />

      <label className="field-label">Compute backend</label>
      <CustomSelect
        value={backend}
        options={backendOptions}
        onChange={(val) => onBackendChange(val as BackendChoice)}
        disabled={busy}
      />
      <div className="recommendation">
        <Sparkles size={14} /> {system?.recommendation ?? "Mendeteksi hardware…"}
      </div>

      <label className="toggle-row" htmlFor="keep-audio-toggle">
        <div>
          <strong>Keep processed audio</strong>
          <span>Default off untuk hemat storage.</span>
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
        <Sparkles size={18} /> Transcribe now
      </button>

      {!runtimeReady && (
        <p className="setup-hint">
          <AlertCircle size={14} /> Runtime belum siap. Jalankan setup Windows.
        </p>
      )}
    </div>
  );
}
