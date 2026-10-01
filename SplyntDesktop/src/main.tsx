import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { installLogBridge } from "./lib/logBridge";
import "./styles.css";

document.documentElement.dataset.platform = navigator.userAgent.includes("Mac")
  ? "macos"
  : navigator.userAgent.includes("Windows")
    ? "windows"
    : "linux";

// After the platform stamp, so the session breadcrumb can name it.
installLogBridge();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </StrictMode>,
);
