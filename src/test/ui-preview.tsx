// Explicit development-only entry point. No hooks/services/IPC or user data are imported.
// Open /src/test/ui-preview.html?scene=ready&lang=id on the local Vite server.
// This HTML is not a production build entry and is never imported by main.tsx.
import { useState } from "react";
import { createRoot } from "react-dom/client";
import { AppShell } from "../components/layout/AppShell";
import { TranscribePage } from "../components/transcribe/TranscribePage";
import { HistoryPage } from "../components/history/HistoryPage";
import { SettingsPage } from "../components/settings/SettingsPage";
import { ErrorAlert } from "../components/common/ErrorAlert";
import { AppUpdateBanner } from "../components/common/AppUpdateBanner";
import { I18nProvider } from "../i18n";
import type { AppTab, AppUpdateStatus, ProgressStage, SystemStatus, TranscriptResult } from "../types";
import "../styles.css";
import thumbnail from "./media-thumbnail.svg";

const params = new URLSearchParams(location.search);
const scene = params.get("scene") ?? "empty";
const language = params.get("lang") ?? "en";
localStorage.setItem("whispertube.ui-language", ["en", "id", "zh"].includes(language) ? language : "en");
// Only the platform titlebar needs metadata to render in a browser fixture.
// Its real native methods are verified separately in Tauri, not simulated here.
Object.defineProperty(window, "__TAURI_INTERNALS__", {
  configurable: true,
  value: { metadata: { currentWindow: { label: "ui-fixture" } }, invoke: () => Promise.reject(new Error("Native window actions are unavailable in this isolated UI fixture.")) },
});

const modelDownloading = ["model-download", "settings-model-download", "result-model-download"].includes(scene);
const acceleratorDownloading = scene === "accelerator-download";
const cudaDownloading = scene === "cuda-download";
const modelAbsent = scene === "missing-model" || modelDownloading;
const models = [
  { id: "base", label: "Fast", description: "", sizeMb: 142, vramRequiredMb: 2048, installed: !modelAbsent },
  { id: "large-v3-turbo-q5_0", label: "Balanced", description: "", sizeMb: 547, vramRequiredMb: 4096, installed: !modelAbsent },
  { id: "large-v3-q5_0", label: "Accurate", description: "", sizeMb: 1100, vramRequiredMb: 7168, installed: false },
];
const system: SystemStatus = {
  qwenBackends: ["auto", "cpu", "cuda"],
  ytDlp: true, ffmpeg: scene !== "runtime-missing", cpuEngine: true, cudaEngine: scene !== "driver-warning" && !cudaDownloading, cudaSupported: true,
  nvidia: true, gpuName: "NVIDIA GeForce — intentionally long compute-device fixture name",
  gpuMemoryMb: 8192, gpuFreeMemoryMb: 6144, availableMemoryMb: 16384, nvidiaDriverVersion: "fixture",
  cudaInstallable: scene !== "driver-warning", cpuThreads: 12, recommendation: "", recommendedModelId: models[1].id,
  recommendedBackend: "auto", accelerators: [{ id: "vulkan", label: "Vulkan", backend: "vulkan",
    supported: true, installed: false, downloadable: true, memoryAvailableMb: acceleratorDownloading ? 16384 : 1024, memoryRequiredMb: 2560,
    memorySufficient: acceleratorDownloading, preflightChecked: acceleratorDownloading, preflightCompatible: acceleratorDownloading, description: "fixture" }],
  computeDevices: [
    { id: "cuda:0", backend: "cuda", name: "NVIDIA GeForce — intentionally long compute-device fixture name", vendor: "nvidia", deviceIndex: 0, integrated: false, totalMemoryMb: 8192, freeMemoryMb: 6144 },
    { id: "vulkan:2", backend: "vulkan", name: "Intel integrated graphics with a second intentionally long adapter name", vendor: "intel", deviceIndex: 2, integrated: true, totalMemoryMb: null, freeMemoryMb: null },
  ],
  jobStorageBytes: 182 * 1024 * 1024, jobStorageLimitBytes: 20 * 1024 * 1024 * 1024,
};
const paragraphs = [
  "A useful transcript makes the spoken word easier to revisit. This isolated fixture checks comfortable line lengths, consistent timestamps, and the flow of a long document.",
  "Percakapan yang panjang perlu tetap mudah dibaca. Tampilan ini menggunakan data contoh, bukan riwayat atau transkrip milik pengguna.",
  "这里使用的是独立测试数据。长段落、时间戳和不同语言都应该保持清晰，不依赖装饰来建立层次。",
];
const segments = Array.from({ length: 1200 }, (_, index) => ({
  from: `${String(Math.floor(index / 60)).padStart(2, "0")}:${String(index % 60).padStart(2, "0")}`,
  to: `${String(Math.floor((index + 1) / 60)).padStart(2, "0")}:${String((index + 1) % 60).padStart(2, "0")}`,
  text: paragraphs[index % paragraphs.length],
}));
const result: TranscriptResult = {
  historyId: 1, title: "The art of listening — notes from a long conversation",
  channel: "Studio fixture", language: "en", duration: 1200, model: models[1].id, backend: "cpu",
  segments, text: paragraphs.join("\n"), txtPath: "/fixture/transcript.txt", srtPath: "/fixture/transcript.srt",
  vttPath: "/fixture/transcript.vtt", audioPath: "/fixture/audio.wav",
  ...(scene === "audio-only" ? { transcriptAvailable: false } : {}),
  ...(scene === "duration-mismatch" ? { sourceDuration: 2400 } : {}),
};
const history = Array.from({ length: 12 }, (_, index) => ({
  id: index + 1,
  title: index === 1 ? "A very long video title that tests wrapping and alignment without hiding the file and delete actions, including 中文与 Bahasa Indonesia" : `Conversation ${String(index + 1).padStart(2, "0")} — ${["On attention and the everyday", "Notes from the studio", "A slower kind of work"][index % 3]}`,
  channel: "Studio fixture", sourceUrl: "https://example.test/video", createdAt: "2026-10-24T08:30:00Z",
  duration: 2400 + index * 63, language: "en", model: models[1].id, backend: "cpu",
}));
const noop = () => undefined;

function Fixture() {
  const [tab, setTab] = useState<AppTab>(scene.startsWith("history") ? "history" : scene.startsWith("settings") ? "settings" : "transcribe");
  const [url, setUrl] = useState(scene === "empty" ? "" : "https://example.test/video");
  const [modelId, setModelId] = useState(models[1].id);
  const [computeTargetId, setComputeTargetId] = useState("auto");
  const [transcriptLanguage, setTranscriptLanguage] = useState("auto");
  const [keepAudio, setKeepAudio] = useState(false);
  const [search, setSearch] = useState(scene === "search-empty" ? "not-present-in-the-fixture" : "");
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState<string | null>(scene === "error" ? "The source could not be read. Check access and try again. This is fixture text." : null);
  const [cacheLoaded, setCacheLoaded] = useState(false);
  const [cookiesPath, setCookiesPath] = useState("");
  const [updateStatus, setUpdateStatus] = useState<AppUpdateStatus>(
    scene.includes("update-installing") ? "installing" : scene.includes("update-checking") ? "checking" :
      scene.includes("update-current") ? "up-to-date" : scene.includes("update") ? "available" : "idle",
  );
  const updateError = scene.includes("update-error") ? "Fixture: the update could not be installed. Try again when the connection is available." : null;
  const [filesFailure, setFilesFailure] = useState(scene === "history-failure");
  const stages: ProgressStage[] = ["downloading", "converting", "transcribing", "finalizing"];
  const busy = stages.includes(scene as ProgressStage);
  const hasResult = ["result", "result-model-download", "search-empty", "audio-only", "duration-mismatch"].includes(scene);
  const metadata = scene === "empty" || scene === "inspecting" || scene === "cancelling" ? null : {
    id: "fixture", title: result.title, channel: "Studio fixture", duration: 1200,
    thumbnail, webpageUrl: url, availability: "public", source: "Video source",
  };
  const fail = async () => { setError("Fixture: the operation could not finish. Your selection has been kept."); throw new Error("Fixture failure"); };
  return <>
    <AppShell tab={tab} historyCount={history.length} runtimeReady={scene !== "runtime-missing"} systemLoading={false} system={system} onTabChange={setTab}>
      <ErrorAlert message={error} onDismiss={() => setError(null)} />
      {tab === "transcribe" && <TranscribePage url={url} busy={busy} operationActive={busy || modelDownloading || acceleratorDownloading || cudaDownloading}
        inspecting={scene === "inspecting" || scene === "cancelling"} cancellingInspection={scene === "cancelling"}
        metadata={metadata} hasResult={hasResult} progress={{ stage: busy ? scene as ProgressStage : "idle", messageCode: "fixture", message: "Processing the fixture audio with the selected runtime.", percent: 42,
          backend: scene === "transcribing" ? "cpu" : null, downloadedBytes: 32000000, totalBytes: 76000000,
          networkBytesPerSecond: scene === "downloading" ? 3200000 : null, cpuUsagePercent: scene === "transcribing" ? 68 : null, gpuUsagePercent: null }}
        transcriptionElapsedMs={busy || hasResult ? 126000 : null} selectedModel={models.find((model) => model.id === modelId)}
        backend="auto" system={system} models={models} modelId={modelId} language={transcriptLanguage} keepAudio={keepAudio}
        canStart={Boolean(metadata) && !busy && !modelAbsent && !acceleratorDownloading && !cudaDownloading && scene !== "warning" && scene !== "checking-resume" && scene !== "runtime-missing"}
        resumeStage={scene === "resume" ? "audio" : scene === "resume-download" ? "download" : null}
        checkingResume={scene === "checking-resume"} retryAvailable={scene === "retry"} modelDownloadBlocked={false}
        runtimeReady={scene !== "runtime-missing"} computeTargetId={computeTargetId} downloadingModel={modelDownloading ? { [modelId]: { id: modelId, downloadedBytes: 120000000, totalBytes: 547000000, percent: 22, bytesPerSecond: 3000000 } } : {}}
        accelerators={system.accelerators} installingCuda={cudaDownloading} cudaDownloadPercent={42} installingAccelerator={acceleratorDownloading ? "vulkan" : null}
        acceleratorDownloadPercent={53} networkSpeedBytesPerSecond={(busy && scene === "downloading") || modelDownloading || cudaDownloading || acceleratorDownloading ? 3200000 : null}
        vramWarning={scene === "warning" ? "Fixture: the selected model requires more free VRAM. Select a smaller model or CPU." : null}
        acceleratorWarning={null} result={hasResult ? result : null} copied={copied} searchQuery={search}
        filteredSegments={segments.filter((segment) => segment.text.toLowerCase().includes(search.toLowerCase()))}
        onTabChange={setTab} onUrlChange={setUrl} onInspect={noop} onClear={() => setUrl("")} onCancelInspection={noop}
        onCancelTranscription={noop} onModelChange={setModelId} onComputeTargetChange={setComputeTargetId}
        onLanguageChange={setTranscriptLanguage} onKeepAudioChange={setKeepAudio} onDownloadModel={noop}
        onCancelModel={noop} onInstallCuda={noop} onCancelCuda={noop} onInstallAccelerator={noop}
        onCancelAccelerator={noop} onStart={noop} onCopy={() => setCopied(true)} onExport={noop}
        onRevealAudio={noop} onSearchChange={setSearch} />}
      {tab === "history" && <HistoryPage history={scene === "history-empty" ? [] : history} hasMore loadingMore={false} operationActive={false}
        onRefresh={noop} onLoadMore={noop} onLoad={noop} onDelete={fail}
        onDeleteFiles={fail} onListFiles={async () => {
          if (filesFailure) { setFilesFailure(false); throw new Error("Fixture: saved files are temporarily unavailable."); }
          return [
            { kind: "audio", path: "C:/fixture/a deliberately long storage directory/another long subdirectory/audio.wav", bytes: 32000000, available: true, pendingDeletion: false },
            { kind: "txt", path: "C:/fixture/transcript.txt", bytes: 18000, available: true, pendingDeletion: false },
            { kind: "srt", path: "C:/fixture/transcript.srt", bytes: null, available: false, pendingDeletion: true },
            { kind: "vtt", path: "C:/fixture/transcript.vtt", bytes: null, available: false, pendingDeletion: false },
          ];
        }} onRevealFile={async () => undefined} onTabChange={setTab} />}
      {tab === "settings" && <SettingsPage cookiesPath={cookiesPath} system={system} models={models} modelDownloadBlockReasons={{}}
        busy={modelDownloading || updateStatus === "installing"} resettingData={false} historyTotalCount={12}
        downloadingModel={modelDownloading ? { [modelId]: { id: modelId, downloadedBytes: 120000000, totalBytes: 547000000, percent: 22, bytesPerSecond: 3000000 } } : {}} accelerators={system.accelerators}
        installingCuda={false} cudaDownloadPercent={0} installingAccelerator={null} acceleratorDownloadPercent={0}
        networkSpeedBytesPerSecond={null} updateChecking={updateStatus === "checking"} appUpdate={updateStatus === "idle" || updateStatus === "checking" ? null : { version: "0.0.0-fixture", notes: "Preview fixture only.", date: null }}
        appUpdateStatus={updateStatus} appUpdateProgress={{ downloadedBytes: 42000, totalBytes: 100000, percent: 42 }} appUpdateError={updateError}
        onSelectCookiesFile={() => setCookiesPath("C:/fixture/cookies.txt")} onClearCookiesFile={() => setCookiesPath("")}
        onOpenUrl={noop} onDownloadModel={noop} onCancelModel={noop} onRemoveModel={fail} onResetUserData={fail}
        jobCache={cacheLoaded ? [{ id: "fixture-job", title: "An unfinished conversation", sourceUrl: "https://example.test/video", bytes: 182000000, modifiedAt: "2026-10-24T08:30:00Z" }] : null}
        onLoadJobCache={async () => setCacheLoaded(true)} onDiscardJobCache={fail} onRefresh={noop} onInstallCuda={noop}
        onCancelCuda={noop} onInstallAccelerator={noop} onCancelAccelerator={noop}
        onCheckForUpdate={() => setUpdateStatus("available")} onInstallAppUpdate={() => setUpdateStatus("installing")} />}
      {scene.startsWith("update") && <AppUpdateBanner update={{ version: "0.0.0-fixture", notes: "Fixture update: this does not install anything.", date: null }} status={updateStatus}
        progress={{ downloadedBytes: 42000, totalBytes: 100000, percent: 42 }} error={updateError} busy={updateStatus === "installing"} onInstall={noop} onDismiss={noop} />}
    </AppShell>
    <div style={{ position: "fixed", left: 8, bottom: 4, fontSize: 10, color: "var(--secondary)", pointerEvents: "none", zIndex: 1100 }}>UI FIXTURE · {scene} · synthetic data</div>
  </>;
}

createRoot(document.getElementById("root")!).render(<I18nProvider><Fixture /></I18nProvider>);
