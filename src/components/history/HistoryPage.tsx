import { AlertTriangle, CheckSquare, ChevronRight, FileText, History, RotateCcw, Square, Trash2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { formatDuration } from "../../lib/format";
import type { AppTab, HistoryItem } from "../../types";
import { getModelLabel, useI18n } from "../../i18n";

type HistoryPageProps = {
  history: HistoryItem[];
  onRefresh: () => void;
  onLoad: (id: number) => void;
  onDelete: (ids: number[]) => Promise<void>;
  onTabChange: (tab: AppTab) => void;
};

export function HistoryPage({ history, onRefresh, onLoad, onDelete, onTabChange }: HistoryPageProps) {
  const { language, t } = useI18n();
  const dateLocale = language === "zh" ? "zh-CN" : language === "id" ? "id-ID" : "en-US";
  const [selectedIds, setSelectedIds] = useState<Set<number>>(new Set());
  const [pendingDeleteIds, setPendingDeleteIds] = useState<number[] | null>(null);
  const cancelDeleteButtonRef = useRef<HTMLButtonElement>(null);
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

  function handleDelete(ids: number[]) {
    if (ids.length === 0) return;
    setPendingDeleteIds(ids);
  }

  async function confirmDelete() {
    const ids = pendingDeleteIds;
    if (!ids) return;
    try {
      await onDelete(ids);
      setSelectedIds((previous) => {
        const next = new Set(previous);
        ids.forEach((id) => next.delete(id));
        return next;
      });
      setPendingDeleteIds(null);
    } catch {
      // The parent reports the error; keep the selection so the user can retry.
    }
  }

  useEffect(() => {
    if (!pendingDeleteIds) return;
    cancelDeleteButtonRef.current?.focus();
    const handleEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setPendingDeleteIds(null);
    };
    document.addEventListener("keydown", handleEscape);
    return () => document.removeEventListener("keydown", handleEscape);
  }, [pendingDeleteIds]);

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

      {pendingDeleteIds && (
        <div className="modal-backdrop" role="presentation">
          <div
            className="history-delete-modal"
            role="dialog"
            aria-modal="true"
            aria-labelledby="history-delete-title"
          >
            <div className="history-delete-icon" aria-hidden="true">
              <AlertTriangle size={22} />
            </div>
            <div className="history-delete-content">
              <h3 id="history-delete-title">{t("history.confirmTitle")}</h3>
              <p>{t("history.confirmDelete", { count: pendingDeleteIds.length })}</p>
              <div className="history-delete-actions">
                <button
                  ref={cancelDeleteButtonRef}
                  type="button"
                  className="secondary-button"
                  onClick={() => setPendingDeleteIds(null)}
                >
                  {t("history.confirmCancel")}
                </button>
                <button
                  type="button"
                  className="danger-button"
                  onClick={() => void confirmDelete()}
                >
                  <Trash2 size={16} /> {t("history.confirmOk")}
                </button>
              </div>
            </div>
          </div>
        </div>
      )}
    </section>
  );
}
