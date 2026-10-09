import type { CapturePayload } from "../shared/types";
import { waitingPayload } from "./overlayState";

export function developmentPreview(): CapturePayload | null {
  if (!import.meta.env.DEV) return null;
  const parameters = new URLSearchParams(window.location.search);
  const phase = parameters.get("preview");
  if (!phase) return null;

  const base: CapturePayload = {
    ...waitingPayload,
    requestId: 42,
    applicationName: "Microsoft Edge",
    captureMethod: "UI Automation",
    translationMode:
      parameters.get("mode") === "academic" ? "academic" : "conversational",
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
  if (phase === "math") {
    return {
      ...base,
      translationMode: "academic",
      phase: "translated",
      success: true,
      text: "质能关系可写为 $E=mc^2$，样本均值定义为：\n$$\\bar{x}=\\frac{1}{n}\\sum_{i=1}^{n}x_i$$\n当 $p \\le 0.05$ 时，仍需结合效应量判断结果。",
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
