import type { ErrorInfo, ReactNode } from "react";
import { Component } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { relaunch } from "@tauri-apps/plugin-process";

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
        await getCurrentWindow().close();
      } catch (closeCause) {
        console.error("WhisperTube gagal menutup window setelah crash UI.", closeCause);
        window.location.reload();
      }
    }
  };

  render() {
    if (this.state.error) {
      return (
        <main className="app-crash-fallback" role="alert">
          <div className="app-crash-card">
            <span className="eyebrow">WhisperTube</span>
            <h1>Antarmuka mengalami masalah</h1>
            <p>Aplikasi tidak dapat menampilkan halaman ini dengan aman. Muat ulang untuk mencoba lagi.</p>
            <button type="button" className="primary-button" onClick={this.reload}>
              Muat ulang aplikasi
            </button>
          </div>
        </main>
      );
    }
    return this.props.children;
  }
}
