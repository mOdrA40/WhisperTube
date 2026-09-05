import { Activity, CircleStop, Gauge, LoaderCircle, Wifi } from "lucide-react";
import { getProgressMessage, getProgressStageLabel, useI18n } from "../../i18n";
import { formatBytes } from "../../lib/format";
import type { BackendChoice, ModelInfo, ProgressPayload } from "../../types";

type ProgressCardProps = {
  progress: ProgressPayload;
  selectedModel: ModelInfo | undefined;
  backend: BackendChoice;
  networkSpeedBytesPerSecond: number | null;
  onCancel: () => void;
};

export function ProgressCard({ progress, selectedModel, backend, networkSpeedBytesPerSecond, onCancel }: ProgressCardProps) {
  const { t } = useI18n();
  const percent = Math.max(0, Math.min(100, progress.percent));
  const actualBackend = progress.backend ?? (backend === "auto" ? null : backend);
  const networkSpeed = networkSpeedBytesPerSecond ?? progress.networkBytesPerSecond;

  return (
    <div className="progress-card card">
      <div className="progress-header">
        <div>
          <span className="eyebrow">
            <LoaderCircle className="spin" size={15} /> {t("progress.processing")}
          </span>
          <h3>{getProgressStageLabel(progress.stage, t)}</h3>
          <p aria-live="polite" aria-atomic="true">
            {getProgressMessage(progress, backend, t)}
          </p>
          {progress.stage === "downloading" && progress.downloadedBytes !== null && (
            <span className="progress-bytes">
              {formatBytes(progress.downloadedBytes, t("progress.unknownSize"))} / {progress.totalBytes !== null
                ? formatBytes(progress.totalBytes)
                : t("progress.unknownSize")}
            </span>
          )}
          <div className="progress-metrics">
            <div className="progress-metric">
              <Activity size={13} />
              <span>{t("progress.cpuUsage")}</span>
              <strong>{formatUsage(progress.cpuUsagePercent, t("progress.unavailable"))}</strong>
            </div>
            <div className="progress-metric">
              <Gauge size={13} />
              <span>{t("progress.gpuUsage")}</span>
              <strong>{formatUsage(progress.gpuUsagePercent, t("progress.unavailable"))}</strong>
            </div>
            {networkSpeed !== null && (
              <div className="progress-metric network-metric">
                <Wifi size={13} />
                <span>{t("progress.networkSpeed")}</span>
                <strong>{formatBytes(networkSpeed)}/s</strong>
              </div>
            )}
          </div>
        </div>
        <strong>{Math.round(progress.percent)}%</strong>
      </div>

      <div
        className="progress-track"
        role="progressbar"
        aria-label={t("progress.processing")}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent}
      >
        <div className="progress-fill" style={{ width: `${percent}%` }} />
      </div>

      <div className="progress-footer">
        <span>
          {selectedModel?.label} • {actualBackend ? actualBackend.toUpperCase() : t("progress.autoBackend")}
        </span>
        <button type="button" className="danger-ghost" onClick={onCancel}>
          <CircleStop size={16} /> {t("progress.cancel")}
        </button>
      </div>
    </div>
  );
}

function formatUsage(value: number | null, fallback: string) {
  return value === null || !Number.isFinite(value) ? fallback : `${Math.round(value)}%`;
}
