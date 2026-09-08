import { useEffect, useState } from "react";
import { Check, Copy, FileText, FolderOpen, Play, Search } from "lucide-react";
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

const SEGMENTS_PER_PAGE = 300;

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
  const [visibleSegmentCount, setVisibleSegmentCount] = useState(SEGMENTS_PER_PAGE);
  const visibleSegments = filteredSegments.slice(0, visibleSegmentCount);

  useEffect(() => {
    setVisibleSegmentCount(SEGMENTS_PER_PAGE);
  }, [result.historyId, searchQuery]);

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
        />
        <span>{filteredSegments.length} {t("transcript.segments")}</span>
      </div>

      <div className="segments">
        {visibleSegments.map((segment, index) => (
          <div className="segment" key={`${segment.from}-${index}`}>
            <div className="timestamp">
              <Play size={11} fill="currentColor" /> {segment.from}
            </div>
            <p>{segment.text}</p>
          </div>
        ))}
        {filteredSegments.length === 0 && (
          <div className="empty-inline">{t("transcript.empty")}</div>
        )}
        {visibleSegmentCount < filteredSegments.length && (
          <button
            type="button"
            className="secondary-button transcript-load-more"
            onClick={() => setVisibleSegmentCount((count) => count + SEGMENTS_PER_PAGE)}
          >
            {t("transcript.loadMore", {
              count: Math.min(SEGMENTS_PER_PAGE, filteredSegments.length - visibleSegmentCount),
            })}
          </button>
        )}
      </div>
    </div>
  );
}
