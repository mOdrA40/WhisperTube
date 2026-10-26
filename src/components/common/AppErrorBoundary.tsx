import type { ErrorInfo, ReactNode } from "react";
import { Component } from "react";
import { exit, relaunch } from "@tauri-apps/plugin-process";

// This fallback must also work when the context provider itself failed.
const recoveryMessages = {
  en: {
    title: "The interface encountered a problem",
    detail: "The application cannot safely display this page. Restart the app to try again.",
    button: "Restart application",
  },
  id: {
    title: "Antarmuka mengalami masalah",
    detail: "Aplikasi tidak dapat menampilkan halaman ini dengan aman. Mulai ulang aplikasi untuk mencoba lagi.",
    button: "Mulai ulang aplikasi",
  },
  zh: {
    title: "界面出现问题",
    detail: "应用无法安全显示此页面。请重启应用后重试。",
    button: "重启应用",
  },
};

function recoveryText() {
  try {
    const language = window.localStorage.getItem("whispertube.ui-language");
    return recoveryMessages[language === "id" || language === "zh" ? language : "en"];
  } catch {
    return recoveryMessages.en;
  }
}

type AppErrorBoundaryProps = {
  children: ReactNode;
};

type AppErrorBoundaryState = {
  error: Error | null;
};

export class AppErrorBoundary extends Component<AppErrorBoundaryProps, AppErrorBoundaryState> {
  state: AppErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): AppErrorBoundaryState {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("WhisperTube UI gagal dirender.", error, info.componentStack);
  }

  private reload = async () => {
    try {
      // A WebView-only reload leaves Rust operations and child processes alive.
      // Relaunching lets Tauri's exit handler terminate active work first.
      await relaunch();
    } catch (cause) {
      console.error("WhisperTube gagal melakukan relaunch setelah crash UI.", cause);
      try {
        // Window close may now hide an active job to the tray. Recovery must
        // really exit so it cannot strand a crashed interface in the background.
        await exit(1);
      } catch (exitCause) {
        console.error("WhisperTube gagal keluar setelah crash UI.", exitCause);
        window.location.reload();
      }
    }
  };

  render() {
    if (this.state.error) {
      const messages = recoveryText();
      return (
        <main className="app-crash-fallback" role="alert">
          <div className="app-crash-card">
            <span className="eyebrow">WhisperTube</span>
            <h1>{messages.title}</h1>
            <p>{messages.detail}</p>
            <button type="button" className="primary-button" onClick={this.reload}>
              {messages.button}
            </button>
          </div>
        </main>
      );
    }
    return this.props.children;
  }
}
