import { getCurrentWindow } from "@tauri-apps/api/window";
import { Copy, Minus, Square, X } from "lucide-react";
import { useEffect, useState } from "react";

export function WindowControls() {
  if (document.documentElement.dataset.platform !== "windows") return null;
  return <WindowsWindowControls />;
}

function WindowsWindowControls() {
  const [appWindow] = useState(getCurrentWindow);
  const [maximized, setMaximized] = useState(false);
  const [fullscreen, setFullscreen] = useState(false);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    const sync = () => {
      void Promise.all([appWindow.isMaximized(), appWindow.isFullscreen()]).then(([isMaximized, isFullscreen]) => {
        if (active) {
          setMaximized(isMaximized);
          setFullscreen(isFullscreen);
        }
      }).catch(() => undefined);
    };
    sync();
    void appWindow.onResized(sync).then((remove) => {
      if (active) unlisten = remove;
      else remove();
    }).catch(() => undefined);
    return () => {
      active = false;
      unlisten?.();
    };
  }, [appWindow]);

  const act = (action: Promise<void>) => {
    void action.catch((error) => console.warn("Window action failed", error));
  };

  if (fullscreen) return null;

  return (
    <div aria-label="Window controls" className="window-controls" role="group">
      <button aria-label="Minimize window" onClick={() => act(appWindow.minimize())} title="Minimize" type="button"><Minus size={16} strokeWidth={1.8} /></button>
      <button aria-label={maximized ? "Restore window" : "Maximize window"} onClick={() => act(appWindow.toggleMaximize())} title={maximized ? "Restore" : "Maximize"} type="button">{maximized ? <Copy size={14} strokeWidth={1.8} /> : <Square size={13} strokeWidth={1.8} />}</button>
      <button aria-label="Close window" className="window-controls__close" onClick={() => act(appWindow.close())} title="Close" type="button"><X size={17} strokeWidth={1.8} /></button>
    </div>
  );
}
