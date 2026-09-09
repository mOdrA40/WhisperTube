import { useCallback, useDeferredValue, useEffect, useMemo, useRef, useState } from "react";
import {
  cancelJob as cancelJobRequest,
  checkForAppUpdate,
  deleteHistory as deleteHistoryRequest,
  deleteModel,
  downloadModel,
  exportTranscriptFile,
  getSystemStatus,
  installAccelerator,
  installCudaEngine,
  inspectMedia,
  installAppUpdate,
  listHistory,
  listModels,
  loadHistory,
  pickCookiesFile,
  revealAudioFile,
  resetUserData as resetUserDataRequest,
  startTranscription,
  subscribeToModelDownload,
  subscribeToAcceleratorDownload,
  subscribeToCudaDownload,
  subscribeToProgress,
} from "../services/tauri";
import { errorCode, friendlyError } from "../lib/format";
import { getAcceleratorCopy, getModelCopy, useI18n, type Translate } from "../i18n";
import type {
  AppUpdateInfo,
  AppUpdateProgress,
  AppUpdateStatus,
  AppTab,
  BackendChoice,
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
  messageCode: "idle",
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

function localizedMetadataError(cause: unknown, t: Translate) {
  const code = errorCode(cause);
  if (code === "media_tiktok_transient") return t("error.tiktokMetadataRetry");
  if (code === "media_source_transient") return t("error.sourceTemporary");
  if (code === "media_source_rate_limited") return t("error.sourceRateLimited");
  if (code === "media_source_membership_required") return t("error.sourceMembershipRequired");
  if (code === "media_source_cookie_file") return t("error.sourceCookiesFile");
  if (code === "media_source_js_runtime") return t("error.sourceJsRuntime");
  if (code === "media_source_access_required") return t("error.sourceAccessRequired");
  if (code === "media_source_unavailable") return t("error.sourceUnavailable");
  if (code === "media_source_runtime") return t("error.sourceRuntime");
  if (code === "media_source_metadata") return t("error.sourceMetadata");
  if (code === "media_source_duration") return t("error.sourceDuration");
  if (code === "media_source_input") {
    return friendlyError(cause).toLowerCase().includes("domain video")
      ? t("error.sourceUnsupported")
      : t("error.sourceInputInvalid");
  }
  return friendlyError(cause);
}
function readStoredCookiesPath() {
  if (typeof window === "undefined") return "";
  try {
    const path = window.localStorage.getItem(COOKIES_PATH_STORAGE_KEY) ?? "";
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

export function useWhisperTube() {
  const { t } = useI18n();
  const [tab, setTab] = useState<AppTab>("transcribe");
  const [url, setUrl] = useState("");
  const [cookiesPath, setCookiesPath] = useState(readStoredCookiesPath);
  const [backend, setBackend] = useState<BackendChoice>("auto");
  const [computeTargetId, setComputeTargetId] = useState("auto");
  const [language, setLanguage] = useState("auto");
  const [modelId, setModelId] = useState("large-v3-turbo-q5_0");
  const [keepAudio, setKeepAudio] = useState(false);
  const [metadata, setMetadata] = useState<VideoMetadata | null>(null);
  const [system, setSystem] = useState<SystemStatus | null>(null);
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [history, setHistory] = useState<HistoryItem[]>([]);
  const [historyTotalCount, setHistoryTotalCount] = useState(0);
  const [historyHasMore, setHistoryHasMore] = useState(false);
  const [loadingMoreHistory, setLoadingMoreHistory] = useState(false);
  const [result, setResult] = useState<TranscriptResult | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [progress, setProgress] = useState<ProgressPayload>(initialProgress);
  const [busy, setBusy] = useState(false);
  const [inspecting, setInspecting] = useState(false);
  const [cancellingInspection, setCancellingInspection] = useState(false);
  const [resettingData, setResettingData] = useState(false);
  const [historyOperation, setHistoryOperation] = useState<"reading" | "deleting" | "exporting" | "revealing" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [downloadingModel, setDownloadingModel] = useState<Record<string, ModelDownloadPayload>>({});
  const [copied, setCopied] = useState(false);
  const [installingCuda, setInstallingCuda] = useState(false);
  const [cudaDownloadPercent, setCudaDownloadPercent] = useState(0);
  const [installingAccelerator, setInstallingAccelerator] = useState<Exclude<BackendChoice, "auto" | "cpu" | "cuda"> | null>(null);
  const [acceleratorDownloadPercent, setAcceleratorDownloadPercent] = useState(0);
  const [networkSpeedBytesPerSecond, setNetworkSpeedBytesPerSecond] = useState<number | null>(null);
  const [appUpdate, setAppUpdate] = useState<AppUpdateInfo | null>(null);
  const [appUpdateStatus, setAppUpdateStatus] = useState<AppUpdateStatus>("idle");
  const [appUpdateProgress, setAppUpdateProgress] = useState<AppUpdateProgress>({
    downloadedBytes: 0,
    totalBytes: null,
    percent: 0,
  });
  const [appUpdateError, setAppUpdateError] = useState<string | null>(null);
  const [refreshingSystem, setRefreshingSystem] = useState(false);
  const autoConfigured = useRef(false);
  const refreshingSystemRef = useRef(false);

  const selectedModel = models.find((model) => model.id === modelId);
  const runtimeReady = Boolean(
    system?.ytDlp &&
      system.ffmpeg &&
      (backend === "cpu"
        ? system.cpuEngine
        : backend === "cuda"
          ? system.cudaSupported && system.nvidia && system.cudaEngine
          : backend === "metal"
            ? system.accelerators.some((accelerator) => accelerator.backend === "metal" && accelerator.installed)
            : backend === "vulkan"
              ? system.accelerators.some((accelerator) => accelerator.backend === "vulkan" && accelerator.installed)
              : system.cpuEngine ||
                (system.cudaSupported && system.nvidia && system.cudaEngine) ||
                system.accelerators.some((accelerator) => accelerator.installed)),
  );

  const refreshSystem = useCallback(async () => {
    if (refreshingSystemRef.current) return;
    refreshingSystemRef.current = true;
    setRefreshingSystem(true);
    try {
      const [systemResult, modelsResult, historyResult] = await Promise.allSettled([
        getSystemStatus(),
        listModels(),
        listHistory(),
      ]);

      if (systemResult.status === "fulfilled") {
        const nextSystem = systemResult.value;
        setSystem(nextSystem);
        if (!autoConfigured.current) {
          setModelId(nextSystem.recommendedModelId);
          setBackend(nextSystem.recommendedBackend);
          setComputeTargetId("auto");
          autoConfigured.current = true;
        }
      }
      if (modelsResult.status === "fulfilled") setModels(modelsResult.value);
      if (historyResult.status === "fulfilled") {
        setHistory(historyResult.value.items);
        setHistoryHasMore(historyResult.value.hasMore);
        setHistoryTotalCount(historyResult.value.totalCount);
      }

      const failures = [systemResult, modelsResult, historyResult]
        .filter((result): result is PromiseRejectedResult => result.status === "rejected")
        .map((result) => friendlyError(result.reason));
      if (failures.length > 0) throw new Error(failures.join(" "));
    } finally {
      refreshingSystemRef.current = false;
      setRefreshingSystem(false);
    }
  }, []);

  const refreshSystemFromUi = useCallback(async () => {
    try {
      await refreshSystem();
    } catch (cause) {
      setError(friendlyError(cause));
    }
  }, [refreshSystem]);

  const refreshAfterSuccessfulOperation = useCallback(async () => {
    try {
      await refreshSystem();
    } catch (cause) {
      setError(t("error.operationSucceededRefreshFailed", { detail: friendlyError(cause) }));
    }
  }, [refreshSystem, t]);

  const refreshHistoryAfterMutationFailure = useCallback(async () => {
    try {
      const page = await listHistory();
      setHistory(page.items);
      setHistoryHasMore(page.hasMore);
      setHistoryTotalCount(page.totalCount);
    } catch {
      // Preserve the mutation error: a failed reconciliation must not hide its cause.
    }
  }, []);

  const refreshAfterMutationFailure = useCallback(async () => {
    try {
      await refreshSystem();
    } catch {
      // Preserve the mutation error: refreshSystem already applied every fulfilled result.
    }
  }, [refreshSystem]);

  useEffect(() => {
    let disposed = false;
    refreshSystem().catch((cause) => setError(friendlyError(cause)));
    checkForAppUpdate()
      .then((update) => {
        if (disposed) return;
        setAppUpdate(update);
        setAppUpdateStatus(update ? "available" : "up-to-date");
      })
      .catch(() => {
        // Update checks are best-effort on startup. The Settings action reports manual failures.
      });

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
      disposed = true;
      unlistenProgress.then((unlisten) => unlisten());
      unlistenModel.then((unlisten) => unlisten());
      unlistenCuda.then((unlisten) => unlisten());
      unlistenAccelerator.then((unlisten) => unlisten());
    };
  }, [refreshSystem]);

  async function handleCheckForAppUpdate() {
    setAppUpdateStatus("checking");
    setAppUpdateError(null);
    try {
      const update = await checkForAppUpdate();
      setAppUpdate(update);
      setAppUpdateStatus(update ? "available" : "up-to-date");
    } catch (cause) {
      setAppUpdateStatus("idle");
      setAppUpdateError(friendlyError(cause));
    }
  }

  async function handleInstallAppUpdate() {
    if (!appUpdate || operationActive) return;
    setAppUpdateStatus("installing");
    setAppUpdateError(null);
    setAppUpdateProgress({ downloadedBytes: 0, totalBytes: null, percent: 0 });
    try {
      await installAppUpdate(setAppUpdateProgress);
      setAppUpdate(null);
      setAppUpdateStatus("up-to-date");
    } catch (cause) {
      setAppUpdateStatus("available");
      setAppUpdateError(friendlyError(cause));
    }
  }

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

  const deferredSearchQuery = useDeferredValue(searchQuery);
  const filteredSegments = useMemo(() => {
    if (!result) return [];
    const query = deferredSearchQuery.trim().toLowerCase();
    if (!query) return result.segments;
    return result.segments.filter((segment) => segment.text.toLowerCase().includes(query));
  }, [deferredSearchQuery, result]);

  const modelDownloadActive = Object.keys(downloadingModel).length > 0;
  const appUpdateInstalling = appUpdateStatus === "installing";
  const operationActive = Boolean(
    busy || inspecting || resettingData || historyOperation || modelDownloadActive || installingCuda || installingAccelerator || appUpdateInstalling || refreshingSystem,
  );
  const vramWarning = useMemo(() => {
    if (!selectedModel || backend === "cpu" || backend === "metal" || backend === "vulkan" || !system) return null;
    if (backend === "cuda" && (!system.cudaSupported || !system.nvidia)) {
      return t("error.cudaUnavailable");
    }
    if (!system.nvidia || !system.cudaSupported) return null;
    if (!system.cudaEngine) return null;
    const selectedCudaDevice = computeTargetId.startsWith("cuda:")
      ? system.computeDevices.find((device) => device.id === computeTargetId)
      : null;
    const available = selectedCudaDevice?.freeMemoryMb ?? system.gpuFreeMemoryMb;
    if ((backend === "cuda" || (backend === "auto" && system.cudaEngine)) && !available) {
      return t("error.vramUnreadable");
    }
    if (available !== null && available !== undefined && available < selectedModel.vramRequiredMb) {
      return t("error.vramTooLow", {
        model: getModelCopy(selectedModel, t).label,
        required: `${(selectedModel.vramRequiredMb / 1024).toFixed(1)} GB`,
        available: `${(available / 1024).toFixed(1)} GB`,
      });
    }
    return null;
  }, [backend, computeTargetId, selectedModel, system, t]);
  const acceleratorWarning = useMemo(() => {
    if (!system || backend === "auto" || backend === "cpu" || backend === "cuda") return null;
    if (backend === "vulkan" && !system.computeDevices.some((device) => device.backend === "vulkan")) {
      return t("error.vulkanDeviceUnavailable");
    }
    const accelerator = system.accelerators.find((item) => item.backend === backend);
    if (!accelerator?.installed) {
      return t("error.acceleratorMissing", {
        accelerator: accelerator ? getAcceleratorCopy(accelerator, t).label : backend,
      });
    }
    return null;
  }, [backend, system, t]);
  const canStart = Boolean(!operationActive && metadata && selectedModel?.installed && runtimeReady && !vramWarning && !acceleratorWarning);
  const modelDownloadBlockReasons = useMemo<Record<string, string>>(() => {
    const reasons: Record<string, string> = {};
    const cudaTarget = backend === "cuda" || (backend === "auto" && system?.cudaEngine);
    if (!system || !cudaTarget) return reasons;
    const selectedCudaDevice = computeTargetId.startsWith("cuda:")
      ? system.computeDevices.find((device) => device.id === computeTargetId)
      : null;
    const available = selectedCudaDevice?.freeMemoryMb ?? system.gpuFreeMemoryMb;
    for (const model of models) {
      if (available === null || available === undefined) {
        reasons[model.id] = t("error.vramUnreadable");
      } else if (available < model.vramRequiredMb) {
        reasons[model.id] = t("error.vramTooLow", {
          model: getModelCopy(model, t).label,
          required: `${(model.vramRequiredMb / 1024).toFixed(1)} GB`,
          available: `${(available / 1024).toFixed(1)} GB`,
        });
      }
    }
    return reasons;
  }, [backend, computeTargetId, models, system, t]);
  const modelDownloadBlocked = Boolean(
    selectedModel && modelDownloadBlockReasons[selectedModel.id],
  );

  function handleComputeTargetChange(targetId: string) {
    setComputeTargetId(targetId);
    if (targetId === "auto") {
      setBackend("auto");
      return;
    }
    if (targetId === "cpu" || targetId === "metal") {
      setBackend(targetId);
      return;
    }
    const backendFromTarget = targetId.split(":", 1)[0];
    if (backendFromTarget === "cuda" || backendFromTarget === "vulkan") {
      setBackend(backendFromTarget);
      return;
    }
    setError(`Target compute tidak dikenal: ${targetId}`);
  }

  function clearAppUpdateError() {
    setAppUpdateError(null);
  }

  async function inspectVideo() {
    if (operationActive) return;
    if (!url.trim()) {
      setError(t("error.emptyUrl"));
      return;
    }
    setInspecting(true);
    setCancellingInspection(false);
    setError(null);
    setResult(null);
    try {
      setMetadata(await inspectMedia(url.trim(), cookiesPath));
    } catch (cause) {
      setMetadata(null);
      const message = friendlyError(cause);
      const normalizedMessage = message.toLowerCase();
      if (normalizedMessage.includes("dibatalkan")) return;
      setError(localizedMetadataError(cause, t));
    } finally {
      setInspecting(false);
      setCancellingInspection(false);
    }
  }

  async function handleDownloadModel(id: string) {
    if (operationActive) return;
    setError(null);
    setDownloadingModel((previous) => ({
      ...previous,
      [id]: { id, downloadedBytes: 0, totalBytes: 0, percent: 0, bytesPerSecond: null },
    }));
    try {
      await downloadModel(id, computeTargetId);
      await refreshAfterSuccessfulOperation();
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
    if (operationActive) return;
    setError(null);
    try {
      await deleteModel(id);
      if (modelId === id) setModelId("base");
      await refreshAfterSuccessfulOperation();
    } catch (cause) {
      setError(friendlyError(cause));
      throw cause;
    }
  }

  async function handleResetUserData() {
    if (operationActive) return;
    setError(null);
    setResettingData(true);
    try {
      await resetUserDataRequest();
      try {
        window.localStorage.removeItem(COOKIES_PATH_STORAGE_KEY);
      } catch {
        // Backend data reset remains successful when WebView storage is unavailable.
      }
      setCookiesPath("");
      setMetadata(null);
      setResult(null);
      setSearchQuery("");
    } catch (cause) {
      const message = friendlyError(cause);
      await refreshAfterMutationFailure();
      setError(message);
      throw cause;
    } finally {
      setResettingData(false);
    }
    await refreshAfterSuccessfulOperation();
  }

  async function handleInstallCuda() {
    if (operationActive) return;
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
      await refreshAfterSuccessfulOperation();
    } catch (cause) {
      const message = friendlyError(cause);
      if (!message.toLowerCase().includes("dibatalkan")) setError(message);
    } finally {
      setInstallingCuda(false);
      setNetworkSpeedBytesPerSecond(null);
    }
  }

  async function handleInstallAccelerator(backendToInstall: Exclude<BackendChoice, "auto" | "cpu" | "cuda">) {
    if (operationActive) return;
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
      await refreshAfterSuccessfulOperation();
    } catch (cause) {
      const message = friendlyError(cause);
      if (!message.toLowerCase().includes("dibatalkan")) setError(message);
    } finally {
      setInstallingAccelerator(null);
      setNetworkSpeedBytesPerSecond(null);
    }
  }

  async function handleStartTranscription() {
    if (operationActive) return;
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
        messageCode: "preparing_download",
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
      const completedResult = await startTranscription({
        url: metadata.webpageUrl,
        title: metadata.title,
        channel: metadata.channel,
        duration: metadata.duration,
        cookiesPath,
        backend,
        computeDeviceId: computeTargetId,
        language,
        modelId,
        keepAudio,
      });
      setResult(completedResult);
      setSearchQuery("");
      await refreshAfterSuccessfulOperation();
    } catch (cause) {
      const message = friendlyError(cause);
      const normalizedMessage = message.toLowerCase();
      if (!normalizedMessage.includes("dibatalkan")) {
        setError(
          errorCode(cause) === "media_source_cookie_file"
            ? t("error.sourceCookiesFile")
            : errorCode(cause) === "storage_quota_exceeded"
              ? t("error.storageQuotaExceeded")
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
    if (operationActive) return;
    setHistoryOperation("reading");
    setError(null);
    try {
      setResult(await loadHistory(id));
      setTab("transcribe");
      setSearchQuery("");
    } catch (cause) {
      setError(friendlyError(cause));
    } finally {
      setHistoryOperation(null);
    }
  }

  async function handleCancelInspection() {
    if (!inspecting || cancellingInspection) return;
    setCancellingInspection(true);
    try {
      await cancelJobRequest();
    } catch (cause) {
      setCancellingInspection(false);
      setError(friendlyError(cause));
    }
  }

  async function handleDeleteHistory(ids: number[]) {
    if (operationActive || ids.length === 0) return;
    setHistoryOperation("deleting");
    setError(null);
    try {
      await deleteHistoryRequest(ids);
      if (result && ids.includes(result.historyId)) {
        setResult(null);
        setSearchQuery("");
        setCopied(false);
      }
      await refreshAfterSuccessfulOperation();
    } catch (cause) {
      const message = friendlyError(cause);
      await refreshHistoryAfterMutationFailure();
      setError(message);
      throw cause;
    } finally {
      setHistoryOperation(null);
    }
  }

  async function handleExport(kind: "txt" | "srt" | "vtt") {
    if (operationActive || !result) return;
    setHistoryOperation("exporting");
    try {
      await exportTranscriptFile(result, kind);
    } catch (cause) {
      setError(friendlyError(cause));
    } finally {
      setHistoryOperation(null);
    }
  }

  async function handleLoadMoreHistory() {
    if (operationActive || loadingMoreHistory || !historyHasMore) return;
    setHistoryOperation("reading");
    setLoadingMoreHistory(true);
    setError(null);
    try {
      const page = await listHistory(history.at(-1)?.id ?? null);
      setHistory((previous) => {
        const known = new Set(previous.map((item) => item.id));
        return [...previous, ...page.items.filter((item) => !known.has(item.id))];
      });
      setHistoryHasMore(page.hasMore);
      setHistoryTotalCount(page.totalCount);
    } catch (cause) {
      setError(friendlyError(cause));
    } finally {
      setLoadingMoreHistory(false);
      setHistoryOperation(null);
    }
  }

  async function handleRevealAudio() {
    if (operationActive || !result?.audioPath) return;
    setHistoryOperation("revealing");
    try {
      await revealAudioFile(result.audioPath);
    } catch (cause) {
      setError(friendlyError(cause));
    } finally {
      setHistoryOperation(null);
    }
  }

  async function copyTranscript() {
    if (operationActive || !result) return;
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
    selectCookiesFile: handleSelectCookiesFile,
    clearCookiesFile: handleClearCookiesFile,
    backend,
    computeTargetId,
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
    historyTotalCount,
    historyHasMore,
    loadingMoreHistory,
    result,
    searchQuery,
    setSearchQuery,
    progress,
    busy,
    operationActive,
    resettingData,
    historyOperation,
    inspecting,
    cancellingInspection,
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
    modelDownloadBlocked,
    modelDownloadBlockReasons,
    vramWarning,
    acceleratorWarning,
    refreshSystem: refreshSystemFromUi,
    inspectVideo,
    cancelInspection: handleCancelInspection,
    downloadModel: handleDownloadModel,
    removeModel: handleRemoveModel,
    resetUserData: handleResetUserData,
    installCuda: handleInstallCuda,
    installAccelerator: handleInstallAccelerator,
    installingAccelerator,
    acceleratorDownloadPercent,
    networkSpeedBytesPerSecond,
    appUpdate,
    appUpdateStatus,
    appUpdateProgress,
    appUpdateError,
    clearAppUpdateError,
    checkForAppUpdate: handleCheckForAppUpdate,
    installAppUpdate: handleInstallAppUpdate,
    startTranscription: handleStartTranscription,
    setComputeTarget: handleComputeTargetChange,
    cancelJob: handleCancelJob,
    loadHistory: handleLoadHistory,
    loadMoreHistory: handleLoadMoreHistory,
    deleteHistory: handleDeleteHistory,
    exportFile: handleExport,
    revealAudio: handleRevealAudio,
    copyTranscript,
  };
}
