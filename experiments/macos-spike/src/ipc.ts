import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface Snapshot {
  selectionIconEnabled: boolean;
  mode: "conversational" | "academic";
  revision: number;
  exiting: boolean;
}

export interface AccessibilityStatus {
  supported: boolean;
  trusted: boolean | null;
}

export const previewSnapshot: Snapshot = {
  selectionIconEnabled: true,
  mode: "conversational",
  revision: 0,
  exiting: false,
};

export const previewAccessibility: AccessibilityStatus = {
  supported: false,
  trusted: null,
};

/** A slow initial IPC response cannot replace a newer tray/menu event. */
export function acceptSnapshot(current: Snapshot | null, next: Snapshot): Snapshot {
  return current && current.revision > next.revision ? current : next;
}

export const native = isTauri();

export function getSnapshot(): Promise<Snapshot> {
  return native ? invoke("get_spike_state") : Promise.resolve(previewSnapshot);
}

export function getAccessibility(): Promise<AccessibilityStatus> {
  return native ? invoke("get_accessibility_status") : Promise.resolve(previewAccessibility);
}

export function requestAccessibility(): Promise<AccessibilityStatus> {
  return native
    ? invoke("request_accessibility")
    : Promise.reject(new Error("请在 Mac 原生程序中请求系统授权"));
}

export function openSystemSettings(): Promise<void> {
  return native
    ? invoke("open_system_settings")
    : Promise.reject(new Error("请在 Mac 原生程序中打开系统设置"));
}

export function onSnapshot(callback: (snapshot: Snapshot) => void): Promise<UnlistenFn> {
  return native
    ? listen<Snapshot>("spike-state", (event) => callback(event.payload))
    : Promise.resolve(() => {});
}

export function onNativeError(callback: (message: string) => void): Promise<UnlistenFn> {
  return native
    ? listen<string>("spike-error", (event) => callback(event.payload))
    : Promise.resolve(() => {});
}
