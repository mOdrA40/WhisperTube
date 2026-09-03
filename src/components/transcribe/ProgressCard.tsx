import { CircleStop, LoaderCircle } from "lucide-react";
import { getProgressMessage, getProgressStageLabel, useI18n } from "../../i18n";
import type { BackendChoice, ModelInfo, ProgressPayload } from "../../types";

type ProgressCardProps = {
  progress: ProgressPayload;
  selectedModel: ModelInfo | undefined;
  backend: BackendChoice;
  onCancel: () => void;
};

export function ProgressCard({ progress, selectedModel, backend, onCancel }: ProgressCardProps) {
  const { t } = useI18n();
  const percent = Math.max(0, Math.min(100, progress.percent));
  const actualBackend = progress.backend ?? (backend === "auto" ? null : backend);

  return (
    <div className="progress-card card">
      <div className="progress-header">
        <div>
          <span className="eyebrow">
            <LoaderCircle className="spin" size={15} /> {t("progress.processing")}
          </span>
          <h3>{getProgressStageLabel(progress.stage, t)}</h3>
          <p>{getProgressMessage(progress, backend, t)}</p>
        </div>
        <strong>{Math.round(progress.percent)}%</strong>
      </div>

      <div className="progress-track">
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
