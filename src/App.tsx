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
  async function handleManualUpdateCheck() {
    setDismissedUpdateVersion(null);
    await app.checkForAppUpdate();
  }

  return (
    <AppShell
      tab={app.tab}
      historyCount={app.historyTotalCount}
      runtimeReady={app.runtimeReady}
      systemLoading={app.systemLoading}
      system={app.system}
      onTabChange={app.setTab}
    >
      <ErrorAlert
        message={app.error ?? (!showUpdateBanner ? app.appUpdateError : null)}
        onDismiss={() => {
          app.setError(null);
          app.clearAppUpdateError();
        }}
      />
      {showUpdateBanner && app.appUpdate && app.tab !== "settings" && (
        <AppUpdateBanner
          update={app.appUpdate}
          status={app.appUpdateStatus}
          progress={app.appUpdateProgress}
          error={app.appUpdateError}
          busy={app.operationActive}
          onInstall={app.installAppUpdate}
          onDismiss={() => setDismissedUpdateVersion(app.appUpdate?.version ?? null)}
        />
      )}

      {app.tab === "transcribe" && (
        <TranscribePage
          url={app.url}
          busy={app.busy}
          operationActive={app.operationActive}
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
          modelDownloadBlocked={app.modelDownloadBlocked}
          runtimeReady={app.runtimeReady}
          computeTargetId={app.computeTargetId}
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
          onCancelInspection={app.cancelInspection}
          onCancelTranscription={app.cancelJob}
          cancellingInspection={app.cancellingInspection}
          onModelChange={app.setModelId}
          onComputeTargetChange={app.setComputeTarget}
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
          hasMore={app.historyHasMore}
          loadingMore={app.loadingMoreHistory}
          operationActive={app.operationActive}
          onRefresh={app.refreshSystem}
          onLoadMore={app.loadMoreHistory}
          onLoad={app.loadHistory}
          onDelete={app.deleteHistory}
          onTabChange={app.setTab}
        />
      )}
      {app.tab === "settings" && (
        <SettingsPage
          cookiesPath={app.cookiesPath}
          system={app.system}
          models={app.models}
          modelDownloadBlockReasons={app.modelDownloadBlockReasons}
          busy={app.operationActive}
          resettingData={app.resettingData}
          historyTotalCount={app.historyTotalCount}
          downloadingModel={app.downloadingModel}
          accelerators={app.system?.accelerators ?? []}
          installingCuda={app.installingCuda}
          cudaDownloadPercent={app.cudaDownloadPercent}
          installingAccelerator={app.installingAccelerator}
          acceleratorDownloadPercent={app.acceleratorDownloadPercent}
          networkSpeedBytesPerSecond={app.networkSpeedBytesPerSecond}
          updateChecking={app.appUpdateStatus === "checking"}
          appUpdate={app.appUpdate}
          appUpdateStatus={app.appUpdateStatus}
          appUpdateProgress={app.appUpdateProgress}
          appUpdateError={app.appUpdateError}
          onSelectCookiesFile={app.selectCookiesFile}
          onClearCookiesFile={app.clearCookiesFile}
          onOpenUrl={(url) => openExternalUrl(url).catch((cause) => app.setError(friendlyError(cause)))}
          onDownloadModel={app.downloadModel}
          onCancelModel={app.cancelJob}
          onRemoveModel={app.removeModel}
          onResetUserData={app.resetUserData}
          onRefresh={app.refreshSystem}
          onInstallCuda={app.installCuda}
          onCancelCuda={app.cancelJob}
          onInstallAccelerator={app.installAccelerator}
          onCancelAccelerator={app.cancelJob}
          onCheckForUpdate={handleManualUpdateCheck}
          onInstallAppUpdate={app.installAppUpdate}
        />
      )}
    </AppShell>
  );
}
