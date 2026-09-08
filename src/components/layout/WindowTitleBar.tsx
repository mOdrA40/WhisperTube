import { Minus, Square, X } from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { WhisperTubeLogo } from "../common/WhisperTubeLogo";

function supportsCustomTitleBar() {
  if (typeof navigator === "undefined") return false;
  return /windows|linux/i.test(navigator.userAgent);
}

export function WindowTitleBar() {
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
      <div className="window-controls" aria-label="Window controls">
        <button type="button" className="window-control-button" onClick={() => void appWindow.minimize()} aria-label="Minimize window" title="Minimize">
          <Minus size={15} strokeWidth={1.8} />
        </button>
        <button type="button" className="window-control-button" onClick={() => void appWindow.toggleMaximize()} aria-label="Maximize window" title="Maximize">
          <Square size={13} strokeWidth={1.8} />
        </button>
        <button type="button" className="window-control-button close" onClick={() => void appWindow.close()} aria-label="Close window" title="Close">
          <X size={16} strokeWidth={1.8} />
        </button>
      </div>
    </div>
  );
}
