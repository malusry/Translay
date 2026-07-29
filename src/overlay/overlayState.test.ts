import { describe, expect, test } from "vitest";
import type { CapturePayload } from "../shared/types";
import {
  applyPayload,
  contentText,
  shouldBridgeLoadingToResult,
  statusText,
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

  test("failure payload shows a short error state", () => {
    const failure = payload(2, {
      phase: "captureFailed",
      success: false,
      text: "",
      errorCode: "NO_TEXT_SELECTED",
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
    expect(contentText(translating)).toBe("正在生成中文译文…");
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
