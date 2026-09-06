import { useState } from "react";
import { AppUpdateBanner } from "./components/common/AppUpdateBanner";
import { AppShell } from "./components/layout/AppShell";
import { ErrorAlert } from "./components/common/ErrorAlert";
import { HistoryPage } from "./components/history/HistoryPage";
import { SettingsPage } from "./components/settings/SettingsPage";
import { TranscribePage } from "./components/transcribe/TranscribePage";
import { useWhisperTube } from "./hooks/useWhisperTube";
import { friendlyError } from "./lib/format";
import { openExternalUrl } from "./services/tauri";

export default function App() {
  const app = useWhisperTube();
  const [dismissedUpdateVersion, setDismissedUpdateVersion] = useState<string | null>(null);
  const showUpdateBanner = Boolean(
    app.appUpdate &&
    app.appUpdateStatus !== "up-to-date" &&
    dismissedUpdateVersion !== app.appUpdate.version,
  );

  return (
    <AppShell
      tab={app.tab}
      historyCount={app.history.length}
      runtimeReady={app.runtimeReady}
      system={app.system}
      onTabChange={app.setTab}
    >
      <ErrorAlert message={app.error} onDismiss={() => app.setError(null)} />
      {showUpdateBanner && app.appUpdate && (
        <AppUpdateBanner
          update={app.appUpdate}
          status={app.appUpdateStatus}
          progress={app.appUpdateProgress}
          busy={app.busy}
          onInstall={app.installAppUpdate}
          onDismiss={() => setDismissedUpdateVersion(app.appUpdate?.version ?? null)}
        />
      )}

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
          networkSpeedBytesPerSecond={app.networkSpeedBytesPerSecond}
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
          onRevealAudio={app.revealAudio}
          onSearchChange={app.setSearchQuery}
        />
      )}
      {app.tab === "history" && (
        <HistoryPage
          history={app.history}
          onRefresh={app.refreshSystem}
          onLoad={app.loadHistory}
          onDelete={app.deleteHistory}
          onTabChange={app.setTab}
        />
      )}
      {app.tab === "settings" && (
        <SettingsPage
          cookiesPath={app.cookiesPath}
          usingSafariSession={app.usingSafariSession}
          browsers={app.browsers}
          system={app.system}
          models={app.models}
          busy={app.busy}
          downloadingModel={app.downloadingModel}
          accelerators={app.system?.accelerators ?? []}
          installingCuda={app.installingCuda}
          cudaDownloadPercent={app.cudaDownloadPercent}
          installingAccelerator={app.installingAccelerator}
          acceleratorDownloadPercent={app.acceleratorDownloadPercent}
          networkSpeedBytesPerSecond={app.networkSpeedBytesPerSecond}
          appUpdate={app.appUpdate}
          appUpdateStatus={app.appUpdateStatus}
          appUpdateProgress={app.appUpdateProgress}
          appUpdateError={app.appUpdateError}
          onSelectCookiesFile={app.selectCookiesFile}
          onClearCookiesFile={app.clearCookiesFile}
          onUseSafariSession={app.useSafariSession}
          onOpenUrl={(url) => openExternalUrl(url).catch((cause) => app.setError(friendlyError(cause)))}
          onDownloadModel={app.downloadModel}
          onCancelModel={app.cancelJob}
          onRemoveModel={app.removeModel}
          onRefresh={app.refreshSystem}
          onInstallCuda={app.installCuda}
          onCancelCuda={app.cancelJob}
          onInstallAccelerator={app.installAccelerator}
          onCancelAccelerator={app.cancelJob}
          onCheckForUpdate={app.checkForAppUpdate}
          onInstallAppUpdate={app.installAppUpdate}
        />
      )}
    </AppShell>
  );
}
