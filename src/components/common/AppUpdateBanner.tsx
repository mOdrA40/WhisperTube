import { Download, LoaderCircle, X } from "lucide-react";
import { useI18n } from "../../i18n";
import type { AppUpdateInfo, AppUpdateProgress, AppUpdateStatus } from "../../types";

type AppUpdateBannerProps = {
  update: AppUpdateInfo;
  status: AppUpdateStatus;
  progress: AppUpdateProgress;
  error: string | null;
  busy: boolean;
  onInstall: () => void;
  onDismiss: () => void;
};

export function AppUpdateBanner({
  update,
  status,
  progress,
  error,
  busy,
  onInstall,
  onDismiss,
}: AppUpdateBannerProps) {
  const { t } = useI18n();
  const installing = status === "installing";
  const percent = Math.max(0, Math.min(100, Math.round(progress.percent)));

  return (
    <section className="update-banner update-toast" role="status" aria-live="polite">
      <div className="update-banner-icon" aria-hidden="true">
        <Download size={18} />
      </div>
      <div className="update-banner-content">
        <div className="update-banner-title-row">
          <strong>{t("update.title")}</strong>
          <span>{t("update.available", { version: update.version })}</span>
        </div>
        <p className={error ? "update-banner-error" : undefined}>
          {error ?? (installing
            ? t("update.installing", { percent })
            : update.notes ?? t("update.restartHint"))}
        </p>
        {installing && (
          <div
            className="update-progress-track"
            role="progressbar"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={percent}
            aria-label={t("update.installing", { percent })}
          >
            <span style={{ width: `${percent}%` }} />
          </div>
        )}
      </div>
      <div className="update-banner-actions">
        <button
          type="button"
          className="primary-button compact"
          onClick={onInstall}
          disabled={busy || installing}
        >
          {installing ? <LoaderCircle className="spin" size={15} /> : <Download size={15} />}
          {installing ? `${percent}%` : t("update.install")}
        </button>
        {!installing && (
          <button
            type="button"
            className="icon-button"
            onClick={onDismiss}
            title={t("update.dismiss")}
            aria-label={t("update.dismiss")}
          >
            <X size={16} />
          </button>
        )}
      </div>
    </section>
  );
}
