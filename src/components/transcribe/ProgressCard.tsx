import { CircleStop, LoaderCircle } from "lucide-react";
import { stageLabels } from "../../lib/format";
import type { BackendChoice, ModelInfo, ProgressPayload } from "../../types";

type ProgressCardProps = {
  progress: ProgressPayload;
  selectedModel: ModelInfo | undefined;
  backend: BackendChoice;
  onCancel: () => void;
};

export function ProgressCard({ progress, selectedModel, backend, onCancel }: ProgressCardProps) {
  const percent = Math.max(0, Math.min(100, progress.percent));

  return (
    <div className="progress-card card">
      <div className="progress-header">
        <div>
          <span className="eyebrow"><LoaderCircle className="spin" size={15} /> Processing</span>
          <h3>{stageLabels[progress.stage]}</h3>
          <p>{progress.message || "Memproses secara lokal…"}</p>
        </div>
        <strong>{Math.round(progress.percent)}%</strong>
      </div>
      <div className="progress-track"><div className="progress-fill" style={{ width: `${percent}%` }} /></div>
      <div className="progress-footer">
        <span>{selectedModel?.label} • {backend === "auto" ? "Auto backend" : backend.toUpperCase()}</span>
        <button className="danger-ghost" onClick={onCancel}><CircleStop size={16} /> Cancel</button>
      </div>
    </div>
  );
}
