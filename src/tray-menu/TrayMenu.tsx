import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { uiFontsReady } from "../shared/uiFonts";
import "./trayMenu.css";

type Snapshot = { generation: number; open: boolean; modelLabel: string; modelEnabled: boolean; selectionEnabled: boolean };
function Icon({ kind }: { kind: "model" | "selection" | "settings" | "quit" | "check" }) {
  return <svg viewBox="0 0 24 24" aria-hidden="true">{kind === "model" ? <path d="M4 7h16m-4-4 4 4-4 4M20 17H4m4-4-4 4 4 4" /> :
    kind === "selection" ? <><path d="M8 3h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H8m8-18h-2a2 2 0 0 0-2 2m0 14a2 2 0 0 0 2 2h2" /><path d="M5 8H3v8h2m14-8h2v8h-2" /></> :
    kind === "settings" ? <><path d="M3 6h6m4 0h8M3 12h12m4 0h2M3 18h2m4 0h12" /><path d="M9 3v6m6 0v6M5 15v6" /></> :
    kind === "quit" ? <path d="M12 3v9m-6.3-6.3a9 9 0 1 0 12.6 0" /> : <path d="m5 12 4 4L19 6" />}</svg>;
}

export function TrayMenu() {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const latest = useRef(0), pending = useRef<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [keyboardNavigation, setKeyboardNavigation] = useState(false);
  const menu = useRef<HTMLDivElement>(null);
  useEffect(() => {
    let disposed = false; let stop: (() => void) | undefined;
    const apply = (value: Snapshot) => {
      if (disposed || value.generation < latest.current) return;
      if (value.generation > latest.current) { pending.current = null; setBusy(false); setKeyboardNavigation(false); }
      latest.current = value.generation; setSnapshot(value);
    };
    void listen<Snapshot>("tray-menu-open", event => apply(event.payload)).then(async unlisten => {
      if (disposed) { unlisten(); return; } stop = unlisten;
      apply(await invoke<Snapshot>("get_tray_menu"));
    }).catch(error => console.error("[Translay] tray menu initialization failed", error));
    return () => { disposed = true; stop?.(); };
  }, []);
  useEffect(() => {
    if (!snapshot?.open) return; const generation = snapshot.generation;
    let sent = false;
    const present = () => {
      if (sent) return;
      sent = true;
      void invoke("tray_menu_painted", { generation }).then(() => {
        if (latest.current === generation) menu.current?.focus();
      }).catch(error => console.error("[Translay] tray menu display failed", error));
    };
    let disposed = false;
    let frame = 0, fallback = 0;
    void uiFontsReady.then(() => {
      if (disposed || latest.current !== generation) return;
      frame = requestAnimationFrame(present);
      // Hidden WebViews may suspend animation frames until the native window shows.
      fallback = window.setTimeout(present, 64);
    });
    return () => { disposed = true; cancelAnimationFrame(frame); clearTimeout(fallback); };
  }, [snapshot?.generation, snapshot?.open]);
  const dismiss = () => { if (snapshot) void invoke("dismiss_tray_menu", { generation: snapshot.generation }).catch(console.error); };
  const act = (action: string) => {
    if (!snapshot?.open || pending.current !== null) return;
    const generation = snapshot.generation; pending.current = generation; setBusy(true);
    void invoke("tray_menu_action", { generation, action })
      .catch(error => console.error("[Translay] tray action failed", error))
      .finally(() => { if (pending.current === generation) { pending.current = null; setBusy(false); } });
  };
  const navigate = (event: KeyboardEvent) => {
    if (event.key === "Escape" || event.key === "Tab") { event.preventDefault(); dismiss(); return; }
    if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
    event.preventDefault(); const items = Array.from(menu.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? []);
    if (!items.length) return;
    setKeyboardNavigation(true);
    const current = items.indexOf(document.activeElement as HTMLButtonElement);
    const index = event.key === "Home" ? 0 : event.key === "End" ? items.length - 1 : current < 0 ? (event.key === "ArrowDown" ? 0 : items.length - 1) : (current + (event.key === "ArrowDown" ? 1 : -1) + items.length) % items.length;
    items[index].focus();
  };
  return <main className="tray-menu-stage" onContextMenu={event => event.preventDefault()} onPointerDown={event => { if (event.target === event.currentTarget) dismiss(); }}>
    {snapshot?.open && <div className="tray-menu" ref={menu} role="menu" tabIndex={-1} data-keyboard={keyboardNavigation} aria-label="Translay" onKeyDown={navigate} onPointerMove={() => setKeyboardNavigation(false)} onPointerDown={() => setKeyboardNavigation(false)}>
      <button role="menuitem" disabled={busy || !snapshot.modelEnabled} onClick={() => act("switch-model-backend")} title={!snapshot.modelEnabled ? "请先在配置中完成另一模型的设置" : undefined}><Icon kind="model" /><span>{snapshot.modelLabel}</span></button>
      <button role="menuitemcheckbox" aria-checked={snapshot.selectionEnabled} disabled={busy} onClick={() => act("selection-icon")}><Icon kind="selection" /><span>划词图标</span><span className="tray-menu-check" data-checked={snapshot.selectionEnabled}><Icon kind="check" /></span></button>
      <div className="tray-menu-divider" role="separator" />
      <button role="menuitem" className="tray-menu-settings" disabled={busy} onClick={() => act("settings")}><Icon kind="settings" /><span>配置</span></button>
      <div className="tray-menu-divider" role="separator" />
      <button role="menuitem" className="tray-menu-quit" disabled={busy} onClick={() => act("quit")}><Icon kind="quit" /><span>退出</span></button>
    </div>}
  </main>;
}
