import { AlertTriangle, CheckSquare, ChevronRight, FileText, FolderOpen, History, RotateCcw, Square, Trash2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { formatBytes, formatDuration, friendlyError } from "../../lib/format";
import type { AppTab, HistoryFileInfo, HistoryFileKind, HistoryFileScope, HistoryItem } from "../../types";
import { getModelLabel, useI18n } from "../../i18n";

type HistoryPageProps = {
  history: HistoryItem[];
  hasMore: boolean;
  loadingMore: boolean;
  operationActive: boolean;
  onRefresh: () => void;
  onLoadMore: () => void;
  onLoad: (id: number) => void;
  onDelete: (ids: number[]) => Promise<void>;
  onDeleteFiles: (ids: number[], scope: HistoryFileScope) => Promise<void>;
  onListFiles: (id: number) => Promise<HistoryFileInfo[]>;
  onRevealFile: (id: number, kind: HistoryFileKind) => Promise<void>;
  onTabChange: (tab: AppTab) => void;
};

const MAX_DELETE_SELECTION = 100;

export function HistoryPage({ history, hasMore, loadingMore, operationActive, onRefresh, onLoadMore, onLoad, onDelete, onDeleteFiles, onListFiles, onRevealFile, onTabChange }: HistoryPageProps) {
  const { language, t } = useI18n();
  const dateLocale = language === "zh" ? "zh-CN" : language === "id" ? "id-ID" : "en-US";
  const [selectedIds, setSelectedIds] = useState<Set<number>>(new Set());
  const [pendingDeleteIds, setPendingDeleteIds] = useState<number[] | null>(null);
  const [deleteScope, setDeleteScope] = useState<HistoryFileScope | "all">("all");
  const [filesTarget, setFilesTarget] = useState<HistoryItem | null>(null);
  const [files, setFiles] = useState<HistoryFileInfo[] | null>(null);
  const [filesError, setFilesError] = useState<string | null>(null);
  const filesRequest = useRef(0);
  const cancelDeleteButtonRef = useRef<HTMLButtonElement>(null);
  const deleteDialogRef = useRef<HTMLDivElement>(null);
  const deleteTriggerRef = useRef<HTMLButtonElement | null>(null);
  const deleteDialogWasOpen = useRef(false);
  const selectableIds = history.slice(0, MAX_DELETE_SELECTION).map((item) => item.id);
  const allSelected = selectableIds.length > 0 && selectableIds.every((id) => selectedIds.has(id));
  useEffect(() => () => { filesRequest.current += 1; }, []);

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
      else if (next.size < MAX_DELETE_SELECTION) next.add(id);
      return next;
    });
  }

  function toggleAll() {
    setSelectedIds(
      allSelected
        ? new Set()
        : new Set(selectableIds),
    );
  }

  function handleDelete(ids: number[], trigger: HTMLButtonElement) {
    if (ids.length === 0) return;
    deleteTriggerRef.current = trigger;
    setDeleteScope("all");
    setPendingDeleteIds(ids);
  }

  async function confirmDelete() {
    const ids = pendingDeleteIds;
    if (!ids) return;
    try {
      if (deleteScope === "all") await onDelete(ids);
      else await onDeleteFiles(ids, deleteScope);
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

  async function openFiles(item: HistoryItem, trigger: HTMLButtonElement) {
    deleteTriggerRef.current = trigger;
    setFilesTarget(item);
    setFiles(null);
    setFilesError(null);
    const request = ++filesRequest.current;
    try {
      const next = await onListFiles(item.id);
      if (request === filesRequest.current) setFiles(next);
    } catch (cause) {
      if (request === filesRequest.current) setFilesError(friendlyError(cause));
    }
  }

  function closeFiles() { filesRequest.current += 1; setFilesTarget(null); }

  useEffect(() => {
    if (!pendingDeleteIds && !filesTarget) return;
    deleteDialogWasOpen.current = true;
    cancelDeleteButtonRef.current?.focus({ preventScroll: true });
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setPendingDeleteIds(null);
        closeFiles();
        return;
      }
      if (event.key !== "Tab" || !deleteDialogRef.current) return;
      const focusable = [...deleteDialogRef.current.querySelectorAll<HTMLElement>(
        "button, a[href], input, select, textarea, [tabindex]:not([tabindex='-1'])",
      )].filter((element) => !element.matches(":disabled") &&
        (!element.matches("input[type='radio']") || (element as HTMLInputElement).checked));
      if (focusable.length === 0) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("keydown", handleKeyDown);
      document.body.style.overflow = previousOverflow;
    };
  }, [pendingDeleteIds, filesTarget]);

  useEffect(() => {
    if (!pendingDeleteIds && !filesTarget && deleteDialogWasOpen.current) {
      deleteDialogWasOpen.current = false;
      deleteTriggerRef.current?.focus({ preventScroll: true });
      deleteTriggerRef.current = null;
    }
  }, [pendingDeleteIds, filesTarget]);

  return (
    <section className="card history-card">
      <div className="section-heading">
        <div>
          <span className="eyebrow">{t("history.eyebrow")}</span>
          <h2>{t("history.title")}</h2>
          <p>{t("history.description")}</p>
        </div>
        <div className="history-actions">
          <button type="button" className="secondary-button" onClick={onRefresh} disabled={operationActive}>
            <RotateCcw size={16} /> {t("history.refresh")}
          </button>
          {history.length > 0 && (
            <button type="button" className="secondary-button" onClick={toggleAll} disabled={operationActive} aria-pressed={allSelected}>
              {allSelected ? <CheckSquare size={16} /> : <Square size={16} />}
              {allSelected ? t("history.clearSelection") : t("history.selectAll")}
            </button>
          )}
        </div>
      </div>

      {selectedIds.size > 0 && (
        <div className="history-selection-bar">
          <span>
            {t("history.selected", { count: selectedIds.size })}
            {history.length > MAX_DELETE_SELECTION && ` · ${t("history.selectionLimit", { count: MAX_DELETE_SELECTION })}`}
          </span>
          <button
            type="button"
            className="danger-ghost"
            onClick={(event) => void handleDelete([...selectedIds], event.currentTarget)}
            disabled={operationActive}
          >
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
                disabled={operationActive || (!selectedIds.has(item.id) && selectedIds.size >= MAX_DELETE_SELECTION)}
                onChange={() => toggleSelection(item.id)}
                aria-label={t("history.selectItem", { title: item.title })}
              />
              <button type="button" className="history-item-main" onClick={() => onLoad(item.id)} disabled={operationActive}>
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
                type="button" className="icon-button" disabled={operationActive}
                onClick={(event) => void openFiles(item, event.currentTarget)}
                title={t("history.filesFor", { title: item.title })}
                aria-label={t("history.filesFor", { title: item.title })}
              ><FolderOpen size={17} /></button>
              <button
                type="button"
                className="icon-button danger history-delete-button"
                onClick={(event) => void handleDelete([item.id], event.currentTarget)}
                disabled={operationActive}
                title={t("history.deleteOne", { title: item.title })}
                aria-label={t("history.deleteOne", { title: item.title })}
              >
                <Trash2 size={17} />
              </button>
            </div>
          ))}
          {hasMore && (
            <button
              type="button"
              className="secondary-button history-load-more"
              onClick={onLoadMore}
              disabled={loadingMore || operationActive}
            >
              {loadingMore ? t("history.loadingMore") : t("history.loadMore")}
            </button>
          )}
        </div>
      )}

      {pendingDeleteIds && createPortal(
        <div className="modal-backdrop history-modal-backdrop" role="presentation">
          <div
            className="history-delete-modal"
            ref={deleteDialogRef}
            role="dialog"
            aria-modal="true"
            aria-labelledby="history-delete-title"
          >
            <div className="history-delete-icon" aria-hidden="true">
              <AlertTriangle size={22} />
            </div>
            <div className="history-delete-content">
              <h3 id="history-delete-title">{t("history.confirmTitle")}</h3>
              <fieldset className="history-delete-scope" disabled={operationActive}>
                <legend>{t("history.deleteScope")}</legend>
                {(["audio", "transcript", "all"] as const).map((scope) => <label key={scope}>
                  <input type="radio" name="history-delete-scope" checked={deleteScope === scope} onChange={() => setDeleteScope(scope)} />
                  {t(scope === "audio" ? "history.audioOnly" : scope === "transcript" ? "history.transcriptOnly" : "history.allFiles")}
                </label>)}
              </fieldset>
              <p>{t(deleteScope === "audio" ? "history.confirmAudio" : deleteScope === "transcript" ? "history.confirmTranscript" : "history.confirmDelete", { count: pendingDeleteIds.length })}</p>
              <p>{t("history.externalCopiesKept")}</p>
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
                  disabled={operationActive}
                >
                  <Trash2 size={16} /> {t("history.confirmOk")}
                </button>
              </div>
            </div>
          </div>
        </div>, document.body
      )}
      {filesTarget && createPortal(<div className="modal-backdrop history-modal-backdrop" role="presentation">
        <div className="history-delete-modal history-files-modal" ref={deleteDialogRef} role="dialog" aria-modal="true" aria-labelledby="history-files-title">
          <header className="history-files-header">
            <h3 id="history-files-title">{t("history.filesFor", { title: filesTarget.title })}</h3>
            <p>{t("history.filesHint")}</p>
          </header>
          <div className="history-files-body">
            {files === null && !filesError && <p role="status">{t("history.loadingFiles")}</p>}
            <div className="history-files-list">{files?.map((file) => <article key={file.kind} className="history-file-row">
              <strong>{file.kind === "audio" ? t("history.audioFormat") : file.kind.toUpperCase()}</strong>
              <span>{file.pendingDeletion ? t("history.filePending") : file.available ? formatBytes(file.bytes) : t("history.fileUnavailable")}</span>
              {(file.available || file.pendingDeletion) && <code>{file.path}</code>}
              <button type="button" className="secondary-button" disabled={operationActive || !file.available} onClick={() => {
                setFilesError(null);
                void onRevealFile(filesTarget.id, file.kind).catch((cause) => setFilesError(friendlyError(cause)));
              }}><FolderOpen size={16} /> {t("history.openLocation")}</button>
            </article>)}</div>
            {filesError && <p role="alert" className="bad-text">{filesError}</p>}
          </div>
          <footer className="history-files-footer">
            <button ref={cancelDeleteButtonRef} type="button" className="secondary-button" onClick={closeFiles}>{t("history.closeFiles")}</button>
          </footer>
        </div>
      </div>, document.body)}
    </section>
  );
}
