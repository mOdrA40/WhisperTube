import { useState } from "react";
import { HardDriveIcon } from "@phosphor-icons/react/dist/csr/HardDrive";
import { TrashIcon } from "@phosphor-icons/react/dist/csr/Trash";
import { useI18n } from "../../i18n";
import { formatBytes, friendlyError } from "../../lib/format";
import type { JobCacheEntry } from "../../types";

type Props = {
  items: JobCacheEntry[] | null;
  busy: boolean;
  onLoad: () => Promise<void>;
  onDiscard: (id: string) => Promise<void>;
};

export function JobCachePanel({ items, busy, onLoad, onDiscard }: Props) {
  const { t } = useI18n();
  const [pendingId, setPendingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  async function discard(id: string) {
    setError(null);
    try { await onDiscard(id); setPendingId(null); }
    catch (cause) { setError(friendlyError(cause)); }
  }
  return <section id="settings-storage" className="card settings-card job-cache-settings-card">
    <div className="card-title-row"><h2>{t("settings.jobCacheTitle")}</h2><HardDriveIcon aria-hidden="true" size={20} /></div>
    <p className="settings-note">{t("settings.jobCacheHint")}</p>
    <button type="button" className="secondary-button" disabled={busy} onClick={() => { setError(null); void onLoad(); }}>
      {t("settings.jobCacheLoad")}
    </button>
    {items?.length === 0 && <p className="settings-note">{t("settings.jobCacheEmpty")}</p>}
    <div className="job-cache-list">{items?.map((item) => <article className="job-cache-item" key={item.id}>
      <strong>{item.title || item.sourceUrl || t("settings.jobCacheUnnamed")}</strong>
      <small>{formatBytes(item.bytes, t("progress.unknownSize"))} · {new Date(item.modifiedAt).toLocaleString()}</small>
      {pendingId === item.id ? <div>
        <p>{t("settings.jobCacheConfirm")}</p>
        <button type="button" className="secondary-button" disabled={busy} onClick={() => { setPendingId(null); setError(null); }}>{t("settings.jobCacheKeep")}</button>
        <button type="button" className="danger-ghost" disabled={busy} onClick={() => void discard(item.id)}>{t("settings.jobCacheDelete")}</button>
      </div> : <button type="button" className="danger-ghost" disabled={busy} onClick={() => { setPendingId(item.id); setError(null); }}>
        <TrashIcon aria-hidden="true" size={14} /> {t("settings.jobCacheDiscard")}
      </button>}
    </article>)}</div>
    {error && <p role="alert" className="bad-text">{error}</p>}
  </section>;
}
