import { AlertTriangle, Check, CircleStop, Copy, Cpu, Database, Download, ExternalLink, FileKey, HardDrive, KeyRound, Languages, LockKeyhole, LoaderCircle, MonitorCog, RotateCcw, Trash2 } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { formatBytes, formatMemory, friendlyError } from "../../lib/format";
import { getAcceleratorCopy, getModelCopy, uiLanguageOptions, useI18n } from "../../i18n";
import type { AcceleratorInfo, AppUpdateInfo, AppUpdateProgress, AppUpdateStatus, BackendChoice, ModelDownloadPayload, ModelInfo, SystemStatus } from "../../types";
import { CustomSelect, type SelectOption } from "../common/CustomSelect";

const COOKIES_EXTENSION_URL = "https://chromewebstore.google.com/detail/get-cookiestxt-locally/cclelndahbckbenkjhflpdbgdldlbecc";
const FIREFOX_COOKIES_EXTENSION_URL = "https://addons.mozilla.org/en-US/firefox/addon/get-cookies-txt-locally/";

type SettingsPageProps = {
  cookiesPath: string;
  system: SystemStatus | null;
  models: ModelInfo[];
  modelDownloadBlockReasons: Record<string, string>;
  busy: boolean;
  resettingData: boolean;
  historyTotalCount: number;
  downloadingModel: Record<string, ModelDownloadPayload>;
  accelerators: AcceleratorInfo[];
  installingCuda: boolean;
  cudaDownloadPercent: number;
  installingAccelerator: Exclude<BackendChoice, "auto" | "cpu" | "cuda"> | null;
  acceleratorDownloadPercent: number;
  networkSpeedBytesPerSecond: number | null;
  appUpdate: AppUpdateInfo | null;
  appUpdateStatus: AppUpdateStatus;
  appUpdateProgress: AppUpdateProgress;
  appUpdateError: string | null;
  updateChecking: boolean;
  onSelectCookiesFile: () => void;
  onClearCookiesFile: () => void;
  onOpenUrl: (url: string) => void;
  onDownloadModel: (id: string) => void;
  onCancelModel: () => void;
  onRemoveModel: (id: string) => Promise<void>;
  onResetUserData: () => Promise<void>;
  onRefresh: () => void;
  onInstallCuda: () => void;
  onCancelCuda: () => void;
  onInstallAccelerator: (backend: Exclude<BackendChoice, "auto" | "cpu" | "cuda">) => void;
  onCancelAccelerator: () => void;
  onCheckForUpdate: () => void;
  onInstallAppUpdate: () => void;
};

export function SettingsPage({
  cookiesPath,
  system,
  models,
  modelDownloadBlockReasons,
  busy,
  resettingData,
  historyTotalCount,
  downloadingModel,
  accelerators,
  installingCuda,
  cudaDownloadPercent,
  installingAccelerator,
  acceleratorDownloadPercent,
  networkSpeedBytesPerSecond,
  appUpdate,
  appUpdateStatus,
  appUpdateProgress,
  appUpdateError,
  updateChecking,
  onSelectCookiesFile,
  onClearCookiesFile,
  onOpenUrl,
  onDownloadModel,
  onCancelModel,
  onRemoveModel,
  onResetUserData,
  onRefresh,
  onInstallCuda,
  onCancelCuda,
  onInstallAccelerator,
  onCancelAccelerator,
  onCheckForUpdate,
  onInstallAppUpdate,
}: SettingsPageProps) {
  const { language: uiLanguage, setLanguage: setUiLanguage, t } = useI18n();
  const [cookiesDialogOpen, setCookiesDialogOpen] = useState(false);
  const [resetDialogOpen, setResetDialogOpen] = useState(false);
  const [pendingModelDelete, setPendingModelDelete] = useState<ModelInfo | null>(null);
  const [copiedUrl, setCopiedUrl] = useState<string | null>(null);
  const [copyFailedUrl, setCopyFailedUrl] = useState<string | null>(null);
  const [resetComplete, setResetComplete] = useState(false);
  const [resetError, setResetError] = useState<string | null>(null);
  const [modelDeleteError, setModelDeleteError] = useState<string | null>(null);
  const cookiesDialogRef = useRef<HTMLDivElement>(null);
  const cookiesTriggerRef = useRef<HTMLButtonElement>(null);
  const cookiesContinueRef = useRef<HTMLButtonElement>(null);
  const cookiesDialogWasOpen = useRef(false);
  const resetDialogRef = useRef<HTMLDivElement>(null);
  const resetTriggerRef = useRef<HTMLButtonElement>(null);
  const resetCancelRef = useRef<HTMLButtonElement>(null);
  const resetDialogWasOpen = useRef(false);
  const modelDeleteDialogRef = useRef<HTMLDivElement>(null);
  const modelDeleteTriggerRef = useRef<HTMLButtonElement | null>(null);
  const modelDeleteCancelRef = useRef<HTMLButtonElement>(null);
  const modelDeleteDialogWasOpen = useRef(false);
  const modelDownloadActive = Object.keys(downloadingModel).length > 0;
  const interfaceLanguageOptions: SelectOption[] = uiLanguageOptions.map((option) => ({
    value: option.value,
    label: option.label,
  }));

  async function copyLink(url: string) {
    try {
      await navigator.clipboard.writeText(url);
      setCopyFailedUrl(null);
      setCopiedUrl(url);
      window.setTimeout(() => setCopiedUrl((current) => (current === url ? null : current)), 1600);
    } catch {
      setCopiedUrl(null);
      setCopyFailedUrl(url);
    }
  }

  const installedModels = models.filter((model) => model.installed);
  const installedAccelerators = accelerators.filter((accelerator) => accelerator.installed);
  const installedRuntimes = [
    ...(system?.cudaEngine ? [t("settings.cudaEngine")] : []),
    ...installedAccelerators.map((accelerator) => getAcceleratorCopy(accelerator, t).label),
  ];
  const hasSavedAccessPreference = Boolean(cookiesPath);

  async function handleResetUserData() {
    setResetComplete(false);
    setResetError(null);
    try {
      await onResetUserData();
      setResetDialogOpen(false);
      setResetComplete(true);
      window.setTimeout(() => setResetComplete(false), 2400);
    } catch (cause) {
      setResetError(friendlyError(cause));
    }
  }

  useEffect(() => {
    if (!cookiesDialogOpen) return;
    cookiesDialogWasOpen.current = true;
    cookiesContinueRef.current?.focus();
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setCookiesDialogOpen(false);
        return;
      }
      if (event.key !== "Tab" || !cookiesDialogRef.current) return;
      const focusable = [...cookiesDialogRef.current.querySelectorAll<HTMLElement>(
        "button, a[href], input, select, textarea, [tabindex]:not([tabindex='-1'])",
      )].filter((element) => !element.hasAttribute("disabled"));
      if (focusable.length === 0) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [cookiesDialogOpen]);

  useEffect(() => {
    if (!cookiesDialogOpen && cookiesDialogWasOpen.current) {
      cookiesDialogWasOpen.current = false;
      cookiesTriggerRef.current?.focus();
    }
  }, [cookiesDialogOpen]);

  useEffect(() => {
    if (!resetDialogOpen) return;
    resetDialogWasOpen.current = true;
    resetCancelRef.current?.focus();
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !resettingData) {
        setResetDialogOpen(false);
        return;
      }
      if (event.key !== "Tab" || !resetDialogRef.current) return;
      const focusable = [...resetDialogRef.current.querySelectorAll<HTMLElement>(
        "button, a[href], input, select, textarea, [tabindex]:not([tabindex='-1'])",
      )].filter((element) => !element.hasAttribute("disabled"));
      if (focusable.length === 0) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [resetDialogOpen, resettingData]);

  useEffect(() => {
    if (!resetDialogOpen && resetDialogWasOpen.current) {
      resetDialogWasOpen.current = false;
      resetTriggerRef.current?.focus();
    }
  }, [resetDialogOpen]);

  useEffect(() => {
    if (!pendingModelDelete) return;
    modelDeleteDialogWasOpen.current = true;
    modelDeleteCancelRef.current?.focus();
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !busy) {
        setPendingModelDelete(null);
        return;
      }
      if (event.key !== "Tab" || !modelDeleteDialogRef.current) return;
      const focusable = [...modelDeleteDialogRef.current.querySelectorAll<HTMLElement>(
        "button, a[href], input, select, textarea, [tabindex]:not([tabindex='-1'])",
      )].filter((element) => !element.hasAttribute("disabled"));
      if (focusable.length === 0) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [busy, pendingModelDelete]);

  useEffect(() => {
    if (!pendingModelDelete && modelDeleteDialogWasOpen.current) {
      modelDeleteDialogWasOpen.current = false;
      modelDeleteTriggerRef.current?.focus();
    }
  }, [pendingModelDelete]);

  async function confirmModelDelete() {
    const model = pendingModelDelete;
    if (!model) return;
    setModelDeleteError(null);
    try {
      await onRemoveModel(model.id);
      setPendingModelDelete(null);
    } catch (cause) {
      setModelDeleteError(friendlyError(cause));
    }
  }

  return (
    <div className="settings-grid">
      <div className="settings-inline-toolbar">
        <div className="settings-update-copy">
          <span>{t("update.description")}</span>
          {appUpdate && appUpdateStatus !== "up-to-date" && (
            <strong>{t("update.available", { version: appUpdate.version })}</strong>
          )}
          {appUpdateError && <small className="settings-error">{appUpdateError}</small>}
        </div>
        <div className="settings-inline-actions">
          <button
            type="button"
            className="secondary-button compact"
            onClick={onCheckForUpdate}
            disabled={updateChecking || appUpdateStatus === "installing" || busy}
          >
            {updateChecking ? <LoaderCircle className="spin" size={15} /> : <RotateCcw size={15} />}
            {updateChecking ? t("update.checking") : t("update.check")}
          </button>
          {appUpdate && appUpdateStatus === "available" && (
            <button
              type="button"
              className="primary-button compact"
              onClick={onInstallAppUpdate}
              disabled={busy}
            >
              <Download size={15} /> {t("update.install")}
            </button>
          )}
          {appUpdateStatus === "installing" && (
            <span className="settings-update-progress">
              {t("update.installing", { percent: Math.round(appUpdateProgress.percent) })}
            </span>
          )}
        </div>
      </div>
      <section className="card settings-card settings-access-card">
        <div className="card-title-row">
          <div>
            <span className="eyebrow">{t("settings.eyebrow")}</span>
            <h3>{t("settings.accessTitle")}</h3>
          </div>
          <LockKeyhole size={20} />
        </div>
        <label className="field-label">{t("settings.cookiesFile")}</label>
        {cookiesPath ? (
          <div className="info-box">
            <FileKey size={16} />
            <span title={cookiesPath}>
              {t("settings.cookiesFileSelected", {
                file: cookiesPath.split(/[\\/]/).pop() ?? cookiesPath,
              })}
            </span>
            <button
              type="button"
              className="icon-button danger"
              onClick={onClearCookiesFile}
              disabled={busy}
              title={t("settings.clearCookiesFile")}
              aria-label={t("settings.clearCookiesFile")}
            >
              <Trash2 size={15} />
            </button>
          </div>
        ) : (
            <button
              type="button"
              className="secondary-button full"
              ref={cookiesTriggerRef}
              onClick={() => setCookiesDialogOpen(true)}
            disabled={busy}
          >
            <FileKey size={16} />
            <span>{t("settings.chooseCookiesFile")}</span>
          </button>
        )}
        <p className="settings-note">{t("settings.cookiesFileNote")}</p>

        <label className="field-label">{t("settings.interfaceLanguage")}</label>
        <CustomSelect
          value={uiLanguage}
          options={interfaceLanguageOptions}
          onChange={(value) => setUiLanguage(value as typeof uiLanguage)}
          disabled={busy}
          ariaLabel={t("settings.interfaceLanguage")}
        />
        <p className="settings-note">{t("settings.interfaceLanguageNote")}</p>
      </section>

      {cookiesDialogOpen && (
        <div className="modal-backdrop" role="presentation">
          <div
            className="cookies-guide-modal"
            ref={cookiesDialogRef}
            role="dialog"
            aria-modal="true"
            aria-labelledby="cookies-guide-title"
          >
            <div className="cookies-guide-icon" aria-hidden="true">
              <FileKey size={22} />
            </div>
            <div className="cookies-guide-content">
              <h3 id="cookies-guide-title">{t("settings.cookiesGuideTitle")}</h3>
              <p>{t("settings.cookiesGuideBody")}</p>
              <p className="cookies-guide-extension">{t("settings.cookiesGuideExtension")}</p>
              <div className="cookies-guide-links">
                <CookieExtensionLink
                    label={t("settings.cookiesGuideChromiumFamily")}
                    url={COOKIES_EXTENSION_URL}
                    copied={copiedUrl === COOKIES_EXTENSION_URL}
                    copyFailed={copyFailedUrl === COOKIES_EXTENSION_URL}
                    onOpen={onOpenUrl}
                    onCopy={copyLink}
                    openLabel={t("settings.openLink")}
                    copyLabel={t("settings.copyLink")}
                    copiedLabel={t("settings.linkCopied")}
                    copyFailedLabel={t("settings.copyLinkFailed")}
                />
                <CookieExtensionLink
                    label={t("settings.cookiesGuideFirefoxFamily")}
                    url={FIREFOX_COOKIES_EXTENSION_URL}
                    copied={copiedUrl === FIREFOX_COOKIES_EXTENSION_URL}
                    copyFailed={copyFailedUrl === FIREFOX_COOKIES_EXTENSION_URL}
                    onOpen={onOpenUrl}
                    onCopy={copyLink}
                    openLabel={t("settings.openLink")}
                    copyLabel={t("settings.copyLink")}
                    copiedLabel={t("settings.linkCopied")}
                    copyFailedLabel={t("settings.copyLinkFailed")}
                />
              </div>
              <ol className="cookies-guide-steps">
                <li>{t("settings.cookiesGuideStep1")}</li>
                <li>{t("settings.cookiesGuideStep2")}</li>
                <li>{t("settings.cookiesGuideStep3")}</li>
              </ol>
              <div className="cookies-guide-actions">
                <button
                  type="button"
                  className="secondary-button"
                  onClick={() => setCookiesDialogOpen(false)}
                >
                  {t("settings.cookiesGuideCancel")}
                </button>
                <button
                  ref={cookiesContinueRef}
                  type="button"
                  className="primary-button"
                  onClick={() => {
                    setCookiesDialogOpen(false);
                    onSelectCookiesFile();
                  }}
                >
                  <FileKey size={15} />
                  {t("settings.cookiesGuideContinue")}
                </button>
              </div>
            </div>
          </div>
        </div>
      )}

      {resetDialogOpen && (
        <div
          className="modal-backdrop"
          role="presentation"
          onMouseDown={(event) => {
            if (event.target === event.currentTarget && !resettingData) setResetDialogOpen(false);
          }}
        >
          <div
            className="data-reset-modal"
            ref={resetDialogRef}
            role="dialog"
            aria-modal="true"
            aria-labelledby="data-reset-title"
            aria-describedby={resetError ? "data-reset-description data-reset-error" : "data-reset-description"}
          >
            <div className="data-reset-icon" aria-hidden="true">
              <AlertTriangle size={22} />
            </div>
            <div className="data-reset-content">
              <span className="eyebrow">{t("settings.dataEyebrow")}</span>
              <h3 id="data-reset-title">{t("settings.resetDataDialogTitle")}</h3>
              <p id="data-reset-description">{t("settings.resetDataDialogIntro")}</p>

              <div className="data-reset-section">
                <strong className="data-reset-section-title">{t("settings.resetDataWillDelete")}</strong>
                <div className="data-reset-list">
                  <ResetDataItem
                    icon={<Database size={16} />}
                    title={t("settings.resetDataHistory")}
                    detail={t("settings.resetDataHistoryDetail", { count: historyTotalCount })}
                  />
                  <ResetDataItem
                    icon={<HardDrive size={16} />}
                    title={t("settings.resetDataModels")}
                    detail={
                      installedModels.length > 0
                        ? installedModels.map((model) => getModelCopy(model, t).label).join(", ")
                        : t("settings.resetDataNoneInstalled")
                    }
                  />
                  <ResetDataItem
                    icon={<HardDrive size={16} />}
                    title={t("settings.resetDataRuntimes")}
                    detail={
                      system
                        ? installedRuntimes.length > 0
                          ? installedRuntimes.join(", ")
                          : t("settings.resetDataNoneInstalled")
                        : t("settings.resetDataStatusUnknown")
                    }
                  />
                  <ResetDataItem
                    icon={<KeyRound size={16} />}
                    title={t("settings.resetDataAccess")}
                    detail={
                      hasSavedAccessPreference
                        ? t("settings.resetDataAccessDetail")
                        : t("settings.resetDataAccessNone")
                    }
                  />
                </div>
              </div>

              <div className="data-reset-keep">
                <div className="data-reset-keep-heading">
                  <Languages size={15} />
                  <strong>{t("settings.resetDataWillKeep")}</strong>
                </div>
                <p>{t("settings.resetDataKeepDetail")}</p>
              </div>

              <div className="data-reset-warning">
                <AlertTriangle size={15} />
                <span>{t("settings.resetDataWarning")}</span>
              </div>

              {resetError && (
                <div id="data-reset-error" className="data-reset-error" role="alert">
                  <AlertTriangle size={15} />
                  <div>
                    <strong>{t("error.title")}</strong>
                    <p>{resetError}</p>
                  </div>
                </div>
              )}

              <div className="data-reset-actions">
                <button
                  ref={resetCancelRef}
                  type="button"
                  className="secondary-button"
                  onClick={() => setResetDialogOpen(false)}
                  disabled={resettingData}
                >
                  {t("settings.resetDataCancel")}
                </button>
                <button
                  type="button"
                  className="danger-button"
                  onClick={() => void handleResetUserData()}
                  disabled={busy || resettingData}
                >
                  {resettingData ? <LoaderCircle className="spin" size={16} /> : <Trash2 size={16} />}
                  {resettingData ? t("settings.resetDataWorking") : t("settings.resetDataConfirmButton")}
                </button>
              </div>
            </div>
          </div>
        </div>
      )}

      {pendingModelDelete && (
        <div
          className="modal-backdrop"
          role="presentation"
          onMouseDown={(event) => {
            if (event.target === event.currentTarget && !busy) setPendingModelDelete(null);
          }}
        >
          <div
            className="model-delete-modal"
            ref={modelDeleteDialogRef}
            role="dialog"
            aria-modal="true"
            aria-labelledby="model-delete-title"
            aria-describedby={modelDeleteError ? "model-delete-description model-delete-error" : "model-delete-description"}
          >
            <div className="model-delete-icon" aria-hidden="true">
              <AlertTriangle size={22} />
            </div>
            <div className="model-delete-content">
              <h3 id="model-delete-title">{t("settings.deleteModelTitle")}</h3>
              <p id="model-delete-description">{t("settings.confirmDeleteModel", { model: getModelCopy(pendingModelDelete, t).label })}</p>
              {modelDeleteError && (
                <div id="model-delete-error" className="data-reset-error" role="alert">
                  <AlertTriangle size={15} />
                  <div>
                    <strong>{t("error.title")}</strong>
                    <p>{modelDeleteError}</p>
                  </div>
                </div>
              )}
              <div className="model-delete-actions">
                <button
                  ref={modelDeleteCancelRef}
                  type="button"
                  className="secondary-button"
                  onClick={() => setPendingModelDelete(null)}
                  disabled={busy}
                >
                  {t("settings.deleteModelCancel")}
                </button>
                <button
                  type="button"
                  className="danger-button"
                  onClick={() => void confirmModelDelete()}
                  disabled={busy}
                >
                  <Trash2 size={16} /> {t("settings.deleteModelConfirm")}
                </button>
              </div>
            </div>
          </div>
        </div>
      )}

      <section className="card settings-card hardware-settings-card">
        <div className="card-title-row">
          <div>
            <span className="eyebrow">{t("settings.hardwareEyebrow")}</span>
            <h3>{t("settings.computeTitle")}</h3>
          </div>
          <Cpu size={20} />
        </div>
        <div className="status-table">
          <div>
            <span>{t("settings.cpuEngine")}</span>
            <strong className={system?.cpuEngine ? "ok-text" : "bad-text"}>
              {system?.cpuEngine ? t("settings.installed") : t("settings.missing")}
            </strong>
          </div>
          <div>
            <span>{t("settings.gpu")}</span>
            <strong title={system?.gpuName ?? undefined}>
              {system?.gpuName ?? t("hardware.notDetected")}
            </strong>
          </div>
          <div>
            <span>{t("settings.computeDevices")}</span>
            <strong
              className="status-device-list"
              aria-label={
                system?.computeDevices.length
                  ? system.computeDevices.map((device) => `${device.id}: ${device.name}`).join(" • ")
                  : t("hardware.notDetected")
              }
            >
              {system?.computeDevices.length
                ? system.computeDevices.map((device) => {
                    const label = `${device.id}: ${device.name}`;
                    return (
                      <span className="status-device" key={device.id} title={label}>
                        {label}
                      </span>
                    );
                  })
                : t("hardware.notDetected")}
            </strong>
          </div>
          <div>
            <span>{t("settings.totalVram")}</span>
            <strong>{formatMemory(system?.gpuMemoryMb, t("hardware.notDetected"))}</strong>
          </div>
          <div>
            <span>{t("settings.freeVram")}</span>
            <strong>{formatMemory(system?.gpuFreeMemoryMb, t("hardware.notDetected"))}</strong>
          </div>
          <div>
            <span>{t("settings.cudaEngine")}</span>
            <strong className={system?.cudaSupported && system.cudaEngine ? "ok-text" : "muted-text"}>
              {system?.cudaSupported && system.cudaEngine
                ? t("settings.installed")
                : system?.cudaSupported
                  ? t("settings.optional")
                : t("settings.unavailable")}
            </strong>
          </div>
          <div>
            <span>{t("hardware.cpuThreads")}</span>
            <strong>{system?.cpuThreads ?? "—"}</strong>
          </div>
          <div>
            <span>{t("settings.jobStorage")}</span>
            <strong>
              {system
                ? `${formatBytes(system.jobStorageBytes)} / ${formatBytes(system.jobStorageLimitBytes)}`
                : t("hardware.notDetected")}
            </strong>
          </div>
        </div>

        {system?.cudaSupported && system.nvidia ? (
          <>
            <div className="info-box">
              <Download size={16} />
              <span>{t("settings.cudaInfo")}</span>
            </div>
            <button
              type="button"
              className="secondary-button full"
              onClick={onInstallCuda}
              disabled={installingCuda || installingAccelerator !== null || modelDownloadActive || system.cudaEngine || busy}
            >
              {installingCuda ? <LoaderCircle className="spin" size={16} /> : <Download size={16} />}
              {installingCuda
                ? (
                  <span className="download-progress-copy">
                    <span>{t("settings.installingCuda", { percent: Math.round(cudaDownloadPercent) })}</span>
                    {networkSpeedBytesPerSecond !== null && <small>{formatBytes(networkSpeedBytesPerSecond)}/s</small>}
                  </span>
                )
                : system.cudaEngine
                  ? t("settings.cudaInstalled")
                  : t("settings.installCuda")}
            </button>
            {installingCuda && (
              <button type="button" className="danger-ghost" onClick={onCancelCuda}>
                <CircleStop size={16} /> {t("settings.cancelCuda")}
              </button>
            )}
          </>
        ) : (
          <p className="settings-note">
            {system?.gpuName && !system.nvidia ? t("settings.cudaUnavailable") : t("settings.noGpu")}
          </p>
        )}

        {accelerators.filter((accelerator) => accelerator.supported).map((accelerator) => {
          const copy = getAcceleratorCopy(accelerator, t);
          return (
          <div className="info-box" key={accelerator.id}>
            <Download size={16} />
            <span>
              {copy.label}: {accelerator.installed ? t("settings.acceleratorInstalled") : copy.description}
            </span>
            {!accelerator.installed && accelerator.downloadable && (
              <button
                type="button"
                className="secondary-button compact"
                onClick={() =>
                  onInstallAccelerator(
                    accelerator.backend as Exclude<BackendChoice, "auto" | "cpu" | "cuda">
                  )
                }
                disabled={installingAccelerator !== null || installingCuda || modelDownloadActive || busy}
              >
                {installingAccelerator === accelerator.backend
                  ? (
                    <span className="download-progress-copy">
                      <span>{t("settings.acceleratorInstalling", { percent: Math.round(acceleratorDownloadPercent) })}</span>
                      {networkSpeedBytesPerSecond !== null && <small>{formatBytes(networkSpeedBytesPerSecond)}/s</small>}
                    </span>
                  )
                  : t("settings.acceleratorInstall")}
              </button>
            )}
          </div>
          );
        })}
        {installingAccelerator && (
          <button type="button" className="danger-ghost" onClick={onCancelAccelerator}>
            <CircleStop size={16} /> {t("controls.cancelAccelerator")}
          </button>
        )}
      </section>

      <section className="card settings-card models-settings">
        <div className="card-title-row">
          <div>
            <span className="eyebrow">{t("settings.storageEyebrow")}</span>
            <h3>{t("settings.modelsTitle")}</h3>
          </div>
          <Download size={20} />
        </div>
        <div className="model-manager">
          {models.map((model) => (
            <div className="model-manage-row" key={model.id}>
              <div>
                <strong>{getModelCopy(model, t).label}</strong>
                <span>
                  {getModelCopy(model, t).description} • {model.sizeMb} MB • {t("controls.memoryEstimate", { memory: formatMemory(model.vramRequiredMb, t("hardware.notDetected")) })}
                </span>
                {modelDownloadBlockReasons[model.id] && (
                  <small className="model-download-block-reason">{modelDownloadBlockReasons[model.id]}</small>
                )}
              </div>
              {downloadingModel[model.id] ? (
                <div className="model-download-status">
                  <span>
                    {t("controls.downloadingModel", {
                      percent: Math.round(downloadingModel[model.id].percent),
                    })}
                  </span>
                  <small>
                    {formatBytes(downloadingModel[model.id].downloadedBytes, t("progress.unknownSize"))} / {downloadingModel[model.id].totalBytes > 0
                      ? formatBytes(downloadingModel[model.id].totalBytes)
                      : t("progress.unknownSize")}
                    {networkSpeedBytesPerSecond !== null && ` • ${formatBytes(networkSpeedBytesPerSecond)}/s`}
                  </small>
                  <button
                    type="button"
                    className="icon-button danger"
                    onClick={onCancelModel}
                    title={t("controls.cancelModel")}
                    aria-label={t("controls.cancelModel")}
                  >
                    <CircleStop size={17} />
                  </button>
                </div>
              ) : model.installed ? (
                <button
                  type="button"
                  className="icon-button danger"
                  onClick={(event) => {
                    modelDeleteTriggerRef.current = event.currentTarget;
                    setModelDeleteError(null);
                    setPendingModelDelete(model);
                  }}
                  disabled={busy || modelDownloadActive || installingCuda || installingAccelerator !== null}
                  title={t("settings.deleteModel")}
                  aria-label={t("settings.deleteModel")}
                >
                  <Trash2 size={17} />
                </button>
              ) : (
                <button
                  type="button"
                  className="secondary-button compact"
                  onClick={() => onDownloadModel(model.id)}
                  disabled={modelDownloadActive || busy || Boolean(modelDownloadBlockReasons[model.id])}
                  title={modelDownloadBlockReasons[model.id]}
                >
                  <Download size={15} /> {t("settings.download")}
                </button>
              )}
            </div>
          ))}
        </div>
      </section>

      <section className="card settings-card runtime-settings-card">
        <div className="card-title-row">
          <div>
            <span className="eyebrow">{t("settings.runtimeEyebrow")}</span>
            <h3>{t("settings.externalTitle")}</h3>
          </div>
          <MonitorCog size={20} />
        </div>
        <div className="status-table">
          <div>
            <span>yt-dlp</span>
            <strong className={system?.ytDlp ? "ok-text" : "bad-text"}>
              {system?.ytDlp ? t("settings.ready") : t("settings.missing")}
            </strong>
          </div>
          <div>
            <span>FFmpeg</span>
            <strong className={system?.ffmpeg ? "ok-text" : "bad-text"}>
              {system?.ffmpeg ? t("settings.ready") : t("settings.missing")}
            </strong>
          </div>
          <div>
            <span>whisper.cpp CPU</span>
            <strong className={system?.cpuEngine ? "ok-text" : "bad-text"}>
              {system?.cpuEngine ? t("settings.ready") : t("settings.missing")}
            </strong>
          </div>
        </div>
        <button type="button" className="secondary-button full" onClick={onRefresh} disabled={busy}>
          <RotateCcw size={16} /> {t("settings.recheck")}
        </button>
      </section>

      <section className="card settings-card danger-settings-card">
        <div className="card-title-row">
          <div>
            <span className="eyebrow">{t("settings.dataEyebrow")}</span>
            <h3>{t("settings.resetDataTitle")}</h3>
          </div>
          <Trash2 size={20} />
        </div>
        <p className="settings-note">{t("settings.resetDataDescription")}</p>
        <button
          type="button"
          className="danger-button full"
          ref={resetTriggerRef}
          onClick={() => {
            setResetComplete(false);
            setResetError(null);
            setResetDialogOpen(true);
          }}
          disabled={busy || resettingData}
        >
          {resettingData ? <LoaderCircle className="spin" size={16} /> : <Trash2 size={16} />}
          {resettingData ? t("settings.resetDataWorking") : t("settings.resetDataButton")}
        </button>
        {resetComplete && <p className="settings-success">{t("settings.resetDataComplete")}</p>}
      </section>
    </div>
  );
}

function ResetDataItem({ icon, title, detail }: { icon: ReactNode; title: string; detail: string }) {
  return (
    <div className="data-reset-item">
      <span className="data-reset-item-icon" aria-hidden="true">{icon}</span>
      <span className="data-reset-item-copy">
        <strong>{title}</strong>
        <span>{detail}</span>
      </span>
    </div>
  );
}

function CookieExtensionLink({
  label,
  url,
  copied,
  copyFailed,
  onOpen,
  onCopy,
  openLabel,
  copyLabel,
  copiedLabel,
  copyFailedLabel,
}: {
  label: string;
  url: string;
  copied: boolean;
  copyFailed: boolean;
  onOpen: (url: string) => void;
  onCopy: (url: string) => void;
  openLabel: string;
  copyLabel: string;
  copiedLabel: string;
  copyFailedLabel: string;
}) {
  return (
    <div className="cookies-guide-link-row">
      <div className="cookies-guide-link-copy">
        <strong>{label}</strong>
        <a
          href={url}
          onClick={(event) => {
            event.preventDefault();
            onOpen(url);
          }}
        >
          {url}
        </a>
      </div>
      <div className="cookies-guide-link-actions">
        <button type="button" className="secondary-button compact" onClick={() => onOpen(url)}>
          <ExternalLink size={14} /> {openLabel}
        </button>
        <button type="button" className="secondary-button compact" onClick={() => onCopy(url)}>
          {copied ? <Check size={14} /> : <Copy size={14} />} {copied ? copiedLabel : copyFailed ? copyFailedLabel : copyLabel}
        </button>
      </div>
    </div>
  );
}
