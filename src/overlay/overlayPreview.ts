import type { CapturePayload } from "../shared/types";
import { waitingPayload } from "./overlayState";

export function developmentPreview(): CapturePayload | null {
  if (!import.meta.env.DEV) return null;
  const phase = new URLSearchParams(window.location.search).get("preview");
  if (!phase) return null;

  const base: CapturePayload = {
    ...waitingPayload,
    requestId: 42,
    applicationName: "Microsoft Edge",
    captureMethod: "UI Automation",
    translationMode: "conversational",
  };
  if (phase === "capturing") {
    return { ...base, phase: "capturing" };
  }
  if (phase === "translating") {
    return { ...base, phase: "translating" };
  }
  if (phase === "motion" || phase === "motion-compact") {
    return { ...base, phase: "translating" };
  }
  if (phase === "error") {
    return {
      ...base,
      phase: "translationFailed",
      errorCode: "TRANSLATION_FAILED",
      errorMessage: "暂时无法连接翻译服务，请稍后重试",
    };
  }
  if (phase === "translated") {
    return {
      ...base,
      phase: "translated",
      success: true,
      text: "翻译应该像系统原本就具备的能力一样自然存在，不打断阅读，也不要求用户切换窗口。",
      errorMessage: null,
    };
  }
  if (phase === "dismiss") {
    return {
      ...base,
      phase: "translated",
      success: true,
      text: "译文阅读完成后，浮层会安静地融回当前环境。",
      errorMessage: null,
    };
  }
  if (phase === "compact") {
    return {
      ...base,
      phase: "translated",
      success: true,
      text: "这是一段简短译文。",
      errorMessage: null,
    };
  }
  return null;
}
