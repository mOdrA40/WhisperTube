import type { ErrorInfo, ReactNode } from "react";
import { Component } from "react";

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

  private reload = () => {
    window.location.reload();
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
