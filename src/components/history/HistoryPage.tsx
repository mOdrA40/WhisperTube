import { ChevronRight, FileText, History, RotateCcw } from "lucide-react";
import { formatDuration } from "../../lib/format";
import type { AppTab, HistoryItem } from "../../types";
import { getModelLabel, useI18n } from "../../i18n";

type HistoryPageProps = {
  history: HistoryItem[];
  onRefresh: () => void;
  onLoad: (id: number) => void;
  onTabChange: (tab: AppTab) => void;
};

export function HistoryPage({ history, onRefresh, onLoad, onTabChange }: HistoryPageProps) {
  const { language, t } = useI18n();
  const dateLocale = language === "zh" ? "zh-CN" : language === "id" ? "id-ID" : "en-US";

  return (
    <section className="card history-card">
      <div className="section-heading">
        <div>
          <span className="eyebrow">{t("history.eyebrow")}</span>
          <h2>{t("history.title")}</h2>
          <p>{t("history.description")}</p>
        </div>
        <button type="button" className="secondary-button" onClick={onRefresh}>
          <RotateCcw size={16} /> {t("history.refresh")}
        </button>
      </div>

      {history.length === 0 ? (
        <div className="empty-state">
          <History size={36} />
          <h3>{t("history.emptyTitle")}</h3>
          <p>{t("history.emptyDescription")}</p>
          <button
            type="button"
            className="primary-button"
            onClick={() => onTabChange("transcribe")}
          >
            {t("history.start")}
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
                  {item.channel} • {new Date(item.createdAt).toLocaleString(dateLocale)}
                </span>
              </div>
              <div className="history-meta">
                <span>{formatDuration(item.duration)}</span>
                <span>{getModelLabel(item.model, t)}</span>
              </div>
              <ChevronRight size={18} />
            </button>
          ))}
        </div>
      )}
    </section>
  );
}
