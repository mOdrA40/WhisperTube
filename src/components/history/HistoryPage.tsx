import { ChevronRight, FileText, History, RotateCcw } from "lucide-react";
import { formatDuration } from "../../lib/format";
import type { AppTab, HistoryItem } from "../../types";

type HistoryPageProps = {
  history: HistoryItem[];
  onRefresh: () => void;
  onLoad: (id: number) => void;
  onTabChange: (tab: AppTab) => void;
};

export function HistoryPage({ history, onRefresh, onLoad, onTabChange }: HistoryPageProps) {
  return (
    <section className="card history-card">
      <div className="section-heading">
        <div>
          <span className="eyebrow">Local library</span>
          <h2>Transcription history</h2>
          <p>Metadata dan transcript tersimpan hanya di komputer ini.</p>
        </div>
        <button type="button" className="secondary-button" onClick={onRefresh}>
          <RotateCcw size={16} /> Refresh
        </button>
      </div>

      {history.length === 0 ? (
        <div className="empty-state">
          <History size={36} />
          <h3>Belum ada history</h3>
          <p>Selesaikan transkripsi pertama dan hasilnya akan muncul di sini.</p>
          <button
            type="button"
            className="primary-button"
            onClick={() => onTabChange("transcribe")}
          >
            Start transcription
          </button>
        </div>
      ) : (
        <div className="history-list">
          {history.map((item) => (
            <button
              key={item.id}
              type="button"
              className="history-item"
              onClick={() => onLoad(item.id)}
            >
              <div className="history-icon">
                <FileText size={19} />
              </div>
              <div className="history-copy">
                <strong>{item.title}</strong>
                <span>
                  {item.channel} • {new Date(item.createdAt).toLocaleString("id-ID")}
                </span>
              </div>
              <div className="history-meta">
                <span>{formatDuration(item.duration)}</span>
                <span>{item.model}</span>
              </div>
              <ChevronRight size={18} />
            </button>
          ))}
        </div>
      )}
    </section>
  );
}
