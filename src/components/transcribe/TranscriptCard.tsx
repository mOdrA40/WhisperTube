import { Check, Copy, FileText, FolderOpen, Play, Search } from "lucide-react";
import { Virtuoso } from "react-virtuoso";
import type { Segment, TranscriptResult } from "../../types";
import { getModelLabel, useI18n } from "../../i18n";

type TranscriptCardProps = {
  result: TranscriptResult;
  operationActive: boolean;
  copied: boolean;
  searchQuery: string;
  filteredSegments: Segment[];
  onCopy: () => void;
  onExport: (kind: "txt" | "srt" | "vtt") => void;
  onRevealAudio: () => void;
  onSearchChange: (query: string) => void;
};

export function TranscriptCard({
  result,
  operationActive,
  copied,
  searchQuery,
  filteredSegments,
  onCopy,
  onExport,
  onRevealAudio,
  onSearchChange,
}: TranscriptCardProps) {
  const { t } = useI18n();

  return (
    <div className="transcript-card card">
      <div className="transcript-toolbar">
        <div>
          <span className="success-label">
            <Check size={14} /> {t("transcript.complete")}
          </span>
          <h3>{result.title}</h3>
          <p>
            {result.language.toUpperCase()} • {getModelLabel(result.model, t)} • {result.backend.toUpperCase()}
          </p>
        </div>
        <div className="toolbar-actions">
          <button type="button" className="secondary-button" onClick={onCopy} disabled={operationActive}>
            {copied ? <Check size={16} /> : <Copy size={16} />}
            {copied ? t("transcript.copied") : t("transcript.copy")}
          </button>
          <button type="button" className="secondary-button" onClick={() => onExport("txt")} disabled={operationActive}>
            <FileText size={16} /> TXT
          </button>
          <button type="button" className="secondary-button" onClick={() => onExport("srt")} disabled={operationActive}>
            SRT
          </button>
          <button type="button" className="secondary-button" onClick={() => onExport("vtt")} disabled={operationActive}>
            VTT
          </button>
          {result.audioPath && (
            <button type="button" className="secondary-button" onClick={onRevealAudio} disabled={operationActive}>
              <FolderOpen size={16} /> {t("transcript.showAudio")}
            </button>
          )}
        </div>
      </div>

      <div className="transcript-search">
        <Search size={16} />
        <input
          value={searchQuery}
          onChange={(event) => onSearchChange(event.target.value)}
          placeholder={t("transcript.search")}
          aria-label={t("transcript.search")}
        />
        <span>{filteredSegments.length} {t("transcript.segments")}</span>
      </div>

      <div className="segments">
        {filteredSegments.length > 0 ? (
          <Virtuoso
            className="transcript-virtual-list"
            data={filteredSegments}
            itemContent={(_, segment) => (
              <div className="segment">
                <div className="timestamp">
                  <Play size={11} fill="currentColor" /> {segment.from}
                </div>
                <p>{segment.text}</p>
              </div>
            )}
          />
        ) : (
          <div className="empty-inline">{t("transcript.empty")}</div>
        )}
      </div>
    </div>
  );
}
