import { Minus, Square, X } from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useI18n } from "../../i18n";
import { WhisperTubeLogo } from "../common/WhisperTubeLogo";

function supportsCustomTitleBar() {
  if (typeof navigator === "undefined") return false;
  return /windows|linux/i.test(navigator.userAgent);
}

export function WindowTitleBar() {
  const { t } = useI18n();
  if (!supportsCustomTitleBar()) return null;

  const appWindow = getCurrentWindow();

  return (
    <div className="window-titlebar" role="presentation">
      <div className="window-titlebar-drag-region" data-tauri-drag-region>
        <span className="window-titlebar-mark" aria-hidden="true">
          <WhisperTubeLogo size={20} />
        </span>
        <span className="window-titlebar-title">WhisperTube</span>
      </div>
      <div className="window-controls" aria-label={t("window.controls")}>
        <button type="button" className="window-control-button" onClick={() => void appWindow.minimize()} aria-label={t("window.minimize")} title={t("window.minimize")}>
          <Minus size={15} strokeWidth={1.8} />
        </button>
        <button type="button" className="window-control-button" onClick={() => void appWindow.toggleMaximize()} aria-label={t("window.maximize")} title={t("window.maximize")}>
          <Square size={13} strokeWidth={1.8} />
        </button>
        <button type="button" className="window-control-button close" onClick={() => void appWindow.close()} aria-label={t("window.close")} title={t("window.close")}>
          <X size={16} strokeWidth={1.8} />
        </button>
      </div>
    </div>
  );
}
