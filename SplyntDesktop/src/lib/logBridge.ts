import { invoke } from "@tauri-apps/api/core";

/// Forwards what the webview knows to the native host's stderr, so `tauri dev`
/// output and any captured terminal log carry it. Without this the Rust side is
/// silent and the webview console is only visible to someone with the inspector
/// open on the machine at the time — which is no use for a problem reported
/// after the fact.
///
/// Warnings, errors and anything uncaught only. `console.log` is deliberately
/// left alone: Vite's HMR chatter would bury the signal.
let forwarding = false;

function render(value: unknown): string {
  if (typeof value === "string") return value;
  if (value instanceof Error) return `${value.name}: ${value.message}${value.stack ? `\n${value.stack}` : ""}`;
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

function forward(level: "warn" | "error", parts: unknown[]) {
  // A failed invoke calls console.error, which would call this again.
  if (forwarding) return;
  forwarding = true;
  try {
    void invoke("log_message", { level, message: parts.map(render).join(" ") }).catch(() => undefined);
  } finally {
    forwarding = false;
  }
}

export function installLogBridge() {
  const original = { warn: console.warn.bind(console), error: console.error.bind(console) };
  console.warn = (...parts: unknown[]) => { original.warn(...parts); forward("warn", parts); };
  console.error = (...parts: unknown[]) => { original.error(...parts); forward("error", parts); };

  window.addEventListener("error", (event) => {
    forward("error", [`uncaught: ${event.message}`, `${event.filename}:${event.lineno}:${event.colno}`]);
  });
  window.addEventListener("unhandledrejection", (event) => {
    forward("error", ["unhandled rejection:", event.reason]);
  });

  // A breadcrumb per session, so an empty log reads as "nothing went wrong"
  // rather than "the bridge was never installed".
  void invoke("log_message", {
    level: "info",
    message: `log bridge ready · ${document.documentElement.dataset.platform ?? "unknown"}`,
  }).catch(() => undefined);
}
