import type { ComponentProps } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test } from "vitest";
import { OverlayView } from "./OverlayView";
import { conversationalToneNote, waitingPayload } from "./overlayState";
import type { CapturePayload } from "../shared/types";

const translated: CapturePayload = {
  ...waitingPayload, requestId: 1, phase: "translated", success: true,
  translationMode: "conversational", text: "有道理。", toneNote: "口语中表示认可对方的判断。",
};

function render(payload: CapturePayload) {
  const noop = () => {};
  const props: ComponentProps<typeof OverlayView> = {
    overlayRef: { current: null }, payload, activeMotionPhase: "settled", motionStyle: {},
    isCompact: false, isTranslated: payload.phase === "translated", isFailure: false,
    isLoading: payload.phase === "translating", copied: false, copyFailed: false,
    explanationPhase: "closed", explanationLoadState: { requestId: 1, status: "idle", content: null, errorMessage: null },
    explanationCopied: false, explanationCopyFailed: false, departingStatusText: null,
    onHoveredChange: noop, onSurfaceAnimationEnd: noop, onCopy: noop, onToggleExplanation: noop,
    onCloseExplanation: noop, onCopyExplanation: noop, onRetryExplanation: noop,
    onExplanationAnimationEnd: noop, onRetry: noop, onDismiss: noop,
  };
  return renderToStaticMarkup(<OverlayView {...props} />);
}

describe("conversational tone note", () => {
  test("renders a separate secondary note and escapes model text", () => {
    const html = render({ ...translated, toneNote: "说明 <script>alert(1)</script>" });
    expect(html).toContain('class="tone-note"');
    expect(html).toContain("有道理。");
    expect(html).toContain("&lt;script&gt;");
    expect(html).not.toContain("<script>");
  });
  test("does not render an empty note or carry it into other modes and states", () => {
    for (const change of [{ toneNote: null }, { toneNote: " " }, { translationMode: "academic" as const },
      { success: false }, { phase: "translating" as const }]) {
      const payload = { ...translated, ...change };
      expect(conversationalToneNote(payload)).toBeNull();
      expect(render(payload)).not.toContain('class="tone-note"');
    }
  });
  test("suppresses duplicate, multiline and excessive notes", () => {
    for (const toneNote of [translated.text, "第一句\n第二句", "字".repeat(49)]) {
      expect(conversationalToneNote({ ...translated, toneNote })).toBeNull();
    }
  });
});
