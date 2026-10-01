import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import { AuthenticatedShell } from "./components/AuthenticatedShell";
import { Brand } from "./components/Brand";
import { LoginView } from "./components/LoginView";
import { cacheConnectedLibrary, readCachedProfile } from "./lib/persistence";
import type { ConnectedLibrary } from "./types";

function restoreFailureMessage(reason: unknown) {
  const message = typeof reason === "string" ? reason : reason instanceof Error ? reason.message : "";
  if (!message || /\bundefined\b|__TAURI|\binvoke\b/i.test(message)) {
    return "Splynt could not reach the desktop service. Close this window and reopen the app.";
  }
  return message;
}

export default function App() {
  const [library, setLibrary] = useState<ConnectedLibrary>();
  const [isRestoring, setIsRestoring] = useState(true);
  const [restoreSlow, setRestoreSlow] = useState(false);
  const [restoreError, setRestoreError] = useState<string>();
  const restoreIgnored = useRef(false);

  useEffect(() => {
    let active = true;
    restoreIgnored.current = false;
    const slowTimer = window.setTimeout(() => {
      if (active && !restoreIgnored.current) setRestoreSlow(true);
    }, 3_500);
    let timeoutTimer = 0;
    const timeout = new Promise<never>((_, reject) => {
      timeoutTimer = window.setTimeout(() => reject(new Error("The saved server took too long to respond.")), 12_000);
    });

    Promise.race([invoke<ConnectedLibrary | null>("restore_session"), timeout])
      .then((saved) => {
        if (active && !restoreIgnored.current && saved) {
          const online = { ...saved, connection: { status: "online" as const } };
          cacheConnectedLibrary(online);
          setLibrary(online);
        }
      })
      .catch((reason) => {
        if (!active || restoreIgnored.current) return;
        const message = restoreFailureMessage(reason);
        const cached = readCachedProfile();
        if (cached) {
          setLibrary({ ...cached.library, connection: { status: "offline", message } });
        } else {
          setRestoreError(message);
        }
      })
      .finally(() => {
        if (active && !restoreIgnored.current) setIsRestoring(false);
        window.clearTimeout(slowTimer);
        window.clearTimeout(timeoutTimer);
      });
    return () => {
      active = false;
      window.clearTimeout(slowTimer);
      window.clearTimeout(timeoutTimer);
    };
  }, []);

  const continueWithoutRestore = () => {
    restoreIgnored.current = true;
    const cached = readCachedProfile();
    if (cached) {
      setLibrary({
        ...cached.library,
        connection: { status: "offline", message: "Opened without waiting for the server." },
      });
    } else {
      setRestoreError("Automatic sign-in was skipped. Connect to a server below.");
    }
    setIsRestoring(false);
  };

  if (isRestoring) {
    return (
      <main className="splash-page" aria-label="Opening Splynt">
        <Brand />
        <span className="spinner" aria-hidden="true" />
        <div className="splash-page__status" role="status">
          <strong>{restoreSlow ? "Your server is taking a while" : "Opening your library"}</strong>
          <small>{restoreSlow ? "You can keep waiting or open Splynt without it." : "Restoring your last session…"}</small>
        </div>
        {restoreSlow && <button onClick={continueWithoutRestore} type="button">Continue without waiting</button>}
      </main>
    );
  }
  if (!library) return <LoginView initialError={restoreError} onConnected={(connected) => {
    const online = { ...connected, connection: { status: "online" as const } };
    cacheConnectedLibrary(online);
    setLibrary(online);
  }} />;
  return <AuthenticatedShell library={library} onConnectionRestored={(connected) => {
    const online = { ...connected, connection: { status: "online" as const } };
    cacheConnectedLibrary(online);
    setLibrary(online);
  }} onSignedOut={() => setLibrary(undefined)} />;
}
