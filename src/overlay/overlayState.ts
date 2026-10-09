import type { CapturePayload } from "../shared/types";

export const waitingPayload: CapturePayload = {
  requestId: 0,
  phase: "waiting",
  success: false,
  text: "",
  applicationName: "等待捕获",
  processId: 0,
  captureMethod: "UI Automation",
  elapsedMs: 0,
  selectionRect: null,
  errorCode: null,
  errorMessage: "在其他应用中选中文字，然后按 Ctrl+Shift+T",
  focusPreserved: true,
  clipboardRestored: null,
  warningCode: null,
  languageProfile: null,
  translationMode: null,
};

export function applyPayload(
  current: CapturePayload,
  next: CapturePayload,
): CapturePayload {
  return shouldAcceptCapturePayload(current, next) ? next : current;
}

export function shouldAcceptCapturePayload(current: CapturePayload, next: CapturePayload): boolean {
  if (next.requestId !== current.requestId) return next.requestId > current.requestId;
  // A locally synthesized IPC error may recover when a real snapshot arrives.
  if (current.errorCode === "OVERLAY_IPC_FAILED") return true;
  const stage = (phase: CapturePayload["phase"]): number => {
    if (phase === "waiting") return 0;
    if (phase === "capturing") return 1;
    if (phase === "translating") return 2;
    return 3;
  };
  return stage(next.phase) >= stage(current.phase);
}

export function shouldBridgeLoadingToResult(
  current: CapturePayload,
  next: CapturePayload,
): boolean {
  const currentIsLoading =
    current.phase === "capturing" || current.phase === "translating";
  const nextIsTerminal =
    next.phase === "translated" ||
    next.phase === "captureFailed" ||
    next.phase === "translationFailed";
  return (
    current.requestId > 0 &&
    current.requestId === next.requestId &&
    currentIsLoading &&
    nextIsTerminal
  );
}

export function shouldShowExplanationAction(payload: CapturePayload): boolean {
  return (
    payload.phase === "translated" &&
    payload.success &&
    payload.translationMode === "academic"
  );
}

export function conversationalToneNote(payload: CapturePayload): string | null {
  if (payload.phase !== "translated" || !payload.success || payload.translationMode !== "conversational") return null;
  const note = payload.toneNote?.trim();
  return note && Array.from(note).length <= 48 && !/[\r\n]/.test(note) && note !== payload.text.trim() ? note : null;
}

export function statusText(payload: CapturePayload): string {
  if (isSelectionHint(payload)) return "提示";
  if (payload.phase === "capturing") return "读取中";
  if (payload.phase === "translating") return "翻译中";
  if (payload.phase === "translated") {
    return payload.translationMode === "academic" ? "Professional" : "Natural";
  }
  if (payload.phase === "translationFailed") return "翻译未完成";
  if (payload.errorCode === "OVERLAY_IPC_FAILED") return "未连接";
  return payload.requestId === 0 ? "等待" : "未读取";
}

export function contentText(payload: CapturePayload): string {
  if (isSelectionHint(payload)) return "请先选择文字";
  if (payload.phase === "capturing") return "正在读取所选文字…";
  if (payload.phase === "translating") return "正在生成译文…";
  if (payload.phase === "translated") return payload.text;
  return payload.errorMessage ?? "请重新选择文字后再试";
}

export function isSelectionHint(payload: CapturePayload): boolean {
  return payload.phase === "captureFailed" && payload.errorCode === "NO_TEXT_SELECTED";
}
