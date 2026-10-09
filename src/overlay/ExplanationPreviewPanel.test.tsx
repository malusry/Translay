import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test } from "vitest";
import type { ExplanationContent } from "../shared/types";
import { ExplanationPreviewPanel } from "./ExplanationPreviewPanel";

function render(content: ExplanationContent) {
  return renderToStaticMarkup(
    <ExplanationPreviewPanel
      phase="open"
      loadState={{ requestId: 1, status: "ready", content, errorMessage: null }}
      copied={false}
      copyFailed={false}
      onCopy={() => {}}
      onRetry={() => {}}
      onClose={() => {}}
      onAnimationEnd={() => {}}
    />,
  );
}

describe("compact explanation panel", () => {
  test("simple explanations have no section headings or empty caveat", () => {
    const html = render({ coreExplanation: "只报告关联，尚未证明因果。", keyConcepts: [], caveat: "" });
    expect(html).toContain("只报告关联，尚未证明因果。");
    expect(html).not.toContain("<h3>");
    expect(html).not.toContain("explanation-caveat");
  });

  test("optional concepts and specific ambiguity remain readable with safe math", () => {
    const html = render({
      coreExplanation: "式中包含 $\\sqrt{a}$。",
      keyConcepts: [{ term: "a", translation: "", explanation: "被开方项。" }],
      caveat: "无法确定 <script>alert(1)</script> 是否来自原文。",
    });
    expect(html).toContain("<h3>关键概念</h3>");
    expect(html).toContain('class="explanation-caveat"');
    expect(html).toContain("katex");
    expect(html).toContain("&lt;script&gt;");
    expect(html).not.toContain("<script>");
    expect(html).not.toContain("上下文与逻辑");
    expect(html).not.toContain("阅读提醒");
  });
});
