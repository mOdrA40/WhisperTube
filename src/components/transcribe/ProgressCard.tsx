import { PulseIcon } from "@phosphor-icons/react/dist/csr/Pulse";
import { StopCircleIcon } from "@phosphor-icons/react/dist/csr/StopCircle";
import { GaugeIcon } from "@phosphor-icons/react/dist/csr/Gauge";
import { CircleNotchIcon } from "@phosphor-icons/react/dist/csr/CircleNotch";
import { TimerIcon } from "@phosphor-icons/react/dist/csr/Timer";
import { WifiHighIcon } from "@phosphor-icons/react/dist/csr/WifiHigh";
import { getProgressMessage, getProgressStageLabel, useI18n } from "../../i18n";
import { formatBytes, formatElapsedDuration } from "../../lib/format";
import type { BackendChoice, ModelInfo, ProgressPayload } from "../../types";

type ProgressCardProps = {
  progress: ProgressPayload;
  selectedModel: ModelInfo | undefined;
  backend: BackendChoice;
  networkSpeedBytesPerSecond: number | null;
  transcriptionElapsedMs: number | null;
  onCancel: () => void;
};

export function ProgressCard({ progress, selectedModel, backend, networkSpeedBytesPerSecond, transcriptionElapsedMs, onCancel }: ProgressCardProps) {
  const { t } = useI18n();
  const percent = Math.max(0, Math.min(100, progress.percent));
  const actualBackend = progress.backend;
  const networkSpeed = networkSpeedBytesPerSecond ?? progress.networkBytesPerSecond;

  return (
    <div className="progress-card card">
      <div className="progress-header">
        <div>
          <span className="eyebrow">
            <CircleNotchIcon aria-hidden="true" className="spin" size={15} /> {t("progress.processing")}
          </span>
          <h2>{getProgressStageLabel(progress.stage, t)}</h2>
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
            <div className="progress-metric" title={t("progress.cpuUsageNote")}>
              <PulseIcon aria-hidden="true" size={13} />
              <span>{t("progress.cpuUsage")}</span>
              <strong>{formatUsage(progress.cpuUsagePercent, t("progress.unavailable"))}</strong>
            </div>
            <div className="progress-metric">
              <GaugeIcon aria-hidden="true" size={13} />
              <span>{t("progress.gpuUsage")}</span>
              <strong>{formatUsage(progress.gpuUsagePercent, t("progress.unavailable"))}</strong>
            </div>
            {networkSpeed !== null && (
              <div className="progress-metric network-metric">
                <WifiHighIcon aria-hidden="true" size={13} />
                <span>{t("progress.networkSpeed")}</span>
                <strong>{formatBytes(networkSpeed)}/s</strong>
              </div>
            )}
            <div className="progress-metric">
              <TimerIcon aria-hidden="true" size={13} />
              <span>{t("progress.elapsedTime")}</span>
              <strong>{formatElapsedDuration(transcriptionElapsedMs)}</strong>
            </div>
          </div>
        </div>
        <strong>{Math.round(percent)}%</strong>
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
          {selectedModel?.label} • {actualBackend ? actualBackend.toUpperCase() : t("workspace.pendingBackend")}
        </span>
        <button type="button" className="danger-ghost" onClick={onCancel}>
          <StopCircleIcon aria-hidden="true" size={16} /> {t("progress.cancel")}
        </button>
      </div>
    </div>
  );
}

function formatUsage(value: number | null, fallback: string) {
  return value === null || !Number.isFinite(value) ? fallback : `${Math.round(value)}%`;
}
