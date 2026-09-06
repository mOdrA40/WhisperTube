import { Check, CircleStop, Copy, Cpu, Download, ExternalLink, FileKey, Globe2, LockKeyhole, LoaderCircle, MonitorCog, RotateCcw, Trash2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { formatBytes, formatMemory } from "../../lib/format";
import { getAcceleratorCopy, getModelCopy, uiLanguageOptions, useI18n } from "../../i18n";
import type { AcceleratorInfo, AppUpdateInfo, AppUpdateProgress, AppUpdateStatus, BackendChoice, BrowserInfo, ModelDownloadPayload, ModelInfo, SystemStatus } from "../../types";
import { CustomSelect, type SelectOption } from "../common/CustomSelect";

const COOKIES_EXTENSION_URL = "https://chromewebstore.google.com/detail/get-cookiestxt-locally/cclelndahbckbenkjhflpdbgdldlbecc";
const FIREFOX_COOKIES_EXTENSION_URL = "https://addons.mozilla.org/en-US/firefox/addon/get-cookies-txt-locally/";
const CHROMIUM_BROWSER_IDS = new Set(["chrome", "brave", "edge", "chromium", "opera", "vivaldi", "whale"]);

type SettingsPageProps = {
  cookiesPath: string;
  usingSafariSession: boolean;
  browsers: BrowserInfo[];
  system: SystemStatus | null;
  models: ModelInfo[];
  busy: boolean;
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
  onSelectCookiesFile: () => void;
  onClearCookiesFile: () => void;
  onUseSafariSession: () => void;
  onOpenUrl: (url: string) => void;
  onDownloadModel: (id: string) => void;
  onCancelModel: () => void;
  onRemoveModel: (id: string) => void;
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
  usingSafariSession,
  browsers,
  system,
  models,
  busy,
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
  onSelectCookiesFile,
  onClearCookiesFile,
  onUseSafariSession,
  onOpenUrl,
  onDownloadModel,
  onCancelModel,
  onRemoveModel,
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
  const [copiedUrl, setCopiedUrl] = useState<string | null>(null);
  const [copyFailedUrl, setCopyFailedUrl] = useState<string | null>(null);
  const cookiesDialogRef = useRef<HTMLDivElement>(null);
  const cookiesTriggerRef = useRef<HTMLButtonElement>(null);
  const cookiesContinueRef = useRef<HTMLButtonElement>(null);
  const cookiesDialogWasOpen = useRef(false);
  const modelDownloadActive = Object.keys(downloadingModel).length > 0;
  const interfaceLanguageOptions: SelectOption[] = uiLanguageOptions.map((option) => ({
    value: option.value,
    label: option.label,
  }));
  const chromiumDetected = browsers.some((browser) => CHROMIUM_BROWSER_IDS.has(browser.id));
  const firefoxDetected = browsers.some((browser) => browser.id === "firefox");
  const safariDetected = browsers.some((browser) => browser.id === "safari");
  const showChromiumLink = chromiumDetected || !firefoxDetected;
  const showFirefoxLink = firefoxDetected || !chromiumDetected;

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

  return (
    <div className="settings-grid">
      <section className="card settings-card update-settings-card">
        <div className="card-title-row">
          <div>
            <span className="eyebrow">{t("update.eyebrow")}</span>
            <h3>{t("update.title")}</h3>
          </div>
          <Download size={20} />
        </div>
        <p className="settings-note">{t("update.description")}</p>
        {appUpdate ? (
          <div className="update-settings-available">
            <strong>{t("update.available", { version: appUpdate.version })}</strong>
            {appUpdate.notes && (
              <div className="update-notes">
                <span>{t("update.releaseNotes")}</span>
                <p>{appUpdate.notes}</p>
              </div>
            )}
            {appUpdateStatus === "installing" && (
              <div className="update-settings-progress">
                <div className="update-progress-track">
                  <span style={{ width: `${Math.round(appUpdateProgress.percent)}%` }} />
                </div>
                <small>{t("update.installing", { percent: Math.round(appUpdateProgress.percent) })}</small>
              </div>
            )}
          </div>
        ) : appUpdateStatus === "up-to-date" ? (
          <p className="settings-success">{t("update.upToDate")}</p>
        ) : null}
        {appUpdateError && (
          <p className="settings-error">{t("update.error", { error: appUpdateError })}</p>
        )}
        <div className="update-settings-actions">
          <button
            type="button"
            className="secondary-button"
            onClick={onCheckForUpdate}
            disabled={appUpdateStatus === "checking" || appUpdateStatus === "installing"}
          >
            {appUpdateStatus === "checking" ? <LoaderCircle className="spin" size={15} /> : <RotateCcw size={15} />}
            {appUpdateStatus === "checking" ? t("update.checking") : t("update.check")}
          </button>
          {appUpdate && appUpdateStatus !== "installing" && (
            <button
              type="button"
              className="primary-button"
              onClick={onInstallAppUpdate}
              disabled={busy}
            >
              <Download size={15} /> {t("update.install")}
            </button>
          )}
        </div>
        {appUpdate && <p className="settings-note">{t("update.restartHint")}</p>}
      </section>

      <section className="card settings-card">
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

        {safariDetected && (
          <div className="info-box">
            <Globe2 size={16} />
            <span>
              {usingSafariSession
                ? t("settings.safariSessionActive")
                : t("settings.safariSessionNote")}
            </span>
            <button
              type="button"
              className="secondary-button compact"
              onClick={usingSafariSession ? onSelectCookiesFile : onUseSafariSession}
              disabled={busy}
            >
              {usingSafariSession
                ? t("settings.useCookiesFileInstead")
                : t("settings.useSafariSession")}
            </button>
          </div>
        )}

        <label className="field-label">{t("settings.interfaceLanguage")}</label>
        <CustomSelect
          value={uiLanguage}
          options={interfaceLanguageOptions}
          onChange={(value) => setUiLanguage(value as typeof uiLanguage)}
          disabled={busy}
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
                {showChromiumLink && (
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
                )}
                {showFirefoxLink && (
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
                )}
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

      <section className="card settings-card">
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
            <strong>{system?.gpuName ?? t("hardware.notDetected")}</strong>
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
                  {getModelCopy(model, t).description} • {model.sizeMb} MB • {t("controls.cudaRequirement", { memory: formatMemory(model.vramRequiredMb, t("hardware.notDetected")) })}
                </span>
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
                  onClick={() => {
                    if (window.confirm(t("settings.confirmDeleteModel", { model: getModelCopy(model, t).label }))) {
                      onRemoveModel(model.id);
                    }
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
                  disabled={modelDownloadActive || busy}
                >
                  <Download size={15} /> {t("settings.download")}
                </button>
              )}
            </div>
          ))}
        </div>
      </section>

      <section className="card settings-card">
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
        <button type="button" className="secondary-button full" onClick={onRefresh}>
          <RotateCcw size={16} /> {t("settings.recheck")}
        </button>
      </section>
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
