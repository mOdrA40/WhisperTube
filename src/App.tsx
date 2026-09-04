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
          hasResult={Boolean(app.result)}
          progress={app.progress}
          selectedModel={app.selectedModel}
          backend={app.backend}
          system={app.system}
          models={app.models}
          modelId={app.modelId}
          language={app.language}
          keepAudio={app.keepAudio}
          canStart={app.canStart}
          runtimeReady={app.runtimeReady}
          downloadingModel={app.downloadingModel}
          accelerators={app.system?.accelerators ?? []}
          installingCuda={app.installingCuda}
          cudaDownloadPercent={app.cudaDownloadPercent}
          installingAccelerator={app.installingAccelerator}
          acceleratorDownloadPercent={app.acceleratorDownloadPercent}
          vramWarning={app.vramWarning}
          acceleratorWarning={app.acceleratorWarning}
          result={app.result}
          copied={app.copied}
          searchQuery={app.searchQuery}
          filteredSegments={app.filteredSegments}
          onTabChange={app.setTab}
          onUrlChange={app.setUrl}
          onInspect={app.inspectVideo}
          onClear={app.clearTranscription}
          onCancel={app.cancelJob}
          onModelChange={app.setModelId}
          onBackendChange={app.setBackend}
          onLanguageChange={app.setLanguage}
          onKeepAudioChange={app.setKeepAudio}
          onDownloadModel={app.downloadModel}
          onCancelModel={app.cancelJob}
          onInstallCuda={app.installCuda}
          onCancelCuda={app.cancelJob}
          onInstallAccelerator={app.installAccelerator}
          onCancelAccelerator={app.cancelJob}
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
          onDelete={app.deleteHistory}
          onError={app.setError}
          onTabChange={app.setTab}
        />
      )}
      {app.tab === "settings" && (
        <SettingsPage
          browser={app.browser}
          browsers={app.browsers}
          browserProfile={app.browserProfile}
          system={app.system}
          models={app.models}
          busy={app.busy}
          downloadingModel={app.downloadingModel}
          accelerators={app.system?.accelerators ?? []}
          installingCuda={app.installingCuda}
          cudaDownloadPercent={app.cudaDownloadPercent}
          installingAccelerator={app.installingAccelerator}
          acceleratorDownloadPercent={app.acceleratorDownloadPercent}
          onBrowserChange={app.setBrowser}
          onBrowserProfileChange={app.setBrowserProfile}
          onDownloadModel={app.downloadModel}
          onCancelModel={app.cancelJob}
          onRemoveModel={app.removeModel}
          onRefresh={app.refreshSystem}
          onInstallCuda={app.installCuda}
          onCancelCuda={app.cancelJob}
          onInstallAccelerator={app.installAccelerator}
          onCancelAccelerator={app.cancelJob}
        />
      )}
    </AppShell>
  );
}
