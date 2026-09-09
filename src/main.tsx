import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { AppErrorBoundary } from "./components/common/AppErrorBoundary";
import { I18nProvider } from "./i18n";
import "./styles.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <AppErrorBoundary>
    <I18nProvider>
      <React.StrictMode>
        <App />
      </React.StrictMode>
    </I18nProvider>
  </AppErrorBoundary>,
);
