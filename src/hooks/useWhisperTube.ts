import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  cancelJob as cancelJobRequest,
  deleteModel,
  downloadModel,
  exportTranscriptFile,
  getSystemStatus,
  inspectYoutube,
  listHistory,
  listModels,
  loadHistory,
  startTranscription,
  subscribeToModelDownload,
  subscribeToProgress,
} from "../services/tauri";
import { friendlyError } from "../lib/format";
import type {
  AppTab,
  BackendChoice,
  BrowserChoice,
  HistoryItem,
  ModelInfo,
  ProgressPayload,
  SystemStatus,
  TranscriptResult,
  VideoMetadata,
} from "../types";

const initialProgress: ProgressPayload = { stage: "idle", percent: 0, message: "" };

export function useWhisperTube() {
  const [tab, setTab] = useState<AppTab>("transcribe");
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
  const [progress, setProgress] = useState<ProgressPayload>(initialProgress);
  const [busy, setBusy] = useState(false);
  const [inspecting, setInspecting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [downloadingModel, setDownloadingModel] = useState<Record<string, number>>({});
  const [copied, setCopied] = useState(false);
  const autoConfigured = useRef(false);

  const selectedModel = models.find((model) => model.id === modelId);
  const runtimeReady = Boolean(system?.ytDlp && system?.ffmpeg && system?.cpuEngine);

  const refreshSystem = useCallback(async () => {
    const [nextSystem, nextModels, nextHistory] = await Promise.all([
      getSystemStatus(),
      listModels(),
      listHistory(),
    ]);
    setSystem(nextSystem);
    if (!autoConfigured.current) {
      setModelId(nextSystem.recommendedModelId);
      setBackend(nextSystem.recommendedBackend);
      autoConfigured.current = true;
    }
    setModels(nextModels);
    setHistory(nextHistory);
  }, []);

  useEffect(() => {
    refreshSystem().catch((cause) => setError(friendlyError(cause)));

    const unlistenProgress = subscribeToProgress(setProgress);
    const unlistenModel = subscribeToModelDownload(({ id, percent }) => {
      setDownloadingModel((previous) => ({ ...previous, [id]: percent }));
    });

    return () => {
      unlistenProgress.then((unlisten) => unlisten());
      unlistenModel.then((unlisten) => unlisten());
    };
  }, [refreshSystem]);

  function handleUrlChange(nextUrl: string) {
    setUrl(nextUrl);
    setMetadata(null);
  }

  const filteredSegments = useMemo(() => {
    if (!result) return [];
    const query = searchQuery.trim().toLowerCase();
    if (!query) return result.segments;
    return result.segments.filter((segment) => segment.text.toLowerCase().includes(query));
  }, [result, searchQuery]);

  async function inspectVideo() {
    if (!url.trim()) {
      setError("Tempel URL YouTube terlebih dahulu.");
      return;
    }
    setInspecting(true);
    setError(null);
    setResult(null);
    try {
      setMetadata(await inspectYoutube(url.trim(), browser));
    } catch (cause) {
      setMetadata(null);
      setError(friendlyError(cause));
    } finally {
      setInspecting(false);
    }
  }

  async function handleDownloadModel(id: string) {
    setError(null);
    setDownloadingModel((previous) => ({ ...previous, [id]: 0 }));
    try {
      await downloadModel(id);
      await refreshSystem();
    } catch (cause) {
      setError(friendlyError(cause));
    } finally {
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
      await refreshSystem();
    } catch (cause) {
      setError(friendlyError(cause));
    }
  }

  async function handleStartTranscription() {
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
      setResult(await startTranscription({
        url: metadata.webpageUrl,
        title: metadata.title,
        channel: metadata.channel,
        duration: metadata.duration,
        browser,
        backend,
        language,
        modelId,
        keepAudio,
      }));
      setSearchQuery("");
      await refreshSystem();
    } catch (cause) {
      const message = friendlyError(cause);
      if (!message.toLowerCase().includes("dibatalkan")) setError(message);
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

  async function handleExport(kind: "txt" | "srt" | "vtt") {
    if (!result) return;
    try {
      await exportTranscriptFile(result, kind);
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
    browser,
    setBrowser,
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
    selectedModel,
    runtimeReady,
    filteredSegments,
    refreshSystem,
    inspectVideo,
    downloadModel: handleDownloadModel,
    removeModel: handleRemoveModel,
    startTranscription: handleStartTranscription,
    cancelJob: handleCancelJob,
    loadHistory: handleLoadHistory,
    exportFile: handleExport,
    copyTranscript,
  };
}
