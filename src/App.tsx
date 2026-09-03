import { AppShell } from "./components/layout/AppShell";
import { ErrorAlert } from "./components/common/ErrorAlert";
import { HistoryPage } from "./components/history/HistoryPage";
import { SettingsPage } from "./components/settings/SettingsPage";
import { TranscribePage } from "./components/transcribe/TranscribePage";
import { useWhisperTube } from "./hooks/useWhisperTube";

export default function App() {
  const app = useWhisperTube();

  return (
    <AppShell
      tab={app.tab}
      historyCount={app.history.length}
      runtimeReady={app.runtimeReady}
      system={app.system}
      onTabChange={app.setTab}
    >
      <ErrorAlert message={app.error} onDismiss={() => app.setError(null)} />

      {app.tab === "transcribe" && (
        <TranscribePage
          url={app.url}
          busy={app.busy}
          inspecting={app.inspecting}
          metadata={app.metadata}
          progress={app.progress}
          selectedModel={app.selectedModel}
          backend={app.backend}
          system={app.system}
          models={app.models}
          modelId={app.modelId}
          language={app.language}
          keepAudio={app.keepAudio}
          runtimeReady={app.runtimeReady}
          downloadingModel={app.downloadingModel}
          result={app.result}
          copied={app.copied}
          searchQuery={app.searchQuery}
          filteredSegments={app.filteredSegments}
          onTabChange={app.setTab}
          onUrlChange={app.setUrl}
          onInspect={app.inspectVideo}
          onCancel={app.cancelJob}
          onModelChange={app.setModelId}
          onBackendChange={app.setBackend}
          onLanguageChange={app.setLanguage}
          onKeepAudioChange={app.setKeepAudio}
          onDownloadModel={app.downloadModel}
          onStart={app.startTranscription}
          onCopy={app.copyTranscript}
          onExport={app.exportFile}
          onSearchChange={app.setSearchQuery}
        />
      )}
      {app.tab === "history" && (
        <HistoryPage
          history={app.history}
          onRefresh={app.refreshSystem}
          onLoad={app.loadHistory}
          onTabChange={app.setTab}
        />
      )}
      {app.tab === "settings" && (
        <SettingsPage
          browser={app.browser}
          system={app.system}
          models={app.models}
          busy={app.busy}
          downloadingModel={app.downloadingModel}
          onBrowserChange={app.setBrowser}
          onDownloadModel={app.downloadModel}
          onRemoveModel={app.removeModel}
          onRefresh={app.refreshSystem}
        />
      )}
    </AppShell>
  );
}
