import { CheckIcon } from "@phosphor-icons/react/dist/csr/Check";
import { CaretRightIcon } from "@phosphor-icons/react/dist/csr/CaretRight";
import { StopCircleIcon } from "@phosphor-icons/react/dist/csr/StopCircle";
import { ClockIcon } from "@phosphor-icons/react/dist/csr/Clock";
import { CircleNotchIcon } from "@phosphor-icons/react/dist/csr/CircleNotch";
import { XIcon } from "@phosphor-icons/react/dist/csr/X";
import { VideoCameraIcon } from "@phosphor-icons/react/dist/csr/VideoCamera";
import type { VideoMetadata } from "../../types";
import { formatDuration } from "../../lib/format";
import { useI18n } from "../../i18n";

type SourceCardProps = {
  url: string;
  busy: boolean;
  inspecting: boolean;
  cancelling: boolean;
  metadata: VideoMetadata | null;
  hasResult: boolean;
  onUrlChange: (url: string) => void;
  onInspect: () => void;
  onCancel: () => void;
  onClear: () => void;
  onOpenSettings: () => void;
};

export function SourceCard({
  url,
  busy,
  inspecting,
  cancelling,
  metadata,
  hasResult,
  onUrlChange,
  onInspect,
  onCancel,
  onClear,
  onOpenSettings,
}: SourceCardProps) {
  const { t } = useI18n();

  return (
    <div className="source-card-section">
      <div className="card source-hero-card">
        <div className="hero-content">
          <label className="source-label" htmlFor="source-url">{t("source.urlLabel")}</label>

          <div className="url-input-container">
            <div className="url-input-box">
              <div className="url-input-icon">
                <VideoCameraIcon aria-hidden="true" size={19} className="yt-red-icon" />
              </div>
              <input
                id="source-url"
                type="text"
                value={url}
                onChange={(event) => onUrlChange(event.target.value)}
                onKeyDown={(event) => event.key === "Enter" && !busy && !inspecting && onInspect()}
                placeholder={t("source.placeholder")}
                aria-label={t("source.urlLabel")}
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
                  <XIcon size={16} />
                </button>
              )}
            </div>

            <button
              type="button"
              className="inspect-action-btn"
              onClick={inspecting ? onCancel : onInspect}
              disabled={cancelling || (!inspecting && (busy || !url.trim()))}
            >
              {inspecting ? (
                <>
                  {cancelling ? <CircleNotchIcon aria-hidden="true" className="spin" size={17} /> : <StopCircleIcon aria-hidden="true" size={17} />}
                  <span>{cancelling ? t("source.cancelling") : t("source.cancelChecking")}</span>
                </>
              ) : (
                <>
                  <CaretRightIcon aria-hidden="true" size={17} />
                  <span>{t("source.checkVideo")}</span>
                </>
              )}
            </button>
          </div>

          <div className="hero-footer-tips">
            <span>{t("source.memberHint")}</span>
            <button type="button" className="settings-link-btn" onClick={onOpenSettings}>
              <span>{t("source.access")}</span>
              <CaretRightIcon aria-hidden="true" size={14} />
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
            <img
              src={metadata.thumbnail}
              alt={metadata.title}
              className="preview-thumb-img"
              loading="lazy"
              referrerPolicy="no-referrer"
            />
            <div className="preview-duration-pill">
              <ClockIcon aria-hidden="true" size={12} />
              <span>{formatDuration(metadata.duration)}</span>
            </div>
          </div>
        ) : (
          <div className="preview-thumb-placeholder">
            <VideoCameraIcon aria-hidden="true" size={36} className="yt-placeholder-icon" />
          </div>
        )}
      </div>

      <div className="preview-details-col">
        <div className="preview-status-strip">
          <span className="status-badge-ready">
            <CheckIcon aria-hidden="true" size={13} />
            <span>{t("source.videoReady")}</span>
          </span>
          {metadata.source && (
            <span className="status-badge-availability">{metadata.source}</span>
          )}
          {metadata.availability && (
            <span className="status-badge-availability">{metadata.availability}</span>
          )}
        </div>

        <h2 className="preview-video-title" title={metadata.title}>
          {metadata.title}
        </h2>

        <div className="preview-channel-row">
          <span className="preview-channel-name">{metadata.channel}</span>
          <span className="meta-separator">•</span>
          <span className="preview-duration-text">{formatDuration(metadata.duration)}</span>
        </div>
      </div>
    </div>
  );
}
