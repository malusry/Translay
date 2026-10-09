import { describe, expect, test } from "vitest";
import type { CapturePayload } from "../shared/types";
import {
  applyPayload,
  contentText,
  shouldBridgeLoadingToResult,
  shouldShowExplanationAction,
  statusText,
  isSelectionHint,
  waitingPayload,
} from "./overlayState";

function payload(
  requestId: number,
  options: Partial<CapturePayload> = {},
): CapturePayload {
  return {
    ...waitingPayload,
    requestId,
    phase: "translated",
    success: true,
    text: `selection-${requestId}`,
    elapsedMs: 76,
    errorMessage: null,
    ...options,
  };
}

describe("overlay payload state", () => {
  test("empty selection gets a neutral prompt without hiding genuine capture errors", () => {
    const hint = payload(10, { phase: "captureFailed", success: false, errorCode: "NO_TEXT_SELECTED" });
    expect(isSelectionHint(hint)).toBe(true);
    expect(statusText(hint)).toBe("提示");
    expect(contentText(hint)).toBe("请先选择文字");
    const failure = { ...hint, errorCode: "CLIPBOARD_TIMEOUT", errorMessage: "复制选区超时" };
    expect(isSelectionHint(failure)).toBe(false);
    expect(contentText(failure)).toBe("复制选区超时");
  });
  test("successful payload shows the active translation mode", () => {
    const success = payload(1);
    expect(statusText(success)).toBe("Natural");
    expect(statusText(success)).not.toContain("暂时无法读取");
    expect(contentText(success)).toBe("selection-1");
  });

  test("professional mode is visible without exposing timing details", () => {
    const success = payload(5, { translationMode: "academic" });
    expect(statusText(success)).toBe("Professional");
    expect(statusText(success)).not.toContain("ms");
  });

  test("explanation action is reserved for successful academic translations", () => {
    expect(
      shouldShowExplanationAction(
        payload(8, { translationMode: "academic" }),
      ),
    ).toBe(true);
    expect(
      shouldShowExplanationAction(
        payload(9, { translationMode: "conversational" }),
      ),
    ).toBe(false);
    expect(
      shouldShowExplanationAction(
        payload(10, {
          phase: "translating",
          success: false,
          translationMode: "academic",
        }),
      ),
    ).toBe(false);
  });

  test("failure payload shows a short error state", () => {
    const failure = payload(2, {
      phase: "captureFailed",
      success: false,
      text: "",
      errorCode: "UIA_TIMEOUT",
      errorMessage: "没有读取到所选文字",
    });
    expect(statusText(failure)).toBe("未读取");
    expect(contentText(failure)).toBe("没有读取到所选文字");
  });

  test("translation loading is distinct from capture failure", () => {
    const translating = payload(3, {
      phase: "translating",
      success: false,
      text: "",
    });
    expect(statusText(translating)).toBe("翻译中");
    expect(contentText(translating)).toBe("正在生成译文…");
  });

  test("selection capture has its own lightweight progress state", () => {
    const capturing = payload(6, {
      phase: "capturing",
      success: false,
      text: "",
    });
    expect(statusText(capturing)).toBe("读取中");
    expect(contentText(capturing)).toBe("正在读取所选文字…");
  });

  test("rapid updates keep the newest request", () => {
    const second = applyPayload(payload(1), payload(2));
    const lateFirst = applyPayload(second, payload(1, { text: "late old" }));
    expect(lateFirst.requestId).toBe(2);
    expect(lateFirst.text).toBe("selection-2");
  });

  test("late snapshots cannot roll the same request back to an earlier stage", () => {
    const done = payload(20, { toneNote: "保留最终附注。" });
    for (const phase of ["capturing", "translating"] as const) {
      expect(applyPayload(done, payload(20, { phase, text: "", toneNote: null }))).toBe(done);
    }
    const translating = payload(21, { phase: "translating" });
    expect(applyPayload(translating, payload(21, { phase: "capturing" }))).toBe(translating);
  });

  test("new generations and local IPC error recovery can still start loading", () => {
    const loading = payload(23, { phase: "translating" });
    expect(applyPayload(payload(22), loading)).toBe(loading);
    const ipcFailure = payload(23, { phase: "captureFailed", errorCode: "OVERLAY_IPC_FAILED" });
    expect(applyPayload(ipcFailure, loading)).toBe(loading);
    const realFailure = payload(23, { phase: "translationFailed", errorCode: "TRANSLATION_FAILED" });
    expect(applyPayload(realFailure, loading)).toBe(realFailure);
  });

  test("equal request id may apply the post-show focus update", () => {
    const initial = payload(4);
    const update = payload(4, { focusPreserved: false });
    expect(applyPayload(initial, update).focusPreserved).toBe(false);
  });

  test("only the same request bridges loading into a terminal result", () => {
    const loading = payload(7, {
      phase: "translating",
      success: false,
      text: "",
    });
    expect(shouldBridgeLoadingToResult(loading, payload(7))).toBe(true);
    expect(shouldBridgeLoadingToResult(loading, payload(8))).toBe(false);
    expect(shouldBridgeLoadingToResult(payload(7), payload(7))).toBe(false);
  });
});
