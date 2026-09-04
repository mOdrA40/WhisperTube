import type { ReactNode } from "react";
import {
  Cpu,
  History,
  LockKeyhole,
  Settings2,
  Video,
  Zap,
} from "lucide-react";
import type { AppTab, SystemStatus } from "../../types";
import { useI18n } from "../../i18n";
import { WhisperTubeLogo } from "../common/WhisperTubeLogo";

type AppShellProps = {
  tab: AppTab;
  historyCount: number;
  runtimeReady: boolean;
  system: SystemStatus | null;
  onTabChange: (tab: AppTab) => void;
  children: ReactNode;
};

export function AppShell({
  tab,
  historyCount,
  runtimeReady,
  system,
  onTabChange,
  children,
}: AppShellProps) {
  const { t } = useI18n();
  const pageCopy: Record<AppTab, { title: string; description: string }> = {
    transcribe: { title: t("page.transcribe.title"), description: t("page.transcribe.description") },
    history: { title: t("page.history.title"), description: t("page.history.description") },
    settings: { title: t("page.settings.title"), description: t("page.settings.description") },
  };
  const copy = pageCopy[tab];

  return (
    <div className="app-shell">
      <div className="ambient-backdrop" aria-hidden="true" />

      <aside className="sidebar">
        <div className="brand-header">
          <div className="brand-logo-wrap">
            <WhisperTubeLogo size={36} />
          </div>
          <div className="brand-identity">
            <strong className="brand-name">WhisperTube</strong>
            <span className="brand-tagline">{t("app.tagline")}</span>
          </div>
        </div>

        <nav className="nav-container">
          <NavItem
            active={tab === "transcribe"}
            icon={<Video size={18} />}
            onClick={() => onTabChange("transcribe")}
          >
            {t("nav.transcribe")}
          </NavItem>
          <NavItem
            active={tab === "history"}
            icon={<History size={18} />}
            badge={historyCount > 0 ? historyCount : undefined}
            onClick={() => onTabChange("history")}
          >
            {t("nav.history")}
          </NavItem>
          <NavItem
            active={tab === "settings"}
            icon={<Settings2 size={18} />}
            onClick={() => onTabChange("settings")}
          >
            {t("nav.settings")}
          </NavItem>
        </nav>

        <div className="sidebar-footer">
          <div className="runtime-card">
            <div className="runtime-header">
              <span className={`status-dot ${runtimeReady ? "dot-online" : "dot-warning"}`} />
              <span className="runtime-label">
                {runtimeReady ? t("status.ready") : t("status.setupNeeded")}
              </span>
            </div>

            <div className="hardware-spec">
              <div className="hardware-icon-box">
                {system?.gpuName ? <Zap size={14} className="nvidia-icon" /> : <Cpu size={14} />}
              </div>
              <div className="hardware-details">
                <span className="hardware-name">
                  {system?.gpuName ?? `${system?.cpuThreads ?? "—"} ${t("hardware.cpuThreads")}`}
                </span>
                <span className="hardware-sub">
                  {system?.nvidia && system.gpuMemoryMb
                    ? `${Math.round(system.gpuMemoryMb / 1024)} ${t("hardware.vram")}`
                    : system?.gpuName
                      ? t("hardware.gpu")
                      : t("hardware.cpuCompute")}
                </span>
              </div>
            </div>
          </div>
        </div>
      </aside>

      <main className="main-area">
        <header className="topbar">
          <div className="topbar-title-group">
            <h1 className="topbar-heading">{copy.title}</h1>
            <p className="topbar-subtitle">{copy.description}</p>
          </div>

          <div className="privacy-chip">
            <LockKeyhole size={14} className="privacy-icon" />
            <span className="privacy-text">{t("app.localInference")}</span>
          </div>
        </header>

        <div className="main-content-flow">{children}</div>
      </main>
    </div>
  );
}

type NavItemProps = {
  active: boolean;
  icon: ReactNode;
  badge?: number;
  onClick: () => void;
  children: ReactNode;
};

function NavItem({ active, icon, badge, onClick, children }: NavItemProps) {
  return (
    <button
      type="button"
      className={`nav-item ${active ? "active" : ""}`}
      onClick={onClick}
    >
      <div className="nav-item-icon">{icon}</div>
      <span className="nav-item-label">{children}</span>
      {badge !== undefined && <span className="nav-item-badge">{badge}</span>}
    </button>
  );
}
