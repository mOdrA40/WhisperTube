import type { ReactNode } from "react";
import {
  Cpu,
  History,
  LockKeyhole,
  MonitorCog,
  Settings2,
  Sparkles,
  Youtube,
  Zap,
} from "lucide-react";
import type { AppTab, SystemStatus } from "../../types";

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

export function AppShell({ tab, historyCount, runtimeReady, system, onTabChange, children }: AppShellProps) {
  const copy = pageCopy[tab];

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-mark"><Sparkles size={20} /></div>
          <div>
            <strong>WhisperTube</strong>
            <span>Local AI transcription</span>
          </div>
        </div>

        <nav>
          <NavItem active={tab === "transcribe"} icon={<Youtube size={18} />} onClick={() => onTabChange("transcribe")}>
            Transcribe
          </NavItem>
          <NavItem active={tab === "history"} icon={<History size={18} />} onClick={() => onTabChange("history")}>
            History
            {historyCount > 0 && <span className="nav-badge">{historyCount}</span>}
          </NavItem>
          <NavItem active={tab === "settings"} icon={<Settings2 size={18} />} onClick={() => onTabChange("settings")}>
            Settings
          </NavItem>
        </nav>

        <div className="sidebar-status">
          <div className="status-title"><MonitorCog size={16} /> Runtime</div>
          <div className={runtimeReady ? "status-pill good" : "status-pill warn"}>
            <span className="dot" /> {runtimeReady ? "Ready" : "Setup needed"}
          </div>
          <div className="hardware-line">
            {system?.nvidia ? <Zap size={14} /> : <Cpu size={14} />}
            <span>{system?.gpuName ?? `${system?.cpuThreads ?? "—"} CPU threads`}</span>
          </div>
        </div>
      </aside>

      <main className="main-area">
        <header className="topbar">
          <div>
            <h1>{copy.title}</h1>
            <p>{copy.description}</p>
          </div>
          <div className="privacy-badge"><LockKeyhole size={15} /> Local inference</div>
        </header>
        {children}
      </main>
    </div>
  );
}

type NavItemProps = {
  active: boolean;
  icon: ReactNode;
  onClick: () => void;
  children: ReactNode;
};

function NavItem({ active, icon, onClick, children }: NavItemProps) {
  return (
    <button className={active ? "nav-item active" : "nav-item"} onClick={onClick}>
      {icon} {children}
    </button>
  );
}
