import {
  Check,
  ChevronRight,
  Clock3,
  LoaderCircle,
  Search,
  X,
  Youtube,
} from "lucide-react";
import type { VideoMetadata } from "../../types";
import { formatDuration } from "../../lib/format";
import { useI18n } from "../../i18n";

type SourceCardProps = {
  url: string;
  busy: boolean;
  inspecting: boolean;
  metadata: VideoMetadata | null;
  hasResult: boolean;
  onUrlChange: (url: string) => void;
  onInspect: () => void;
  onClear: () => void;
  onOpenSettings: () => void;
};

export function SourceCard({
  url,
  busy,
  inspecting,
  metadata,
  hasResult,
  onUrlChange,
  onInspect,
  onClear,
  onOpenSettings,
}: SourceCardProps) {
  const { t } = useI18n();

  return (
    <div className="source-card-section">
      <div className="card source-hero-card">
        <div className="hero-content">
          <div className="hero-badge">
            <Youtube size={14} className="hero-badge-icon" />
            <span className="hero-badge-text">{t("source.badge")}</span>
          </div>

          <h2 className="hero-headline">{t("source.headline")}</h2>
          <p className="hero-description">
            {t("source.description")}
          </p>

          <div className="url-input-container">
            <div className="url-input-box">
              <div className="url-input-icon">
                <Youtube size={19} className="yt-red-icon" />
              </div>
              <input
                type="text"
                value={url}
                onChange={(event) => onUrlChange(event.target.value)}
                onKeyDown={(event) => event.key === "Enter" && !busy && !inspecting && onInspect()}
                placeholder={t("source.placeholder")}
                disabled={busy || inspecting}
                className="url-text-input"
              />
              {(url.trim() || metadata || hasResult) && (
                <button
                  type="button"
                  className="clear-input-button"
                  onClick={onClear}
                  disabled={busy || inspecting}
                  title={t("source.clear")}
                  aria-label={t("source.clear")}
                >
                  <X size={16} />
                </button>
              )}
            </div>

            <button
              type="button"
              className="inspect-action-btn"
              onClick={onInspect}
              disabled={busy || inspecting || !url.trim()}
            >
              {inspecting ? (
                <>
                  <LoaderCircle className="spin" size={17} />
                  <span>{t("source.checking")}</span>
                </>
              ) : (
                <>
                  <Search size={17} />
                  <span>{t("source.checkVideo")}</span>
                </>
              )}
            </button>
          </div>

          <div className="hero-footer-tips">
            <span>{t("source.memberHint")}</span>
            <button type="button" className="settings-link-btn" onClick={onOpenSettings}>
              <span>{t("source.access")}</span>
              <ChevronRight size={14} />
            </button>
          </div>
        </div>
      </div>

      {metadata && <VideoPreviewCard metadata={metadata} />}
    </div>
  );
}

function VideoPreviewCard({ metadata }: { metadata: VideoMetadata }) {
  const { t } = useI18n();

  return (
    <div className="card video-preview-card">
      <div className="preview-thumbnail-col">
        {metadata.thumbnail ? (
          <div className="preview-thumbnail-frame">
            <img src={metadata.thumbnail} alt={metadata.title} className="preview-thumb-img" />
            <div className="preview-duration-pill">
              <Clock3 size={12} />
              <span>{formatDuration(metadata.duration)}</span>
            </div>
          </div>
        ) : (
          <div className="preview-thumb-placeholder">
            <Youtube size={36} className="yt-placeholder-icon" />
          </div>
        )}
      </div>

      <div className="preview-details-col">
        <div className="preview-status-strip">
          <span className="status-badge-ready">
            <Check size={13} />
            <span>{t("source.videoReady")}</span>
          </span>
          {metadata.availability && (
            <span className="status-badge-availability">{metadata.availability}</span>
          )}
        </div>

        <h3 className="preview-video-title" title={metadata.title}>
          {metadata.title}
        </h3>

        <div className="preview-channel-row">
          <span className="preview-channel-name">{metadata.channel}</span>
          <span className="meta-separator">•</span>
          <span className="preview-duration-text">{formatDuration(metadata.duration)}</span>
        </div>
      </div>
    </div>
  );
}
