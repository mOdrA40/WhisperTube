import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  cancelJob as cancelJobRequest,
  deleteHistory as deleteHistoryRequest,
  deleteModel,
  downloadModel,
  exportTranscriptFile,
  getSystemStatus,
  installAccelerator,
  installCudaEngine,
  inspectMedia,
  listBrowsers,
  listHistory,
  listModels,
  loadHistory,
  pickCookiesFile,
  revealAudioFile,
  startTranscription,
  subscribeToModelDownload,
  subscribeToAcceleratorDownload,
  subscribeToCudaDownload,
  subscribeToProgress,
} from "../services/tauri";
import { friendlyError } from "../lib/format";
import { getAcceleratorCopy, getModelCopy, useI18n } from "../i18n";
import type {
  AppTab,
  BackendChoice,
  BrowserChoice,
  BrowserInfo,
  HistoryItem,
  ModelInfo,
  ModelDownloadPayload,
  ProgressPayload,
  SystemStatus,
  TranscriptResult,
  VideoMetadata,
} from "../types";

const initialProgress: ProgressPayload = {
  stage: "idle",
  percent: 0,
  message: "",
  backend: null,
  downloadedBytes: null,
  totalBytes: null,
  networkBytesPerSecond: null,
  cpuUsagePercent: null,
  gpuUsagePercent: null,
};

const COOKIES_PATH_STORAGE_KEY = "whispertube.cookiesPath";
const ACCESS_BROWSER_STORAGE_KEY = "whispertube.accessBrowser";

function readStoredCookiesPath() {
  if (typeof window === "undefined") return "";
  try {
    const path = window.localStorage.getItem(COOKIES_PATH_STORAGE_KEY) ?? "";
    if (path.toLowerCase().endsWith("browser-session.cookies.txt")) {
      window.localStorage.removeItem(COOKIES_PATH_STORAGE_KEY);
      return "";
    }
    return path;
  } catch {
    return "";
  }
}

function persistCookiesPath(path: string) {
  try {
    if (path) window.localStorage.setItem(COOKIES_PATH_STORAGE_KEY, path);
    else window.localStorage.removeItem(COOKIES_PATH_STORAGE_KEY);
  } catch {
    // The file path still works for the current session if storage is unavailable.
  }
}

function readStoredBrowser(): BrowserChoice {
  if (typeof window === "undefined") return "none";
  try {
    return window.localStorage.getItem(ACCESS_BROWSER_STORAGE_KEY) === "safari" ? "safari" : "none";
  } catch {
    return "none";
  }
}

function persistBrowser(browser: BrowserChoice) {
  try {
    if (browser === "safari") window.localStorage.setItem(ACCESS_BROWSER_STORAGE_KEY, browser);
    else window.localStorage.removeItem(ACCESS_BROWSER_STORAGE_KEY);
  } catch {
    // The browser choice still works for the current session if storage is unavailable.
  }
}

export function useWhisperTube() {
  const { t } = useI18n();
  const [tab, setTab] = useState<AppTab>("transcribe");
  const [url, setUrl] = useState("");
  const [cookiesPath, setCookiesPath] = useState(readStoredCookiesPath);
  const [browser, setBrowser] = useState<BrowserChoice>(readStoredBrowser);
  const [backend, setBackend] = useState<BackendChoice>("auto");
  const [language, setLanguage] = useState("auto");
  const [modelId, setModelId] = useState("large-v3-turbo-q5_0");
  const [keepAudio, setKeepAudio] = useState(false);
  const [metadata, setMetadata] = useState<VideoMetadata | null>(null);
  const [system, setSystem] = useState<SystemStatus | null>(null);
  const [browsers, setBrowsers] = useState<BrowserInfo[]>([]);
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [history, setHistory] = useState<HistoryItem[]>([]);
  const [result, setResult] = useState<TranscriptResult | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [progress, setProgress] = useState<ProgressPayload>(initialProgress);
  const [busy, setBusy] = useState(false);
  const [inspecting, setInspecting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [downloadingModel, setDownloadingModel] = useState<Record<string, ModelDownloadPayload>>({});
  const [copied, setCopied] = useState(false);
  const [installingCuda, setInstallingCuda] = useState(false);
  const [cudaDownloadPercent, setCudaDownloadPercent] = useState(0);
  const [installingAccelerator, setInstallingAccelerator] = useState<Exclude<BackendChoice, "auto" | "cpu" | "cuda"> | null>(null);
  const [acceleratorDownloadPercent, setAcceleratorDownloadPercent] = useState(0);
  const [networkSpeedBytesPerSecond, setNetworkSpeedBytesPerSecond] = useState<number | null>(null);
  const autoConfigured = useRef(false);

  const selectedModel = models.find((model) => model.id === modelId);
  const runtimeReady = Boolean(system?.ytDlp && system?.ffmpeg && system?.cpuEngine);

  const refreshSystem = useCallback(async () => {
    const [nextSystem, nextModels, nextHistory, nextBrowsers] = await Promise.all([
      getSystemStatus(),
      listModels(),
      listHistory(),
      listBrowsers(),
    ]);
    setSystem(nextSystem);
    setBrowsers(nextBrowsers);
    if (!autoConfigured.current) {
      setModelId(nextSystem.recommendedModelId);
      setBackend(nextSystem.recommendedBackend);
      autoConfigured.current = true;
    }
    setModels(nextModels);
    setHistory(nextHistory);
  }, []);

  useEffect(() => {
    if (browser === "safari" && browsers.length > 0 && !browsers.some((item) => item.id === "safari")) {
      setBrowser("none");
      persistBrowser("none");
    }
  }, [browser, browsers]);

  useEffect(() => {
    refreshSystem().catch((cause) => setError(friendlyError(cause)));

    const unlistenProgress = subscribeToProgress((payload) => {
      setProgress(payload);
      setNetworkSpeedBytesPerSecond(
        payload.stage === "downloading" ? payload.networkBytesPerSecond : null,
      );
    });
    const unlistenModel = subscribeToModelDownload((payload) => {
      setDownloadingModel((previous) => ({ ...previous, [payload.id]: payload }));
      setNetworkSpeedBytesPerSecond(payload.bytesPerSecond);
    });
    const unlistenCuda = subscribeToCudaDownload((payload) => {
      setCudaDownloadPercent(payload.percent);
      setNetworkSpeedBytesPerSecond(payload.bytesPerSecond);
    });
    const unlistenAccelerator = subscribeToAcceleratorDownload((payload) => {
      setAcceleratorDownloadPercent(payload.percent);
      setNetworkSpeedBytesPerSecond(payload.bytesPerSecond);
    });

    return () => {
      unlistenProgress.then((unlisten) => unlisten());
      unlistenModel.then((unlisten) => unlisten());
      unlistenCuda.then((unlisten) => unlisten());
      unlistenAccelerator.then((unlisten) => unlisten());
    };
  }, [refreshSystem]);

  function handleUrlChange(nextUrl: string) {
    setUrl(nextUrl);
    setMetadata(null);
  }

  function resetInspectedVideo() {
    setMetadata(null);
    setResult(null);
    setSearchQuery("");
    setCopied(false);
  }

  function clearTranscription() {
    if (busy || inspecting) return;
    setUrl("");
    setMetadata(null);
    setResult(null);
    setSearchQuery("");
    setCopied(false);
    setNetworkSpeedBytesPerSecond(null);
    setProgress(initialProgress);
    setError(null);
  }

  async function handleSelectCookiesFile() {
    try {
      const path = await pickCookiesFile();
      if (!path) return;
      setBrowser("none");
      persistBrowser("none");
      setCookiesPath(path);
      persistCookiesPath(path);
      resetInspectedVideo();
      setError(null);
    } catch (cause) {
      setError(friendlyError(cause));
    }
  }

  function handleClearCookiesFile() {
    setCookiesPath("");
    persistCookiesPath("");
    resetInspectedVideo();
    setError(null);
  }

  function handleUseSafariSession() {
    if (!browsers.some((item) => item.id === "safari")) {
      setError(t("error.safariUnavailable"));
      return;
    }
    setBrowser("safari");
    persistBrowser("safari");
    setCookiesPath("");
    persistCookiesPath("");
    resetInspectedVideo();
    setError(null);
  }

  const filteredSegments = useMemo(() => {
    if (!result) return [];
    const query = searchQuery.trim().toLowerCase();
    if (!query) return result.segments;
    return result.segments.filter((segment) => segment.text.toLowerCase().includes(query));
  }, [result, searchQuery]);

  const autoAcceleratorInstalled = Boolean(system?.accelerators.some((accelerator) => accelerator.installed));
  const modelDownloadActive = Object.keys(downloadingModel).length > 0;
  const cudaInstallRequired = Boolean(system?.cudaSupported && system.nvidia && backend === "auto" && !system.cudaEngine && !autoAcceleratorInstalled);
  const vramWarning = useMemo(() => {
    if (!selectedModel || backend === "cpu" || backend === "metal" || backend === "vulkan" || !system) return null;
    if (backend === "cuda" && (!system.cudaSupported || !system.nvidia)) {
      return t("error.cudaUnavailable");
    }
    if (!system.nvidia) return null;
    if (!system.cudaSupported) {
      return t("error.cudaUnavailable");
    }
    if (!system.cudaEngine && backend === "auto" && !system.accelerators.some((accelerator) => accelerator.installed)) {
      return t("error.cudaRequired");
    }
    if (!system.cudaEngine) return null;
    if (backend === "cuda" && !system.gpuFreeMemoryMb && !system.gpuMemoryMb) {
      return t("error.vramUnreadable");
    }
    const available = system.gpuFreeMemoryMb ?? system.gpuMemoryMb;
    if (available !== null && available !== undefined && available < selectedModel.vramRequiredMb) {
      return t("error.vramTooLow", {
        model: getModelCopy(selectedModel, t).label,
        required: `${(selectedModel.vramRequiredMb / 1024).toFixed(1)} GB`,
        available: `${(available / 1024).toFixed(1)} GB`,
      });
    }
    return null;
  }, [backend, selectedModel, system, t]);
  const acceleratorWarning = useMemo(() => {
    if (!system || backend === "auto" || backend === "cpu" || backend === "cuda") return null;
    const accelerator = system.accelerators.find((item) => item.backend === backend);
    if (!accelerator?.installed) {
      return t("error.acceleratorMissing", {
        accelerator: accelerator ? getAcceleratorCopy(accelerator, t).label : backend,
      });
    }
    return null;
  }, [backend, system, t]);
  const canStart = Boolean(!busy && metadata && selectedModel?.installed && runtimeReady && !installingCuda && !installingAccelerator && !modelDownloadActive && !cudaInstallRequired && !vramWarning && !acceleratorWarning);

  async function inspectVideo() {
    if (!url.trim()) {
      setError(t("error.emptyUrl"));
      return;
    }
    setInspecting(true);
    setError(null);
    setResult(null);
    try {
      setMetadata(await inspectMedia(url.trim(), browser, "", cookiesPath));
    } catch (cause) {
      setMetadata(null);
      const message = friendlyError(cause);
      const normalizedMessage = message.toLowerCase();
      setError(
        normalizedMessage.startsWith("media_tiktok_transient:")
          ? t("error.tiktokMetadataRetry")
          : normalizedMessage.startsWith("media_source_transient:")
            ? t("error.sourceTemporary")
            : normalizedMessage.startsWith("media_source_rate_limited:")
              ? t("error.sourceRateLimited")
            : normalizedMessage.startsWith("media_source_membership_required:")
              ? t("error.sourceMembershipRequired")
              : normalizedMessage.startsWith("media_source_browser_decryption:")
                ? t("error.sourceBrowserEncryption")
              : normalizedMessage.startsWith("media_source_cookie_file:")
                ? t("error.sourceCookiesFile")
              : normalizedMessage.startsWith("media_source_js_runtime:")
                ? t("error.sourceJsRuntime")
              : normalizedMessage.startsWith("media_source_access_required:")
                ? t("error.sourceAccessRequired")
                : normalizedMessage.startsWith("media_source_unavailable:")
                  ? t("error.sourceUnavailable")
                  : normalizedMessage.startsWith("media_source_runtime:")
                    ? t("error.sourceRuntime")
                    : normalizedMessage.startsWith("media_source_metadata:")
                      ? t("error.sourceMetadata")
                      : normalizedMessage.startsWith("media_source_browser:")
                        ? t("error.sourceBrowser")
                        : normalizedMessage.startsWith("media_source_input:")
                          ? normalizedMessage.includes("domain video")
                            ? t("error.sourceUnsupported")
                            : t("error.sourceInputInvalid")
                          : message,
      );
    } finally {
      setInspecting(false);
    }
  }

  async function handleDownloadModel(id: string) {
    setError(null);
    setDownloadingModel((previous) => ({
      ...previous,
      [id]: { id, downloadedBytes: 0, totalBytes: 0, percent: 0, bytesPerSecond: null },
    }));
    try {
      await downloadModel(id);
      await refreshSystem();
    } catch (cause) {
      const message = friendlyError(cause);
      if (!message.toLowerCase().includes("dibatalkan")) setError(message);
    } finally {
      setNetworkSpeedBytesPerSecond(null);
      setDownloadingModel((previous) => {
        const next = { ...previous };
        delete next[id];
        return next;
      });
    }
  }

  async function handleRemoveModel(id: string) {
    setError(null);
    try {
      await deleteModel(id);
      if (modelId === id) setModelId("base");
      await refreshSystem();
    } catch (cause) {
      setError(friendlyError(cause));
    }
  }

  async function handleInstallCuda() {
    if (!system?.cudaSupported || !system.nvidia) {
      setError(t("error.cudaUnavailable"));
      return;
    }
    if (installingAccelerator !== null) {
      setError(t("error.installerBusy"));
      return;
    }
    setError(null);
    setInstallingCuda(true);
    setCudaDownloadPercent(0);
    setNetworkSpeedBytesPerSecond(null);
    try {
      await installCudaEngine();
      await refreshSystem();
    } catch (cause) {
      const message = friendlyError(cause);
      if (!message.toLowerCase().includes("dibatalkan")) setError(message);
    } finally {
      setInstallingCuda(false);
      setNetworkSpeedBytesPerSecond(null);
    }
  }

  async function handleInstallAccelerator(backendToInstall: Exclude<BackendChoice, "auto" | "cpu" | "cuda">) {
    if (installingCuda || installingAccelerator !== null) {
      setError(t("error.installerBusy"));
      return;
    }
    setError(null);
    setInstallingAccelerator(backendToInstall);
    setAcceleratorDownloadPercent(0);
    setNetworkSpeedBytesPerSecond(null);
    try {
      await installAccelerator(backendToInstall);
      await refreshSystem();
    } catch (cause) {
      const message = friendlyError(cause);
      if (!message.toLowerCase().includes("dibatalkan")) setError(message);
    } finally {
      setInstallingAccelerator(null);
      setNetworkSpeedBytesPerSecond(null);
    }
  }

  async function handleStartTranscription() {
    if (busy) return;
    if (!metadata) {
      setError(t("error.videoCheckFirst"));
      return;
    }
    if (!selectedModel?.installed) {
      setError(t("error.modelNotDownloaded"));
      return;
    }
    if (!runtimeReady) {
      setError(t("error.runtimeIncomplete"));
      return;
    }
    if (cudaInstallRequired) {
      setError(t("error.cudaRequired"));
      return;
    }
    if (vramWarning) {
      setError(vramWarning);
      return;
    }
    if (acceleratorWarning) {
      setError(acceleratorWarning);
      return;
    }

    setBusy(true);
    setError(null);
    setResult(null);
    setNetworkSpeedBytesPerSecond(null);
    setProgress({
      stage: "downloading",
      percent: 0,
      message: "",
      backend: null,
      downloadedBytes: 0,
      totalBytes: null,
      networkBytesPerSecond: null,
      cpuUsagePercent: null,
      gpuUsagePercent: null,
    });
    try {
      setResult(await startTranscription({
        url: metadata.webpageUrl,
        title: metadata.title,
        channel: metadata.channel,
        duration: metadata.duration,
        browser,
        browserProfile: "",
        cookiesPath,
        backend,
        language,
        modelId,
        keepAudio,
      }));
      setSearchQuery("");
      await refreshSystem();
    } catch (cause) {
      const message = friendlyError(cause);
      const normalizedMessage = message.toLowerCase();
      if (!normalizedMessage.includes("dibatalkan")) {
        setError(
          normalizedMessage.startsWith("media_source_cookie_file:")
            ? t("error.sourceCookiesFile")
            : message,
        );
      }
    } finally {
      setBusy(false);
    }
  }

  async function handleCancelJob() {
    try {
      await cancelJobRequest();
    } catch (cause) {
      setError(friendlyError(cause));
    }
  }

  async function handleLoadHistory(id: number) {
    setError(null);
    try {
      setResult(await loadHistory(id));
      setTab("transcribe");
      setSearchQuery("");
    } catch (cause) {
      setError(friendlyError(cause));
    }
  }

  async function handleDeleteHistory(ids: number[]) {
    if (ids.length === 0) return;
    setError(null);
    try {
      await deleteHistoryRequest(ids);
      if (result && ids.includes(result.historyId)) {
        setResult(null);
        setSearchQuery("");
        setCopied(false);
      }
      await refreshSystem();
    } catch (cause) {
      setError(friendlyError(cause));
      await refreshSystem().catch((refreshCause) => setError(friendlyError(refreshCause)));
      throw cause;
    }
  }

  async function handleExport(kind: "txt" | "srt" | "vtt") {
    if (!result) return;
    try {
      await exportTranscriptFile(result, kind);
    } catch (cause) {
      setError(friendlyError(cause));
    }
  }

  async function handleRevealAudio() {
    if (!result?.audioPath) return;
    try {
      await revealAudioFile(result.audioPath);
    } catch (cause) {
      setError(friendlyError(cause));
    }
  }

  async function copyTranscript() {
    if (!result) return;
    try {
      await navigator.clipboard.writeText(result.text);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1400);
    } catch (cause) {
      setError(friendlyError(cause));
    }
  }

  return {
    tab,
    setTab,
    url,
    setUrl: handleUrlChange,
    clearTranscription,
    cookiesPath,
    usingSafariSession: browser === "safari",
    browsers,
    selectCookiesFile: handleSelectCookiesFile,
    clearCookiesFile: handleClearCookiesFile,
    useSafariSession: handleUseSafariSession,
    backend,
    setBackend,
    language,
    setLanguage,
    modelId,
    setModelId,
    keepAudio,
    setKeepAudio,
    metadata,
    system,
    models,
    history,
    result,
    searchQuery,
    setSearchQuery,
    progress,
    busy,
    inspecting,
    error,
    setError,
    downloadingModel,
    copied,
    installingCuda,
    cudaDownloadPercent,
    selectedModel,
    runtimeReady,
    filteredSegments,
    canStart,
    cudaInstallRequired,
    vramWarning,
    acceleratorWarning,
    refreshSystem,
    inspectVideo,
    downloadModel: handleDownloadModel,
    removeModel: handleRemoveModel,
    installCuda: handleInstallCuda,
    installAccelerator: handleInstallAccelerator,
    installingAccelerator,
    acceleratorDownloadPercent,
    networkSpeedBytesPerSecond,
    startTranscription: handleStartTranscription,
    cancelJob: handleCancelJob,
    loadHistory: handleLoadHistory,
    deleteHistory: handleDeleteHistory,
    exportFile: handleExport,
    revealAudio: handleRevealAudio,
    copyTranscript,
  };
}
