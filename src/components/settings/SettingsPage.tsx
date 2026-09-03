import { CircleStop, Cpu, Download, LockKeyhole, LoaderCircle, MonitorCog, RotateCcw, Trash2 } from "lucide-react";
import { formatMemory } from "../../lib/format";
import type { AcceleratorInfo, BackendChoice, BrowserChoice, BrowserInfo, ModelInfo, SystemStatus } from "../../types";

type SettingsPageProps = {
  browser: BrowserChoice;
  browsers: BrowserInfo[];
  browserProfile: string;
  system: SystemStatus | null;
  models: ModelInfo[];
  busy: boolean;
  downloadingModel: Record<string, number>;
  accelerators: AcceleratorInfo[];
  installingCuda: boolean;
  cudaDownloadPercent: number;
  installingAccelerator: Exclude<BackendChoice, "auto" | "cpu" | "cuda"> | null;
  acceleratorDownloadPercent: number;
  onBrowserChange: (browser: BrowserChoice) => void;
  onBrowserProfileChange: (profile: string) => void;
  onDownloadModel: (id: string) => void;
  onRemoveModel: (id: string) => void;
  onRefresh: () => void;
  onInstallCuda: () => void;
  onCancelCuda: () => void;
  onInstallAccelerator: (backend: Exclude<BackendChoice, "auto" | "cpu" | "cuda">) => void;
  onCancelAccelerator: () => void;
};

export function SettingsPage({
  browser,
  browsers,
  browserProfile,
  system,
  models,
  busy,
  downloadingModel,
  accelerators,
  installingCuda,
  cudaDownloadPercent,
  installingAccelerator,
  acceleratorDownloadPercent,
  onBrowserChange,
  onBrowserProfileChange,
  onDownloadModel,
  onRemoveModel,
  onRefresh,
  onInstallCuda,
  onCancelCuda,
  onInstallAccelerator,
  onCancelAccelerator,
}: SettingsPageProps) {
  return (
    <div className="settings-grid">
      <section className="card settings-card">
        <div className="card-title-row"><div><span className="eyebrow">Authentication</span><h3>YouTube access</h3></div><LockKeyhole size={20} /></div>
        <p className="settings-intro">WhisperTube tidak meminta email/password Google. Untuk Member-only, yt-dlp membaca session browser lokal yang sudah login.</p>
        <label className="field-label">Browser session</label>
        <select value={browser} onChange={(event) => onBrowserChange(event.target.value as BrowserChoice)} disabled={busy}>
          <option value="none">Public videos only</option>
          {browsers.map((item) => <option value={item.id} key={item.id}>{item.label}</option>)}
        </select>
        {browser !== "none" && browsers.find((item) => item.id === browser) && (
          <>
            <label className="field-label">Browser profile</label>
            <select value={browserProfile} onChange={(event) => onBrowserProfileChange(event.target.value)} disabled={busy}>
              {browsers.find((item) => item.id === browser)?.profiles.map((profile) => (
                <option value={profile.id} key={`${browser}-${profile.id}`}>{profile.label}{profile.isDefault ? " (Default)" : ""}</option>
              ))}
            </select>
            <p className="settings-note">Profile terdeteksi dari konfigurasi lokal. Status login diverifikasi saat Check video.</p>
          </>
        )}
        {browser === "none" && browsers.length === 0 && <p className="settings-note">Tidak ada browser session yang terdeteksi. Public videos tetap bisa diproses tanpa cookies.</p>}
        <div className="info-box"><LockKeyhole size={16} /><span>Cookies tidak disimpan oleh aplikasi. yt-dlp membacanya saat proses berjalan.</span></div>
      </section>

      <section className="card settings-card">
        <div className="card-title-row"><div><span className="eyebrow">Hardware</span><h3>Compute engine</h3></div><Cpu size={20} /></div>
        <div className="status-table">
          <div><span>CPU engine</span><strong className={system?.cpuEngine ? "ok-text" : "bad-text"}>{system?.cpuEngine ? "Installed" : "Missing"}</strong></div>
          <div><span>NVIDIA GPU</span><strong>{system?.gpuName ?? "Not detected"}</strong></div>
          <div><span>Total VRAM</span><strong>{formatMemory(system?.gpuMemoryMb)}</strong></div>
          <div><span>Free VRAM</span><strong>{formatMemory(system?.gpuFreeMemoryMb)}</strong></div>
          <div><span>CUDA engine</span><strong className={system?.cudaEngine ? "ok-text" : "muted-text"}>{system?.cudaEngine ? "Installed" : "Optional"}</strong></div>
          <div><span>CPU threads</span><strong>{system?.cpuThreads ?? "—"}</strong></div>
        </div>
        {system?.nvidia ? (
          <>
            <div className="info-box"><Download size={16} /><span>CUDA engine diunduh dari release resmi whisper.cpp dan dipasang ke storage aplikasi (~437 MB).</span></div>
            <button className="secondary-button full" onClick={onInstallCuda} disabled={installingCuda || installingAccelerator !== null || system.cudaEngine || busy}>
              {installingCuda ? <LoaderCircle className="spin" size={16} /> : <Download size={16} />}
              {installingCuda ? `Installing CUDA ${Math.round(cudaDownloadPercent)}%` : system.cudaEngine ? "CUDA engine installed" : "Install CUDA acceleration"}
            </button>
            {installingCuda && <button className="danger-ghost" onClick={onCancelCuda}><CircleStop size={16} /> Cancel CUDA download</button>}
          </>
        ) : (
          <p className="settings-note">NVIDIA GPU tidak terdeteksi. CUDA acceleration hanya tersedia jika driver NVIDIA aktif.</p>
        )}
        {accelerators.map((accelerator) => (
          <div className="info-box" key={accelerator.id}>
            <Download size={16} />
            <span>{accelerator.label}: {accelerator.installed ? "Installed" : accelerator.description}</span>
            {!accelerator.installed && <button className="secondary-button compact" onClick={() => onInstallAccelerator(accelerator.backend as Exclude<BackendChoice, "auto" | "cpu" | "cuda">)} disabled={installingAccelerator !== null || installingCuda || busy}>
              {installingAccelerator === accelerator.backend ? `${Math.round(acceleratorDownloadPercent)}%` : "Install"}
            </button>}
          </div>
        ))}
        {installingAccelerator && <button className="danger-ghost" onClick={onCancelAccelerator}><CircleStop size={16} /> Cancel accelerator download</button>}
      </section>

      <section className="card settings-card models-settings">
        <div className="card-title-row"><div><span className="eyebrow">Storage</span><h3>Whisper models</h3></div><Download size={20} /></div>
        <div className="model-manager">
          {models.map((model) => (
            <div className="model-manage-row" key={model.id}>
              <div><strong>{model.label}</strong><span>{model.description} • {model.sizeMb} MB • CUDA ≥ {formatMemory(model.vramRequiredMb)}</span></div>
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
