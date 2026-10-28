import { CheckIcon } from "@phosphor-icons/react/dist/csr/Check";
import { CopyIcon } from "@phosphor-icons/react/dist/csr/Copy";
import { FileTextIcon } from "@phosphor-icons/react/dist/csr/FileText";
import { FolderOpenIcon } from "@phosphor-icons/react/dist/csr/FolderOpen";
import { MagnifyingGlassIcon } from "@phosphor-icons/react/dist/csr/MagnifyingGlass";
import { TimerIcon } from "@phosphor-icons/react/dist/csr/Timer";
import { Virtuoso } from "react-virtuoso";
import type { Segment, TranscriptResult } from "../../types";
import { getModelLabel, useI18n } from "../../i18n";
import { isQwenModel } from "../../lib/models";
import { formatElapsedDuration } from "../../lib/format";

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
  transcriptionElapsedMs: number | null;
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
  transcriptionElapsedMs,
}: TranscriptCardProps) {
  const { t } = useI18n();
  const transcriptAvailable = result.transcriptAvailable !== false;
  const durationMismatch = transcriptAvailable && result.sourceDuration != null;

  return (
    <div className="transcript-card card">
      <div className="transcript-toolbar">
        <div>
          <span className="success-label">
            <CheckIcon aria-hidden="true" size={14} /> {t(!transcriptAvailable ? "history.savedFiles" : durationMismatch || result.audioRecovered ? "transcript.availableAudioComplete" : "transcript.complete")}
          </span>
          <h2>{result.title}</h2>
          <p>
            {result.language.toUpperCase()} • {getModelLabel(result.model, t)} • {result.backend.toUpperCase()}
            {transcriptionElapsedMs !== null && (
              <> • <TimerIcon size={13} aria-hidden="true" /> {t("transcript.processingTime")}: {formatElapsedDuration(transcriptionElapsedMs)}</>
            )}
          </p>
        </div>
        <div className="toolbar-actions">
          <button type="button" className="secondary-button" onClick={onCopy} disabled={operationActive || !transcriptAvailable}>
            {copied ? <CheckIcon aria-hidden="true" size={16} /> : <CopyIcon aria-hidden="true" size={16} />}
            {copied ? t("transcript.copied") : t("transcript.copy")}
          </button>
          <button type="button" className="secondary-button" onClick={() => onExport("txt")} disabled={operationActive || !transcriptAvailable}>
            <FileTextIcon aria-hidden="true" size={16} /> TXT
          </button>
          <button type="button" className="secondary-button" onClick={() => onExport("srt")} disabled={operationActive || !transcriptAvailable}>
            SRT
          </button>
          <button type="button" className="secondary-button" onClick={() => onExport("vtt")} disabled={operationActive || !transcriptAvailable}>
            VTT
          </button>
          {result.audioPath && (
            <button type="button" className="secondary-button" onClick={onRevealAudio} disabled={operationActive}>
              <FolderOpenIcon aria-hidden="true" size={16} /> {t("transcript.showAudio")}
            </button>
          )}
        </div>
      </div>

      {transcriptAvailable && isQwenModel(result.model) && <p className="warning-inline" role="note">{t("controls.qwenTiming")}</p>}
      {result.audioRecovered && <p role="status" className="warning-inline">{t("source.audioRecovered")}</p>}
      {durationMismatch && (
        <p role="status" className="warning-inline">
          {t("transcript.durationMismatch", {
            actual: formatElapsedDuration(result.duration * 1000),
            source: formatElapsedDuration(result.sourceDuration! * 1000),
          })}
        </p>
      )}

      <div className="transcript-search">
        <MagnifyingGlassIcon aria-hidden="true" size={16} />
        <input
          value={searchQuery}
          disabled={!transcriptAvailable}
          onChange={(event) => onSearchChange(event.target.value)}
          placeholder={t("transcript.search")}
          aria-label={t("transcript.search")}
        />
        <span>{transcriptAvailable ? filteredSegments.length : 0} {t("transcript.segments")}</span>
      </div>

      <div className="segments">
        {transcriptAvailable && filteredSegments.length > 0 ? (
          <Virtuoso
            className="transcript-virtual-list"
            data={filteredSegments}
            itemContent={(_, segment) => (
              <div className="segment">
                <div className="timestamp">
                  {segment.from}
                </div>
                <p>{segment.text}</p>
              </div>
            )}
          />
        ) : (
          <div className="empty-inline">{t(transcriptAvailable ? "transcript.empty" : "history.transcriptRemoved")}</div>
        )}
      </div>
    </div>
  );
}
