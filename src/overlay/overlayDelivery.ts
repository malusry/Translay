import type { CapturePayload } from "../shared/types";

export interface CaptureSyncDependencies {
  getLatest: () => Promise<CapturePayload | null>;
  apply: (payload: CapturePayload) => boolean;
  acknowledge: (requestId: number) => Promise<boolean>;
  onError?: (
    stage: "get-latest" | "acknowledge",
    error: unknown,
  ) => void;
}

export async function syncLatestCapture(
  dependencies: CaptureSyncDependencies,
): Promise<boolean> {
  try {
    const latest = await dependencies.getLatest();
    if (!latest || !dependencies.apply(latest)) return false;
    try {
      return await dependencies.acknowledge(latest.requestId);
    } catch (error) {
      dependencies.onError?.("acknowledge", error);
      return false;
    }
  } catch (error) {
    dependencies.onError?.("get-latest", error);
    return false;
  }
}

export interface BoundedPollingOptions {
  intervalMs?: number;
  durationMs?: number;
  now?: () => number;
  onStop?: () => void;
}

export function startBoundedPolling(
  synchronize: () => Promise<boolean>,
  options: BoundedPollingOptions = {},
): () => void {
  const intervalMs = options.intervalMs ?? 75;
  const durationMs = options.durationMs ?? 1_500;
  const now = options.now ?? Date.now;
  const deadline = now() + durationMs;
  let active = true;
  let timer: ReturnType<typeof setTimeout> | undefined;

  const stop = () => {
    if (!active) return;
    active = false;
    if (timer !== undefined) clearTimeout(timer);
    options.onStop?.();
  };

  const tick = async () => {
    if (!active) return;
    if (await synchronize()) {
      stop();
      return;
    }
    if (!active || now() >= deadline) {
      stop();
      return;
    }
    timer = setTimeout(() => {
      void tick();
    }, intervalMs);
  };

  void tick();
  return stop;
}

export interface VisibilityDocument {
  readonly visibilityState: DocumentVisibilityState;
  addEventListener(
    type: "visibilitychange",
    listener: EventListenerOrEventListenerObject,
  ): void;
  removeEventListener(
    type: "visibilitychange",
    listener: EventListenerOrEventListenerObject,
  ): void;
}

export interface PageShowWindow {
  addEventListener(
    type: "pageshow",
    listener: EventListenerOrEventListenerObject,
  ): void;
  removeEventListener(
    type: "pageshow",
    listener: EventListenerOrEventListenerObject,
  ): void;
}

export function attachLifecycleSynchronization(
  documentTarget: VisibilityDocument,
  windowTarget: PageShowWindow,
  synchronize: () => void,
): () => void {
  const onVisibilityChange = () => {
    if (documentTarget.visibilityState === "visible") synchronize();
  };
  const onPageShow = () => synchronize();
  documentTarget.addEventListener("visibilitychange", onVisibilityChange);
  windowTarget.addEventListener("pageshow", onPageShow);
  return () => {
    documentTarget.removeEventListener("visibilitychange", onVisibilityChange);
    windowTarget.removeEventListener("pageshow", onPageShow);
  };
}
