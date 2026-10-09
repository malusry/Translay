import type { ExplanationContent } from "../shared/types";

export type ExplanationLoadStatus = "idle" | "loading" | "ready" | "error";

export interface ExplanationLoadState {
  requestId: number | null;
  status: ExplanationLoadStatus;
  content: ExplanationContent | null;
  errorMessage: string | null;
}

export function idleExplanationLoadState(): ExplanationLoadState {
  return {
    requestId: null,
    status: "idle",
    content: null,
    errorMessage: null,
  };
}

export function beginExplanationLoad(requestId: number): ExplanationLoadState {
  return {
    requestId,
    status: "loading",
    content: null,
    errorMessage: null,
  };
}

export function shouldStartExplanationLoad(
  state: ExplanationLoadState,
  requestId: number,
): boolean {
  return state.requestId !== requestId || state.status === "idle";
}

export function applyExplanationResult(
  state: ExplanationLoadState,
  currentRequestId: number,
  responseRequestId: number,
  content: ExplanationContent,
): ExplanationLoadState {
  if (
    state.requestId !== responseRequestId ||
    state.status !== "loading" ||
    currentRequestId !== responseRequestId
  ) {
    return state;
  }
  return {
    requestId: responseRequestId,
    status: "ready",
    content,
    errorMessage: null,
  };
}

export function applyExplanationFailure(
  state: ExplanationLoadState,
  currentRequestId: number,
  responseRequestId: number,
  errorMessage: string,
): ExplanationLoadState {
  if (
    state.requestId !== responseRequestId ||
    state.status !== "loading" ||
    currentRequestId !== responseRequestId
  ) {
    return state;
  }
  return {
    requestId: responseRequestId,
    status: "error",
    content: null,
    errorMessage,
  };
}

export function explanationErrorMessage(error: unknown): string {
  const message =
    typeof error === "string"
      ? error.trim()
      : error instanceof Error
        ? error.message.trim()
        : "";
  if (!message) return "暂时无法生成解释，请稍后重试";
  const characters = Array.from(message);
  return characters.length <= 160
    ? message
    : `${characters.slice(0, 160).join("")}…`;
}

export function formatExplanationForCopy(content: ExplanationContent): string {
  const sections = [content.coreExplanation];
  if (content.caveat) sections.push(content.caveat);
  if (content.keyConcepts.length > 0) {
    sections.push(
      "关键概念\n" + content.keyConcepts
        .map((term) => {
          const label = term.translation
            ? term.term + "（" + term.translation + "）"
            : term.term;
          return "- " + label + "：" + term.explanation;
        })
        .join("\n"),
    );
  }
  return sections.join("\n\n");
}

export const developmentExplanationPreview: ExplanationContent = {
  coreExplanation:
    "公式 $y=ax+b$ 表示 y 随 x 按线性关系变化：x 每增加一个单位，y 就变化 a 个单位；当 x 为零时，y 等于 b。",
  keyConcepts: [],
  caveat: "",
};
