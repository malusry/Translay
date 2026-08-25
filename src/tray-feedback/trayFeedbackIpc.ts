import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface TrayFeedbackPayload {
  generation: number;
  message: string;
}

export function onTrayFeedbackShow(
  handler: (payload: TrayFeedbackPayload) => void,
): Promise<UnlistenFn> {
  return listen<TrayFeedbackPayload>(
    "tray-style-feedback-show",
    ({ payload }) => handler(payload),
  );
}

export function onTrayFeedbackDismiss(
  handler: (generation: number) => void,
): Promise<UnlistenFn> {
  return listen<number>(
    "tray-style-feedback-dismiss",
    ({ payload }) => handler(Number(payload)),
  );
}
