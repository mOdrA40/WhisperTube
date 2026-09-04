import { CheckSquare, ChevronRight, FileText, History, RotateCcw, Square, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { confirm } from "@tauri-apps/plugin-dialog";
import { formatDuration } from "../../lib/format";
import type { AppTab, HistoryItem } from "../../types";
import { getModelLabel, useI18n } from "../../i18n";

type HistoryPageProps = {
  history: HistoryItem[];
  onRefresh: () => void;
  onLoad: (id: number) => void;
  onDelete: (ids: number[]) => Promise<void>;
  onError: (message: string) => void;
  onTabChange: (tab: AppTab) => void;
};

export function HistoryPage({ history, onRefresh, onLoad, onDelete, onError, onTabChange }: HistoryPageProps) {
  const { language, t } = useI18n();
  const dateLocale = language === "zh" ? "zh-CN" : language === "id" ? "id-ID" : "en-US";
  const [selectedIds, setSelectedIds] = useState<Set<number>>(new Set());
  const allSelected = history.length > 0 && selectedIds.size === history.length;

  useEffect(() => {
    setSelectedIds((previous) => {
      const available = new Set(history.map((item) => item.id));
      return new Set([...previous].filter((id) => available.has(id)));
    });
  }, [history]);

  function toggleSelection(id: number) {
    setSelectedIds((previous) => {
      const next = new Set(previous);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  function toggleAll() {
    setSelectedIds(allSelected ? new Set() : new Set(history.map((item) => item.id)));
  }

  async function handleDelete(ids: number[]) {
    if (ids.length === 0) return;
    let confirmed: boolean;
    try {
      confirmed = await confirm(t("history.confirmDelete", { count: ids.length }), {
        title: t("history.confirmTitle"),
        kind: "warning",
        okLabel: t("history.confirmOk"),
        cancelLabel: t("history.confirmCancel"),
      });
    } catch (cause) {
      onError(String(cause));
      return;
    }
    if (!confirmed) return;
    try {
      await onDelete(ids);
      setSelectedIds((previous) => {
        const next = new Set(previous);
        ids.forEach((id) => next.delete(id));
        return next;
      });
    } catch {
      // The parent reports the error; keep the selection so the user can retry.
    }
  }

  return (
    <section className="card history-card">
      <div className="section-heading">
        <div>
          <span className="eyebrow">{t("history.eyebrow")}</span>
          <h2>{t("history.title")}</h2>
          <p>{t("history.description")}</p>
        </div>
        <div className="history-actions">
          <button type="button" className="secondary-button" onClick={onRefresh}>
            <RotateCcw size={16} /> {t("history.refresh")}
          </button>
          {history.length > 0 && (
            <button type="button" className="secondary-button" onClick={toggleAll} aria-pressed={allSelected}>
              {allSelected ? <CheckSquare size={16} /> : <Square size={16} />}
              {allSelected ? t("history.clearSelection") : t("history.selectAll")}
            </button>
          )}
        </div>
      </div>

      {selectedIds.size > 0 && (
        <div className="history-selection-bar">
          <span>{t("history.selected", { count: selectedIds.size })}</span>
          <button type="button" className="danger-ghost" onClick={() => void handleDelete([...selectedIds])}>
            <Trash2 size={16} /> {t("history.deleteSelected")}
          </button>
        </div>
      )}

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
            <div
              key={item.id}
              className={`history-item ${selectedIds.has(item.id) ? "selected" : ""}`}
            >
              <input
                type="checkbox"
                className="history-checkbox"
                checked={selectedIds.has(item.id)}
                onChange={() => toggleSelection(item.id)}
                aria-label={t("history.selectItem")}
              />
              <button type="button" className="history-item-main" onClick={() => onLoad(item.id)}>
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
              <button
                type="button"
                className="icon-button danger history-delete-button"
                onClick={() => void handleDelete([item.id])}
                title={t("history.deleteOne")}
                aria-label={t("history.deleteOne")}
              >
                <Trash2 size={17} />
              </button>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}
