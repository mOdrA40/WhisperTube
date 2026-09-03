import { Cpu, Download, LockKeyhole, LoaderCircle, MonitorCog, RotateCcw, Trash2 } from "lucide-react";
import type { BrowserChoice, ModelInfo, SystemStatus } from "../../types";

type SettingsPageProps = {
  browser: BrowserChoice;
  system: SystemStatus | null;
  models: ModelInfo[];
  busy: boolean;
  downloadingModel: Record<string, number>;
  onBrowserChange: (browser: BrowserChoice) => void;
  onDownloadModel: (id: string) => void;
  onRemoveModel: (id: string) => void;
  onRefresh: () => void;
};

export function SettingsPage({
  browser,
  system,
  models,
  busy,
  downloadingModel,
  onBrowserChange,
  onDownloadModel,
  onRemoveModel,
  onRefresh,
}: SettingsPageProps) {
  return (
    <div className="settings-grid">
      <section className="card settings-card">
        <div className="card-title-row"><div><span className="eyebrow">Authentication</span><h3>YouTube access</h3></div><LockKeyhole size={20} /></div>
        <p className="settings-intro">WhisperTube tidak meminta email/password Google. Untuk Member-only, yt-dlp membaca session browser lokal yang sudah login.</p>
        <label className="field-label">Browser session</label>
        <select value={browser} onChange={(event) => onBrowserChange(event.target.value as BrowserChoice)} disabled={busy}>
          <option value="none">Public videos only</option>
          <option value="chrome">Google Chrome</option>
          <option value="edge">Microsoft Edge</option>
          <option value="firefox">Mozilla Firefox</option>
          <option value="brave">Brave</option>
        </select>
        <div className="info-box"><LockKeyhole size={16} /><span>Cookies tidak disimpan oleh aplikasi. yt-dlp membacanya saat proses berjalan.</span></div>
      </section>

      <section className="card settings-card">
        <div className="card-title-row"><div><span className="eyebrow">Hardware</span><h3>Compute engine</h3></div><Cpu size={20} /></div>
        <div className="status-table">
          <div><span>CPU engine</span><strong className={system?.cpuEngine ? "ok-text" : "bad-text"}>{system?.cpuEngine ? "Installed" : "Missing"}</strong></div>
          <div><span>NVIDIA GPU</span><strong>{system?.gpuName ?? "Not detected"}</strong></div>
          <div><span>CUDA engine</span><strong className={system?.cudaEngine ? "ok-text" : "muted-text"}>{system?.cudaEngine ? "Installed" : "Optional"}</strong></div>
          <div><span>CPU threads</span><strong>{system?.cpuThreads ?? "—"}</strong></div>
        </div>
        <p className="settings-note">Untuk CUDA, jalankan <code>scripts/install-cuda-engine.ps1</code>, lalu restart aplikasi.</p>
      </section>

      <section className="card settings-card models-settings">
        <div className="card-title-row"><div><span className="eyebrow">Storage</span><h3>Whisper models</h3></div><Download size={20} /></div>
        <div className="model-manager">
          {models.map((model) => (
            <div className="model-manage-row" key={model.id}>
              <div><strong>{model.label}</strong><span>{model.description} • {model.sizeMb} MB</span></div>
              {model.installed ? (
                <button className="icon-button danger" onClick={() => onRemoveModel(model.id)} title="Delete model"><Trash2 size={17} /></button>
              ) : (
                <button className="secondary-button compact" onClick={() => onDownloadModel(model.id)} disabled={downloadingModel[model.id] !== undefined}>
                  {downloadingModel[model.id] !== undefined ? <LoaderCircle className="spin" size={15} /> : <Download size={15} />}
                  {downloadingModel[model.id] !== undefined ? `${Math.round(downloadingModel[model.id])}%` : "Download"}
                </button>
              )}
            </div>
          ))}
        </div>
      </section>

      <section className="card settings-card">
        <div className="card-title-row"><div><span className="eyebrow">Runtime</span><h3>External components</h3></div><MonitorCog size={20} /></div>
        <div className="status-table">
          <div><span>yt-dlp</span><strong className={system?.ytDlp ? "ok-text" : "bad-text"}>{system?.ytDlp ? "Ready" : "Missing"}</strong></div>
          <div><span>FFmpeg</span><strong className={system?.ffmpeg ? "ok-text" : "bad-text"}>{system?.ffmpeg ? "Ready" : "Missing"}</strong></div>
          <div><span>whisper.cpp CPU</span><strong className={system?.cpuEngine ? "ok-text" : "bad-text"}>{system?.cpuEngine ? "Ready" : "Missing"}</strong></div>
        </div>
        <button className="secondary-button full" onClick={onRefresh}><RotateCcw size={16} /> Re-check components</button>
      </section>
    </div>
  );
}
