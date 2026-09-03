import { AlertCircle, Check, CircleStop, Download, Gauge, LoaderCircle, Sparkles } from "lucide-react";
import { formatMemory } from "../../lib/format";
import type { BackendChoice, ModelInfo, SystemStatus } from "../../types";

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
  installingCuda: boolean;
  cudaDownloadPercent: number;
  vramWarning: string | null;
  onInstallCuda: () => void;
  onCancelCuda: () => void;
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
  installingCuda,
  cudaDownloadPercent,
  vramWarning,
  onInstallCuda,
  onCancelCuda,
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
          <button className="download-button" onClick={onInstallCuda} disabled={installingCuda || busy}>
            {installingCuda ? <LoaderCircle className="spin" size={17} /> : <Download size={17} />}
            {installingCuda ? `Installing CUDA ${Math.round(cudaDownloadPercent)}%` : "Install CUDA acceleration (~437 MB)"}
          </button>
          {installingCuda && <button className="danger-ghost" onClick={onCancelCuda}><CircleStop size={16} /> Cancel CUDA download</button>}
        </>
      )}

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
      </select>
      <div className="recommendation"><Sparkles size={14} /> {system?.recommendation ?? "Mendeteksi hardware…"}</div>

      <label className="toggle-row">
        <div><strong>Keep processed audio</strong><span>Default off untuk hemat storage.</span></div>
        <input type="checkbox" checked={keepAudio} onChange={(event) => onKeepAudioChange(event.target.checked)} disabled={busy} />
      </label>

      {vramWarning && <p className="setup-hint"><AlertCircle size={14} /> {vramWarning}</p>}
      <button className="start-button" disabled={!canStart} onClick={onStart}>
        <Sparkles size={18} /> Transcribe now
      </button>
      {!runtimeReady && <p className="setup-hint"><AlertCircle size={14} /> Runtime belum siap. Jalankan setup Windows.</p>}
    </div>
  );
}
