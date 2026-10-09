import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import mark from "../../assets/brand/folded-ribbon/mark.svg?no-inline";
import lettering from "../../assets/brand/folded-ribbon/lettering.svg?no-inline";
import ribbonMask from "../../assets/brand/folded-ribbon/ribbon-mask.svg?no-inline";
import { confirmationDelay, startupLabel, type StartupSnapshot } from "./startupState";
import "../shared/base.css";
import "./startup.css";

// The startup document deliberately has no React dependency.
export function mountStartup(root: HTMLElement, earlyPresentation = false) {
  root.innerHTML = `<main class="startup-stage" aria-live="polite">
    <div class="startup-signature waiting">
      <div class="startup-backdrop" aria-hidden="true"></div>
      <div class="startup-ink">
        <div class="startup-mark"><img alt=""><span class="startup-sheen"></span></div>
        <div class="startup-lettering"><img alt="Translay"><span class="startup-word-sheen"></span></div>
      </div>
      <span class="startup-label"></span>
    </div>
  </main>`;
  root.querySelector<HTMLImageElement>(".startup-mark img")!.src = mark;
  root.querySelector<HTMLImageElement>(".startup-lettering img")!.src = lettering;
  const signature = root.querySelector<HTMLElement>(".startup-signature")!;
  const label = signature.querySelector<HTMLElement>(".startup-label")!;
  let disposed = false, busy = false, generation = 0, started = 0, scheduled = false;
  let poll: ReturnType<typeof setTimeout> | undefined;
  let slow: ReturnType<typeof setTimeout> | undefined;
  const timers: ReturnType<typeof setTimeout>[] = [];
  let unlisten: (() => void) | undefined;
  function begin(next: StartupSnapshot) {
    timers.forEach(clearTimeout); timers.length = 0; clearTimeout(slow);
    generation = next.generation; started = performance.now(); scheduled = false;
    // Keep the already decoded images and their rendering surfaces in place.
    // Commit the reset before restarting a repeated presentation's CSS timeline.
    signature.className = "startup-signature waiting";
    label.textContent = "";
    void signature.offsetWidth;
    signature.className = `startup-signature playing ${next.phase === "already" ? "already" : ""}`;
    const id = generation;
    slow = setTimeout(() => {
      if (!disposed && generation === id && !scheduled) label.textContent = "正在启动";
    }, 1900);
  }
  const later = (fn: () => void, delay: number, id: number) => {
    timers.push(setTimeout(() => { if (!disposed && generation === id) fn(); }, delay));
  };
  async function sync(reveal = false) {
    if (disposed || busy) return;
    busy = true;
    try {
      const next = await invoke<StartupSnapshot>("startup_status", { reveal });
      if (disposed || !next.visible || next.generation < generation) return;
      if (next.generation !== generation) {
        begin(next);
        if (!reveal) await invoke("startup_status", { reveal: true });
      }
      const delay = confirmationDelay(next.phase, performance.now() - started);
      if (delay !== null && !scheduled) {
        scheduled = true; clearTimeout(slow);
        const id = generation;
        later(() => {
          label.textContent = startupLabel(next.phase);
          later(() => {
            signature.classList.add("leaving");
            later(() => {
              void invoke("finish_startup", { generation: id }).then(() => {
                if (!disposed && generation === id) signature.className = "startup-signature waiting";
              }).catch(console.error);
            }, 1230, id);
          }, next.phase === "setup" ? 1050 : 750, id);
        }, delay, id);
      }
    } catch (error) { console.error("Startup presentation sync failed", error); }
    finally { busy = false; }
  }
  async function initialize() {
    // CSS masks are not covered by querying <img>; load the T sweep mask too,
    // otherwise its first visible frame can arrive partway through the sweep.
    const mask = new Image();
    mask.src = ribbonMask;
    await Promise.all([...Array.from(root.querySelectorAll("img")), mask].map(img => img.decode().catch(() => {})));
    if (disposed) return;
    // Native creation explicitly permits this only for a visible cold launch.
    // Paint while the main thread is still preparing the other WebViews.
    if (earlyPresentation) begin({ generation: 1, phase: "loading", visible: true });
    unlisten = await listen("startup-repeat", () => { void sync(); });
    if (disposed) { unlisten(); return; }
    await sync(true);
    const tick = async () => { await sync(); if (!disposed) poll = setTimeout(tick, 300); };
    poll = setTimeout(tick, 300);
  }
  void initialize().catch(console.error);
  return () => {
    disposed = true; clearTimeout(poll); clearTimeout(slow);
    timers.forEach(clearTimeout); unlisten?.();
  };
}
