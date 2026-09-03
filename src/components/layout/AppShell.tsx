import type { ReactNode } from "react";
import {
  Cpu,
  History,
  LockKeyhole,
  Settings2,
  Youtube,
  Zap,
} from "lucide-react";
import type { AppTab, SystemStatus } from "../../types";
import { WhisperTubeLogo } from "../common/WhisperTubeLogo";

type AppShellProps = {
  tab: AppTab;
  historyCount: number;
  runtimeReady: boolean;
  system: SystemStatus | null;
  onTabChange: (tab: AppTab) => void;
  children: ReactNode;
};

const pageCopy: Record<AppTab, { title: string; description: string }> = {
  transcribe: {
    title: "Transcribe video",
    description: "YouTube → local Whisper → transcript. Audio tidak dikirim ke cloud.",
  },
  history: {
    title: "History",
    description: "Buka kembali hasil transkripsi lokal.",
  },
  settings: {
    title: "Settings",
    description: "Atur model, autentikasi YouTube, dan compute backend.",
  },
};

export function AppShell({
  tab,
  historyCount,
  runtimeReady,
  system,
  onTabChange,
  children,
}: AppShellProps) {
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
            <span className="brand-tagline">Local Whisper transcription</span>
          </div>
        </div>

        <nav className="nav-container">
          <NavItem
            active={tab === "transcribe"}
            icon={<Youtube size={18} />}
            onClick={() => onTabChange("transcribe")}
          >
            Transcribe
          </NavItem>
          <NavItem
            active={tab === "history"}
            icon={<History size={18} />}
            badge={historyCount > 0 ? historyCount : undefined}
            onClick={() => onTabChange("history")}
          >
            History
          </NavItem>
          <NavItem
            active={tab === "settings"}
            icon={<Settings2 size={18} />}
            onClick={() => onTabChange("settings")}
          >
            Settings
          </NavItem>
        </nav>

        <div className="sidebar-footer">
          <div className="runtime-card">
            <div className="runtime-header">
              <span className={`status-dot ${runtimeReady ? "dot-online" : "dot-warning"}`} />
              <span className="runtime-label">{runtimeReady ? "Ready" : "Setup needed"}</span>
            </div>

            <div className="hardware-spec">
              <div className="hardware-icon-box">
                {system?.nvidia ? <Zap size={14} className="nvidia-icon" /> : <Cpu size={14} />}
              </div>
              <div className="hardware-details">
                <span className="hardware-name">
                  {system?.gpuName ?? `${system?.cpuThreads ?? "—"} CPU threads`}
                </span>
                <span className="hardware-sub">
                  {system?.nvidia && system.gpuMemoryMb
                    ? `${Math.round(system.gpuMemoryMb / 1024)} GB VRAM`
                    : "CPU Compute"}
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
            <span className="privacy-text">Local inference</span>
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
