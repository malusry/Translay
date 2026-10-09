import { describe, expect, test } from "vitest";

import type { ExplanationContent } from "../shared/types";
import {
  applyExplanationFailure,
  idleExplanationLoadState,
  applyExplanationResult,
  beginExplanationLoad,
  formatExplanationForCopy,
  explanationErrorMessage,
  shouldStartExplanationLoad,
} from "./explanationState";

const explanation: ExplanationContent = {
  coreExplanation: "核心解释。",
  keyConcepts: [
    {
      term: "bias",
      translation: "偏差",
      explanation: "系统性的偏离。",
    },
  ],
  caveat: "",
};

describe("explanation state", () => {
  test("a late result cannot overwrite a newer translation", () => {
    const loading = beginExplanationLoad(7);

    expect(applyExplanationResult(loading, 8, 7, explanation)).toBe(loading);
    expect(applyExplanationFailure(loading, 8, 7, "late error")).toBe(loading);
  });

  test("a ready explanation is reused instead of requested again", () => {
    const ready = applyExplanationResult(
      beginExplanationLoad(9),
      9,
      9,
      explanation,
    );

    expect(ready.status).toBe("ready");
    expect(shouldStartExplanationLoad(ready, 9)).toBe(false);
    expect(shouldStartExplanationLoad(ready, 10)).toBe(true);
  });

  test("a failed explanation waits for an explicit retry", () => {
    const failed = applyExplanationFailure(
      beginExplanationLoad(11),
      11,
      11,
      "temporary failure",
    );

    expect(failed.status).toBe("error");
    expect(shouldStartExplanationLoad(failed, 11)).toBe(false);
  });

  test("copy text contains only sections that have useful content", () => {
    const text = formatExplanationForCopy(explanation);

    expect(text.startsWith("核心解释。\n\n关键概念\n")).toBe(true);
    expect(text).toContain("bias（偏差）：系统性的偏离。");
    expect(text).not.toContain("通俗理解");
    expect(text).not.toContain("阅读提醒");
  });

  test("minimal copy has no empty headings and preserves formula source and caveat", () => {
    expect(formatExplanationForCopy({
      coreExplanation: "简单解释。",
      keyConcepts: [],
      caveat: "",
    })).toBe("简单解释。");
    expect(formatExplanationForCopy({
      coreExplanation: "原式为 $\\sqrt{d_k}$。",
      keyConcepts: [],
      caveat: "变量含义未给出。",
    })).toBe("原式为 $\\sqrt{d_k}$。\n\n变量含义未给出。");
  });

  test("long provider errors stay readable inside the compact panel", () => {
    const message = explanationErrorMessage("x".repeat(200));

    expect(Array.from(message)).toHaveLength(161);
    expect(message.endsWith("…")).toBe(true);
  });
});

test("closing a pending explanation rejects late success and failure for the same capture", () => {
  const closed = idleExplanationLoadState();
  expect(applyExplanationResult(closed, 7, 7, explanation)).toBe(closed);
  expect(applyExplanationFailure(closed, 7, 7, "late failure")).toBe(closed);
  expect(shouldStartExplanationLoad(closed, 7)).toBe(true);
});
