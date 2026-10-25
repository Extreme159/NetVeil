import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./fonts.css";
import "./index.css";
import { applyScale } from "./lib/uiScale";
import { applyContentScale } from "./lib/contentScale";
import { t } from "./lib/i18n";

// Kaydedilmis UI scale tercihini acilista uygula (zoom + pencere boyutu).
void applyScale();
// Kaydedilmis icerik boyutu tercihini acilista uygula (pencere sabit, sadece icerik).
applyContentScale();

class ErrorBoundary extends React.Component<
  { children: React.ReactNode },
  { error: Error | null }
> {
  constructor(props: { children: React.ReactNode }) {
    super(props);
    this.state = { error: null };
  }
  static getDerivedStateFromError(error: Error) {
    return { error };
  }
  render() {
    if (this.state.error) {
      return (
        <div style={{
          padding: 32,
          fontFamily: "JetBrains Mono, monospace",
          background: "#0a0a0f",
          color: "#fca5a5",
          minHeight: "100vh",
          whiteSpace: "pre-wrap",
          fontSize: 13,
        }}>
          <strong style={{ color: "#f87171", fontSize: 16 }}>{t("render_error_title")}</strong>
          {"\n\n"}
          {this.state.error.message}
          {"\n\n"}
          {this.state.error.stack}
        </div>
      );
    }
    return this.props.children;
  }
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </React.StrictMode>
);
