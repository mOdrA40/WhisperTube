import { Check, ChevronRight, Clock3, LoaderCircle, Search, Youtube } from "lucide-react";
import type { VideoMetadata } from "../../types";
import { formatDuration } from "../../lib/format";

type SourceCardProps = {
  url: string;
  busy: boolean;
  inspecting: boolean;
  metadata: VideoMetadata | null;
  onUrlChange: (url: string) => void;
  onInspect: () => void;
  onOpenSettings: () => void;
};

export function SourceCard({
  url,
  busy,
  inspecting,
  metadata,
  onUrlChange,
  onInspect,
  onOpenSettings,
}: SourceCardProps) {
  return (
    <>
      <div className="hero-card card">
        <div className="eyebrow"><Youtube size={15} /> YouTube source</div>
        <h2>Tempel link. Sisanya otomatis.</h2>
        <p>Public video maupun video Member yang akun browser kamu memang berhak akses.</p>
        <div className="url-row">
          <div className="url-input-wrap">
            <Youtube size={19} />
            <input
              value={url}
              onChange={(event) => onUrlChange(event.target.value)}
              onKeyDown={(event) => event.key === "Enter" && onInspect()}
              placeholder="https://www.youtube.com/watch?v=..."
              disabled={busy}
            />
          </div>
          <button className="primary-button" onClick={onInspect} disabled={busy || inspecting}>
            {inspecting ? <LoaderCircle className="spin" size={18} /> : <Search size={18} />}
            {inspecting ? "Checking" : "Check video"}
          </button>
        </div>
        <div className="helper-row">
          <span>Untuk Member-only, pilih browser login di Settings.</span>
          <button className="link-button" onClick={onOpenSettings}>YouTube access <ChevronRight size={14} /></button>
        </div>
      </div>

      {metadata && <VideoCard metadata={metadata} />}
    </>
  );
}

function VideoCard({ metadata }: { metadata: VideoMetadata }) {
  return (
    <div className="video-card card">
      <div className="thumbnail-wrap">
        {metadata.thumbnail ? <img src={metadata.thumbnail} alt="Video thumbnail" /> : <div className="thumbnail-placeholder"><Youtube size={34} /></div>}
        <span className="duration-chip"><Clock3 size={13} /> {formatDuration(metadata.duration)}</span>
      </div>
      <div className="video-info">
        <span className="success-label"><Check size={14} /> Video ready</span>
        <h3>{metadata.title}</h3>
        <p>{metadata.channel}</p>
        <div className="video-meta-line">
          {metadata.availability && <span>{metadata.availability}</span>}
          <span>{formatDuration(metadata.duration)}</span>
        </div>
      </div>
    </div>
  );
}
