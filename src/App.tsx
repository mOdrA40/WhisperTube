import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { save } from "@tauri-apps/plugin-dialog";
import {
  AlertCircle,
  Check,
  ChevronRight,
  CircleStop,
  Clock3,
  Copy,
  Cpu,
  Download,
  ExternalLink,
  FileText,
  Gauge,
  History,
  LoaderCircle,
  LockKeyhole,
  MonitorCog,
  Play,
  RotateCcw,
  Search,
  Settings2,
  Sparkles,
  Trash2,
  Youtube,
  Zap,
} from "lucide-react";

type BrowserChoice = "none" | "chrome" | "edge" | "firefox" | "brave";
type BackendChoice = "auto" | "cpu" | "cuda";

type SystemStatus = {
  ytDlp: boolean;
  ffmpeg: boolean;
  cpuEngine: boolean;
  cudaEngine: boolean;
  nvidia: boolean;
  gpuName: string | null;
  cpuThreads: number;
  recommendation: string;
  recommendedModelId: string;
  recommendedBackend: BackendChoice;
};

type ModelInfo = {
  id: string;
  label: string;
  description: string;
  sizeMb: number;
  installed: boolean;
};

type VideoMetadata = {
  id: string;
  title: string;
  channel: string;
  duration: number;
  thumbnail: string | null;
  webpageUrl: string;
  availability: string | null;
};

type ProgressPayload = {
  stage: "idle" | "downloading" | "converting" | "transcribing" | "finalizing" | "done" | "error";
  percent: number;
  message: string;
};

type ModelDownloadPayload = {
  id: string;
  percent: number;
};

type Segment = {
  from: string;
  to: string;
  text: string;
};

type TranscriptResult = {
  historyId: number;
  title: string;
  channel: string;
  language: string;
  duration: number;
  model: string;
  backend: string;
  segments: Segment[];
  text: string;
  txtPath: string;
  srtPath: string;
  vttPath: string;
};

type HistoryItem = {
  id: number;
  title: string;
  channel: string;
  sourceUrl: string;
  createdAt: string;
  duration: number;
  language: string;
  model: string;
  backend: string;
};

const stageLabels: Record<ProgressPayload["stage"], string> = {
  idle: "Siap",
  downloading: "Mengunduh audio",
  converting: "Menyiapkan audio",
  transcribing: "Mentranskripsi",
  finalizing: "Menyimpan hasil",
  done: "Selesai",
  error: "Gagal",
};

function formatDuration(totalSeconds: number) {
  if (!Number.isFinite(totalSeconds) || totalSeconds <= 0) return "—";
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = Math.floor(totalSeconds % 60);
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`
    : `${minutes}:${String(seconds).padStart(2, "0")}`;
}

function friendlyError(error: unknown) {
  const raw = String(error ?? "Terjadi kesalahan yang tidak diketahui");
  return raw.replace(/^Error:\s*/i, "");
}

export default function App() {
  const [tab, setTab] = useState<"transcribe" | "history" | "settings">("transcribe");
  const [url, setUrl] = useState("");
  const [browser, setBrowser] = useState<BrowserChoice>("none");
  const [backend, setBackend] = useState<BackendChoice>("auto");
  const [language, setLanguage] = useState("auto");
  const [modelId, setModelId] = useState("large-v3-turbo-q5_0");
  const [keepAudio, setKeepAudio] = useState(false);
  const [metadata, setMetadata] = useState<VideoMetadata | null>(null);
  const [system, setSystem] = useState<SystemStatus | null>(null);
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [history, setHistory] = useState<HistoryItem[]>([]);
  const [result, setResult] = useState<TranscriptResult | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [progress, setProgress] = useState<ProgressPayload>({ stage: "idle", percent: 0, message: "" });
  const [busy, setBusy] = useState(false);
  const [inspecting, setInspecting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [downloadingModel, setDownloadingModel] = useState<Record<string, number>>({});
  const [copied, setCopied] = useState(false);
  const autoConfigured = useRef(false);

  const selectedModel = models.find((m) => m.id === modelId);
  const runtimeReady = Boolean(system?.ytDlp && system?.ffmpeg && system?.cpuEngine);

  const filteredSegments = useMemo(() => {
    if (!result) return [];
    const q = searchQuery.trim().toLowerCase();
    if (!q) return result.segments;
    return result.segments.filter((segment) => segment.text.toLowerCase().includes(q));
  }, [result, searchQuery]);

  async function refreshSystem() {
    const [nextSystem, nextModels, nextHistory] = await Promise.all([
      invoke<SystemStatus>("system_status"),
      invoke<ModelInfo[]>("list_models"),
      invoke<HistoryItem[]>("list_history"),
    ]);
    setSystem(nextSystem);
    if (!autoConfigured.current) {
      setModelId(nextSystem.recommendedModelId);
      setBackend(nextSystem.recommendedBackend);
      autoConfigured.current = true;
    }
    setModels(nextModels);
    setHistory(nextHistory);
  }

  useEffect(() => {
    refreshSystem().catch((e) => setError(friendlyError(e)));

    const unlistenProgress = listen<ProgressPayload>("job-progress", (event) => {
      setProgress(event.payload);
    });
    const unlistenModel = listen<ModelDownloadPayload>("model-download", (event) => {
      setDownloadingModel((prev) => ({ ...prev, [event.payload.id]: event.payload.percent }));
    });

    return () => {
      unlistenProgress.then((fn) => fn());
      unlistenModel.then((fn) => fn());
    };
  }, []);

  async function inspectVideo() {
    if (!url.trim()) {
      setError("Tempel URL YouTube terlebih dahulu.");
      return;
    }
    setInspecting(true);
    setError(null);
    setResult(null);
    try {
      const data = await invoke<VideoMetadata>("inspect_youtube", {
        url: url.trim(),
        browser,
      });
      setMetadata(data);
    } catch (e) {
      setMetadata(null);
      setError(friendlyError(e));
    } finally {
      setInspecting(false);
    }
  }

  async function downloadModel(id: string) {
    setError(null);
    setDownloadingModel((prev) => ({ ...prev, [id]: 0 }));
    try {
      await invoke("download_model", { modelId: id });
      await refreshSystem();
    } catch (e) {
      setError(friendlyError(e));
    } finally {
      setDownloadingModel((prev) => {
        const copy = { ...prev };
        delete copy[id];
        return copy;
      });
    }
  }

  async function removeModel(id: string) {
    setError(null);
    try {
      await invoke("delete_model", { modelId: id });
      await refreshSystem();
    } catch (e) {
      setError(friendlyError(e));
    }
  }

  async function startTranscription() {
    if (!metadata) {
      setError("Periksa video terlebih dahulu.");
      return;
    }
    if (!selectedModel?.installed) {
      setError("Model yang dipilih belum diunduh.");
      return;
    }
    if (!runtimeReady) {
      setError("Runtime belum lengkap. Jalankan scripts/setup-windows.ps1 terlebih dahulu.");
      return;
    }

    setBusy(true);
    setError(null);
    setResult(null);
    setProgress({ stage: "downloading", percent: 0, message: "Memulai job…" });
    try {
      const data = await invoke<TranscriptResult>("start_transcription", {
        request: {
          url: metadata.webpageUrl,
          title: metadata.title,
          channel: metadata.channel,
          duration: metadata.duration,
          browser,
          backend,
          language,
          modelId,
          keepAudio,
        },
      });
      setResult(data);
      setSearchQuery("");
      await refreshSystem();
    } catch (e) {
      const message = friendlyError(e);
      if (!message.toLowerCase().includes("dibatalkan")) setError(message);
    } finally {
      setBusy(false);
    }
  }

  async function cancelJob() {
    try {
      await invoke("cancel_job");
    } catch (e) {
      setError(friendlyError(e));
    }
  }

  async function loadHistory(id: number) {
    setError(null);
    try {
      const data = await invoke<TranscriptResult>("load_history", { id });
      setResult(data);
      setTab("transcribe");
      setSearchQuery("");
    } catch (e) {
      setError(friendlyError(e));
    }
  }

  async function exportFile(kind: "txt" | "srt" | "vtt") {
    if (!result) return;
    const source = kind === "txt" ? result.txtPath : kind === "srt" ? result.srtPath : result.vttPath;
    const safeTitle = result.title.replace(/[<>:"/\\|?*]+/g, "_").slice(0, 80);
    const target = await save({
      defaultPath: `${safeTitle}.${kind}`,
      filters: [{ name: kind.toUpperCase(), extensions: [kind] }],
    });
    if (!target) return;
    try {
      await invoke("copy_export", { source, target });
    } catch (e) {
      setError(friendlyError(e));
    }
  }

  async function copyTranscript() {
    if (!result) return;
    await navigator.clipboard.writeText(result.text);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1400);
  }

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-mark"><Sparkles size={20} /></div>
          <div>
            <strong>WhisperTube</strong>
            <span>Local AI transcription</span>
          </div>
        </div>

        <nav>
          <button className={tab === "transcribe" ? "nav-item active" : "nav-item"} onClick={() => setTab("transcribe")}>
            <Youtube size={18} /> Transcribe
          </button>
          <button className={tab === "history" ? "nav-item active" : "nav-item"} onClick={() => setTab("history")}>
            <History size={18} /> History
            {history.length > 0 && <span className="nav-badge">{history.length}</span>}
          </button>
          <button className={tab === "settings" ? "nav-item active" : "nav-item"} onClick={() => setTab("settings")}>
            <Settings2 size={18} /> Settings
          </button>
        </nav>

        <div className="sidebar-status">
          <div className="status-title"><MonitorCog size={16} /> Runtime</div>
          <div className={runtimeReady ? "status-pill good" : "status-pill warn"}>
            <span className="dot" /> {runtimeReady ? "Ready" : "Setup needed"}
          </div>
          <div className="hardware-line">
            {system?.nvidia ? <Zap size={14} /> : <Cpu size={14} />}
            <span>{system?.gpuName ?? `${system?.cpuThreads ?? "—"} CPU threads`}</span>
          </div>
        </div>
      </aside>

      <main className="main-area">
        <header className="topbar">
          <div>
            <h1>{tab === "transcribe" ? "Transcribe video" : tab === "history" ? "History" : "Settings"}</h1>
            <p>{tab === "transcribe" ? "YouTube → local Whisper → transcript. Audio tidak dikirim ke cloud." : tab === "history" ? "Buka kembali hasil transkripsi lokal." : "Atur model, autentikasi YouTube, dan compute backend."}</p>
          </div>
          <div className="privacy-badge"><LockKeyhole size={15} /> Local inference</div>
        </header>

        {error && (
          <div className="alert error-alert">
            <AlertCircle size={18} />
            <div><strong>Ada masalah</strong><span>{error}</span></div>
            <button onClick={() => setError(null)}>×</button>
          </div>
        )}

        {tab === "transcribe" && (
          <div className="content-grid">
            <section className="primary-column">
              <div className="hero-card card">
                <div className="eyebrow"><Youtube size={15} /> YouTube source</div>
                <h2>Tempel link. Sisanya otomatis.</h2>
                <p>Public video maupun video Member yang akun browser kamu memang berhak akses.</p>
                <div className="url-row">
                  <div className="url-input-wrap">
                    <Youtube size={19} />
                    <input
                      value={url}
                      onChange={(e) => { setUrl(e.target.value); setMetadata(null); }}
                      onKeyDown={(e) => e.key === "Enter" && inspectVideo()}
                      placeholder="https://www.youtube.com/watch?v=..."
                      disabled={busy}
                    />
                  </div>
                  <button className="primary-button" onClick={inspectVideo} disabled={busy || inspecting}>
                    {inspecting ? <LoaderCircle className="spin" size={18} /> : <Search size={18} />}
                    {inspecting ? "Checking" : "Check video"}
                  </button>
                </div>
                <div className="helper-row">
                  <span>Untuk Member-only, pilih browser login di Settings.</span>
                  <button className="link-button" onClick={() => setTab("settings")}>YouTube access <ChevronRight size={14} /></button>
                </div>
              </div>

              {metadata && (
                <div className="video-card card">
                  <div className="thumbnail-wrap">
                    {metadata.thumbnail ? <img src={metadata.thumbnail} alt="Video thumbnail" /> : <div className="thumbnail-placeholder"><Youtube size={34} /></div>}
                    <span className="duration-chip"><Clock3 size={13} /> {formatDuration(metadata.duration)}</span>
                  </div>
                  <div className="video-info">
                    <span className="success-label"><Check size={14} /> Video ready</span>
                    <h3>{metadata.title}</h3>
                    <p>{metadata.channel}</p>
                    <div className="video-meta-line">
                      {metadata.availability && <span>{metadata.availability}</span>}
                      <span>{formatDuration(metadata.duration)}</span>
                    </div>
                  </div>
                </div>
              )}

              {busy && (
                <div className="progress-card card">
                  <div className="progress-header">
                    <div>
                      <span className="eyebrow"><LoaderCircle className="spin" size={15} /> Processing</span>
                      <h3>{stageLabels[progress.stage]}</h3>
                      <p>{progress.message || "Memproses secara lokal…"}</p>
                    </div>
                    <strong>{Math.round(progress.percent)}%</strong>
                  </div>
                  <div className="progress-track"><div className="progress-fill" style={{ width: `${Math.max(0, Math.min(100, progress.percent))}%` }} /></div>
                  <div className="progress-footer">
                    <span>{selectedModel?.label} • {backend === "auto" ? "Auto backend" : backend.toUpperCase()}</span>
                    <button className="danger-ghost" onClick={cancelJob}><CircleStop size={16} /> Cancel</button>
                  </div>
                </div>
              )}

              {result && !busy && (
                <div className="transcript-card card">
                  <div className="transcript-toolbar">
                    <div>
                      <span className="success-label"><Check size={14} /> Transcription complete</span>
                      <h3>{result.title}</h3>
                      <p>{result.language.toUpperCase()} • {result.model} • {result.backend.toUpperCase()}</p>
                    </div>
                    <div className="toolbar-actions">
                      <button className="secondary-button" onClick={copyTranscript}>{copied ? <Check size={16} /> : <Copy size={16} />}{copied ? "Copied" : "Copy"}</button>
                      <button className="secondary-button" onClick={() => exportFile("txt")}><FileText size={16} /> TXT</button>
                      <button className="secondary-button" onClick={() => exportFile("srt")}>SRT</button>
                      <button className="secondary-button" onClick={() => exportFile("vtt")}>VTT</button>
                    </div>
                  </div>
                  <div className="transcript-search"><Search size={16} /><input value={searchQuery} onChange={(e) => setSearchQuery(e.target.value)} placeholder="Cari di transcript…" /><span>{filteredSegments.length} segments</span></div>
                  <div className="segments">
                    {filteredSegments.map((segment, index) => (
                      <div className="segment" key={`${segment.from}-${index}`}>
                        <div className="timestamp"><Play size={11} fill="currentColor" /> {segment.from}</div>
                        <p>{segment.text}</p>
                      </div>
                    ))}
                    {filteredSegments.length === 0 && <div className="empty-inline">Tidak ada bagian transcript yang cocok.</div>}
                  </div>
                </div>
              )}
            </section>

            <aside className="control-column">
              <div className="card control-card sticky-card">
                <div className="card-title-row"><div><span className="eyebrow">Transcription</span><h3>Quality & compute</h3></div><Gauge size={20} /></div>

                <label className="field-label">Model</label>
                <div className="model-options">
                  {models.map((model) => (
                    <button key={model.id} className={modelId === model.id ? "model-option selected" : "model-option"} onClick={() => setModelId(model.id)} disabled={busy}>
                      <div className="radio-dot"><span /></div>
                      <div className="model-copy"><strong>{model.label}</strong><span>{model.description}</span></div>
                      <div className="model-state">{model.installed ? <Check size={15} /> : `${model.sizeMb} MB`}</div>
                    </button>
                  ))}
                </div>

                {selectedModel && !selectedModel.installed && (
                  <button className="download-button" onClick={() => downloadModel(selectedModel.id)} disabled={downloadingModel[selectedModel.id] !== undefined}>
                    {downloadingModel[selectedModel.id] !== undefined ? <LoaderCircle className="spin" size={17} /> : <Download size={17} />}
                    {downloadingModel[selectedModel.id] !== undefined ? `Downloading ${Math.round(downloadingModel[selectedModel.id])}%` : `Download ${selectedModel.label}`}
                  </button>
                )}

                <div className="divider" />

                <label className="field-label">Language</label>
                <select value={language} onChange={(e) => setLanguage(e.target.value)} disabled={busy}>
                  <option value="auto">Auto detect</option>
                  <option value="id">Indonesian</option>
                  <option value="en">English</option>
                  <option value="zh">Chinese</option>
                  <option value="ja">Japanese</option>
                  <option value="ko">Korean</option>
                </select>

                <label className="field-label">Compute backend</label>
                <select value={backend} onChange={(e) => setBackend(e.target.value as BackendChoice)} disabled={busy}>
                  <option value="auto">Auto — recommended</option>
                  <option value="cpu">CPU</option>
                  <option value="cuda" disabled={!system?.cudaEngine}>NVIDIA CUDA{system?.cudaEngine ? "" : " — not installed"}</option>
                </select>
                <div className="recommendation"><Sparkles size={14} /> {system?.recommendation ?? "Mendeteksi hardware…"}</div>

                <label className="toggle-row">
                  <div><strong>Keep processed audio</strong><span>Default off untuk hemat storage.</span></div>
                  <input type="checkbox" checked={keepAudio} onChange={(e) => setKeepAudio(e.target.checked)} disabled={busy} />
                </label>

                <button className="start-button" disabled={!metadata || busy || !selectedModel?.installed || !runtimeReady} onClick={startTranscription}>
                  <Sparkles size={18} /> Transcribe now
                </button>
                {!runtimeReady && <p className="setup-hint"><AlertCircle size={14} /> Runtime belum siap. Jalankan setup Windows.</p>}
              </div>
            </aside>
          </div>
        )}

        {tab === "history" && (
          <section className="card history-card">
            <div className="section-heading"><div><span className="eyebrow">Local library</span><h2>Transcription history</h2><p>Metadata dan transcript tersimpan hanya di komputer ini.</p></div><button className="secondary-button" onClick={() => refreshSystem()}><RotateCcw size={16} /> Refresh</button></div>
            {history.length === 0 ? (
              <div className="empty-state"><History size={36} /><h3>Belum ada history</h3><p>Selesaikan transkripsi pertama dan hasilnya akan muncul di sini.</p><button className="primary-button" onClick={() => setTab("transcribe")}>Start transcription</button></div>
            ) : (
              <div className="history-list">
                {history.map((item) => (
                  <button key={item.id} className="history-item" onClick={() => loadHistory(item.id)}>
                    <div className="history-icon"><FileText size={19} /></div>
                    <div className="history-copy"><strong>{item.title}</strong><span>{item.channel} • {new Date(item.createdAt).toLocaleString("id-ID")}</span></div>
                    <div className="history-meta"><span>{formatDuration(item.duration)}</span><span>{item.model}</span></div>
                    <ChevronRight size={18} />
                  </button>
                ))}
              </div>
            )}
          </section>
        )}

        {tab === "settings" && (
          <div className="settings-grid">
            <section className="card settings-card">
              <div className="card-title-row"><div><span className="eyebrow">Authentication</span><h3>YouTube access</h3></div><LockKeyhole size={20} /></div>
              <p className="settings-intro">WhisperTube tidak meminta email/password Google. Untuk Member-only, yt-dlp membaca session browser lokal yang sudah login.</p>
              <label className="field-label">Browser session</label>
              <select value={browser} onChange={(e) => setBrowser(e.target.value as BrowserChoice)} disabled={busy}>
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
                      <button className="icon-button danger" onClick={() => removeModel(model.id)} title="Delete model"><Trash2 size={17} /></button>
                    ) : (
                      <button className="secondary-button compact" onClick={() => downloadModel(model.id)} disabled={downloadingModel[model.id] !== undefined}>
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
              <button className="secondary-button full" onClick={() => refreshSystem()}><RotateCcw size={16} /> Re-check components</button>
            </section>
          </div>
        )}
      </main>
    </div>
  );
}
