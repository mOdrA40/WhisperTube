import { Check, Copy, FileText, Play, Search } from "lucide-react";
import type { Segment, TranscriptResult } from "../../types";

type TranscriptCardProps = {
  result: TranscriptResult;
  copied: boolean;
  searchQuery: string;
  filteredSegments: Segment[];
  onCopy: () => void;
  onExport: (kind: "txt" | "srt" | "vtt") => void;
  onSearchChange: (query: string) => void;
};

export function TranscriptCard({
  result,
  copied,
  searchQuery,
  filteredSegments,
  onCopy,
  onExport,
  onSearchChange,
}: TranscriptCardProps) {
  return (
    <div className="transcript-card card">
      <div className="transcript-toolbar">
        <div>
          <span className="success-label">
            <Check size={14} /> Transcription complete
          </span>
          <h3>{result.title}</h3>
          <p>
            {result.language.toUpperCase()} • {result.model} • {result.backend.toUpperCase()}
          </p>
        </div>
        <div className="toolbar-actions">
          <button type="button" className="secondary-button" onClick={onCopy}>
            {copied ? <Check size={16} /> : <Copy size={16} />}
            {copied ? "Copied" : "Copy"}
          </button>
          <button type="button" className="secondary-button" onClick={() => onExport("txt")}>
            <FileText size={16} /> TXT
          </button>
          <button type="button" className="secondary-button" onClick={() => onExport("srt")}>
            SRT
          </button>
          <button type="button" className="secondary-button" onClick={() => onExport("vtt")}>
            VTT
          </button>
        </div>
      </div>

      <div className="transcript-search">
        <Search size={16} />
        <input
          value={searchQuery}
          onChange={(event) => onSearchChange(event.target.value)}
          placeholder="Cari di transcript…"
        />
        <span>{filteredSegments.length} segments</span>
      </div>

      <div className="segments">
        {filteredSegments.map((segment, index) => (
          <div className="segment" key={`${segment.from}-${index}`}>
            <div className="timestamp">
              <Play size={11} fill="currentColor" /> {segment.from}
            </div>
            <p>{segment.text}</p>
          </div>
        ))}
        {filteredSegments.length === 0 && (
          <div className="empty-inline">Tidak ada bagian transcript yang cocok.</div>
        )}
      </div>
    </div>
  );
}
