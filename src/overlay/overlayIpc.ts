import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { CapturePayload } from "../shared/types";

const commands = {
  acknowledgeCapture: "ack_capture",
  copyTranslation: "copy_translation",
  dismissOverlay: "dismiss_overlay",
  fitOverlayHeight: "fit_overlay_height",
  getLatestCapture: "get_latest_capture",
  overlayFrontendReady: "overlay_frontend_ready",
  retryCapture: "retry_capture",
  setOverlayHovered: "set_overlay_hovered",
} as const;

const events = {
  captureResult: "capture-result",
  dismissRequested: "overlay-dismiss-requested",
} as const;

export function acknowledgeCapture(requestId: number): Promise<boolean> {
  return invoke<boolean>(commands.acknowledgeCapture, { requestId });
}

export function getLatestCapture(): Promise<CapturePayload | null> {
  return invoke<CapturePayload | null>(commands.getLatestCapture);
}

export function notifyOverlayFrontendReady(): Promise<void> {
  return invoke(commands.overlayFrontendReady);
}

export function setOverlayHovered(
  requestId: number,
  hovered: boolean,
): Promise<boolean> {
  return invoke<boolean>(commands.setOverlayHovered, { requestId, hovered });
}

export function dismissOverlay(requestId: number): Promise<boolean> {
  return invoke<boolean>(commands.dismissOverlay, { requestId });
}

export function retryCapture(): Promise<void> {
  return invoke(commands.retryCapture);
}

export function copyTranslation(text: string): Promise<void> {
  return invoke(commands.copyTranslation, { text });
}

export function fitOverlayHeight(
  requestId: number,
  logicalHeight: number,
): Promise<boolean> {
  return invoke<boolean>(commands.fitOverlayHeight, {
    requestId,
    logicalHeight,
  });
}

export function onCaptureResult(
  handler: () => void,
): Promise<UnlistenFn> {
  return listen<CapturePayload>(events.captureResult, handler);
}

export function onOverlayDismissRequested(
  handler: (requestId: number) => void,
): Promise<UnlistenFn> {
  return listen<number>(events.dismissRequested, ({ payload }) => {
    handler(Number(payload));
  });
}
