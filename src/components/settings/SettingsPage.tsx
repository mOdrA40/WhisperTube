import { CircleStop, Cpu, Download, LockKeyhole, LoaderCircle, MonitorCog, RotateCcw, Trash2 } from "lucide-react";
import { formatBytes, formatMemory } from "../../lib/format";
import { getAcceleratorCopy, getModelCopy, uiLanguageOptions, useI18n } from "../../i18n";
import type { AcceleratorInfo, BackendChoice, BrowserChoice, BrowserInfo, ModelDownloadPayload, ModelInfo, SystemStatus } from "../../types";
import { CustomSelect, type SelectOption } from "../common/CustomSelect";

type SettingsPageProps = {
  browser: BrowserChoice;
  browsers: BrowserInfo[];
  browserProfile: string;
  system: SystemStatus | null;
  models: ModelInfo[];
  busy: boolean;
  downloadingModel: Record<string, ModelDownloadPayload>;
  accelerators: AcceleratorInfo[];
  installingCuda: boolean;
  cudaDownloadPercent: number;
  installingAccelerator: Exclude<BackendChoice, "auto" | "cpu" | "cuda"> | null;
  acceleratorDownloadPercent: number;
  onBrowserChange: (browser: BrowserChoice) => void;
  onBrowserProfileChange: (profile: string) => void;
  onDownloadModel: (id: string) => void;
  onCancelModel: () => void;
  onRemoveModel: (id: string) => void;
  onRefresh: () => void;
  onInstallCuda: () => void;
  onCancelCuda: () => void;
  onInstallAccelerator: (backend: Exclude<BackendChoice, "auto" | "cpu" | "cuda">) => void;
  onCancelAccelerator: () => void;
};

export function SettingsPage({
  browser,
  browsers,
  browserProfile,
  system,
  models,
  busy,
  downloadingModel,
  accelerators,
  installingCuda,
  cudaDownloadPercent,
  installingAccelerator,
  acceleratorDownloadPercent,
  onBrowserChange,
  onBrowserProfileChange,
  onDownloadModel,
  onCancelModel,
  onRemoveModel,
  onRefresh,
  onInstallCuda,
  onCancelCuda,
  onInstallAccelerator,
  onCancelAccelerator,
}: SettingsPageProps) {
  const { language: uiLanguage, setLanguage: setUiLanguage, t } = useI18n();
  const modelDownloadActive = Object.keys(downloadingModel).length > 0;
  const browserOptions: SelectOption[] = [
    { value: "none", label: t("settings.publicOnly") },
    ...browsers.map((item) => ({ value: item.id, label: item.label })),
  ];
  const interfaceLanguageOptions: SelectOption[] = uiLanguageOptions.map((option) => ({
    value: option.value,
    label: option.label,
  }));

  const activeBrowser = browsers.find((item) => item.id === browser);
    const profileOptions: SelectOption[] =
    activeBrowser?.profiles.map((profile) => ({
      value: profile.id,
      label: `${profile.label}${profile.isDefault ? ` (${t("settings.default")})` : ""}`,
    })) ?? [];

  return (
    <div className="settings-grid">
      <section className="card settings-card">
        <div className="card-title-row">
          <div>
            <span className="eyebrow">{t("settings.eyebrow")}</span>
            <h3>{t("settings.accessTitle")}</h3>
          </div>
          <LockKeyhole size={20} />
        </div>
        <p className="settings-intro">
          {t("settings.accessIntro")}
        </p>

        <label className="field-label">{t("settings.browserSession")}</label>
        <CustomSelect
          value={browser}
          options={browserOptions}
          onChange={(val) => onBrowserChange(val as BrowserChoice)}
          disabled={busy}
        />
        <p className="settings-note">{t("settings.browserDetectNote")}</p>

        {browser !== "none" && activeBrowser && (
          <>
            <label className="field-label">{t("settings.browserProfile")}</label>
            <CustomSelect
              value={browserProfile}
              options={profileOptions}
              onChange={onBrowserProfileChange}
              disabled={busy}
            />
            <p className="settings-note">
              {t("settings.profileNote")}
            </p>
          </>
        )}

        {browser === "none" && browsers.length === 0 && (
          <p className="settings-note">
            {t("settings.noBrowsers")}
          </p>
        )}

        <div className="info-box">
          <LockKeyhole size={16} />
          <span>{t("settings.cookies")}</span>
        </div>

        <label className="field-label">{t("settings.interfaceLanguage")}</label>
        <CustomSelect
          value={uiLanguage}
          options={interfaceLanguageOptions}
          onChange={(value) => setUiLanguage(value as typeof uiLanguage)}
          disabled={busy}
        />
        <p className="settings-note">{t("settings.interfaceLanguageNote")}</p>
      </section>

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
                ? t("settings.installingCuda", { percent: Math.round(cudaDownloadPercent) })
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
            {!accelerator.installed && (
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
                  ? t("settings.acceleratorInstalling", { percent: Math.round(acceleratorDownloadPercent) })
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
                  onClick={() => onRemoveModel(model.id)}
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
