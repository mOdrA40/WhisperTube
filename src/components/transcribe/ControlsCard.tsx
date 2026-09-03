import { AlertCircle, Check, CircleStop, Download, Gauge, LoaderCircle, Sparkles } from "lucide-react";
import { formatMemory } from "../../lib/format";
import type { AcceleratorInfo, BackendChoice, ModelInfo, SystemStatus } from "../../types";

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

  return (
    <div className="card control-card sticky-card">
      <div className="card-title-row"><div><span className="eyebrow">Transcription</span><h3>Quality & compute</h3></div><Gauge size={20} /></div>

      <label className="field-label">Model</label>
      <div className="model-options">
        {models.map((model) => (
          <button key={model.id} className={modelId === model.id ? "model-option selected" : "model-option"} onClick={() => onModelChange(model.id)} disabled={busy}>
            <div className="radio-dot"><span /></div>
            <div className="model-copy"><strong>{model.label}</strong><span>{model.description} • CUDA ≥ {formatMemory(model.vramRequiredMb)}</span></div>
            <div className="model-state">{model.installed ? <Check size={15} /> : `${model.sizeMb} MB`}</div>
          </button>
        ))}
      </div>

      {selectedModel && !selectedModel.installed && (
        <button className="download-button" onClick={() => onDownloadModel(selectedModel.id)} disabled={selectedDownload !== undefined}>
          {selectedDownload !== undefined ? <LoaderCircle className="spin" size={17} /> : <Download size={17} />}
          {selectedDownload !== undefined ? `Downloading ${Math.round(selectedDownload)}%` : `Download ${selectedModel.label}`}
        </button>
      )}

      {system?.nvidia && !system.cudaEngine && (
        <>
          <div className="info-box"><Sparkles size={16} /><span>NVIDIA terdeteksi. Pasang CUDA agar transkripsi memakai GPU, bukan CPU.</span></div>
          <button className="download-button" onClick={onInstallCuda} disabled={installingCuda || installingAccelerator !== null || busy}>
            {installingCuda ? <LoaderCircle className="spin" size={17} /> : <Download size={17} />}
            {installingCuda ? `Installing CUDA ${Math.round(cudaDownloadPercent)}%` : "Install CUDA acceleration (~437 MB)"}
          </button>
          {installingCuda && <button className="danger-ghost" onClick={onCancelCuda}><CircleStop size={16} /> Cancel CUDA download</button>}
        </>
      )}

      {accelerators.filter((accelerator) => accelerator.supported && accelerator.downloadable && !accelerator.installed).map((accelerator) => (
        <div key={accelerator.id}>
          <div className="info-box"><Download size={16} /><span>{accelerator.description}. Download dari accelerator release project.</span></div>
          <button className="download-button" onClick={() => onInstallAccelerator(accelerator.backend as Exclude<BackendChoice, "auto" | "cpu" | "cuda">)} disabled={installingAccelerator !== null || installingCuda || busy}>
            {installingAccelerator === accelerator.backend ? <LoaderCircle className="spin" size={17} /> : <Download size={17} />}
            {installingAccelerator === accelerator.backend ? `Installing ${accelerator.label} ${Math.round(acceleratorDownloadPercent)}%` : `Install ${accelerator.label}`}
          </button>
          {installingAccelerator === accelerator.backend && <button className="danger-ghost" onClick={onCancelAccelerator}><CircleStop size={16} /> Cancel accelerator download</button>}
        </div>
      ))}

      <div className="divider" />
      <label className="field-label">Language</label>
      <select value={language} onChange={(event) => onLanguageChange(event.target.value)} disabled={busy}>
        <option value="auto">Auto detect</option>
        <option value="id">Indonesian</option>
        <option value="en">English</option>
        <option value="zh">Chinese</option>
        <option value="ja">Japanese</option>
        <option value="ko">Korean</option>
      </select>

      <label className="field-label">Compute backend</label>
      <select value={backend} onChange={(event) => onBackendChange(event.target.value as BackendChoice)} disabled={busy}>
        <option value="auto">Auto — recommended</option>
        <option value="cpu">CPU</option>
        <option value="cuda" disabled={!system?.cudaEngine || !system?.nvidia}>NVIDIA CUDA{system?.cudaEngine && system?.nvidia ? "" : " — not available"}</option>
        {accelerators.map((accelerator) => (
          <option value={accelerator.backend} disabled={!accelerator.installed} key={accelerator.id}>{accelerator.label}{accelerator.installed ? "" : " — not installed"}</option>
        ))}
      </select>
      <div className="recommendation"><Sparkles size={14} /> {system?.recommendation ?? "Mendeteksi hardware…"}</div>

      <label className="toggle-row">
        <div><strong>Keep processed audio</strong><span>Default off untuk hemat storage.</span></div>
        <input type="checkbox" checked={keepAudio} onChange={(event) => onKeepAudioChange(event.target.checked)} disabled={busy} />
      </label>

      {vramWarning && <p className="setup-hint"><AlertCircle size={14} /> {vramWarning}</p>}
      {acceleratorWarning && <p className="setup-hint"><AlertCircle size={14} /> {acceleratorWarning}</p>}
      <button className="start-button" disabled={!canStart} onClick={onStart}>
        <Sparkles size={18} /> Transcribe now
      </button>
      {!runtimeReady && <p className="setup-hint"><AlertCircle size={14} /> Runtime belum siap. Jalankan setup Windows.</p>}
    </div>
  );
}
