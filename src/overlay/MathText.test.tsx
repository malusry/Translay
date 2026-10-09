import { describe, expect, test } from "vitest";

import { renderMathToHtml, tokenizeMathText } from "./MathText";

describe("academic math text", () => {
  test("recognizes common inline and display delimiters", () => {
    const segments = tokenizeMathText(
      "当 $E=mc^2$ 时，\\(x_i\\) 保持不变。$$\\sum_{i=1}^{n} x_i$$",
    );
    const math = segments.filter((segment) => segment.kind === "math");

    expect(math).toHaveLength(3);
    expect(math[0]).toMatchObject({ value: "E=mc^2", display: false });
    expect(math[1]).toMatchObject({ value: "x_i", display: false });
    expect(math[2]).toMatchObject({ display: true });
  });

  test("does not mistake prices, escaped dollars, or code for formulas", () => {
    const segments = tokenizeMathText(
      "价格从 $5 到 $10，写作 \\$5；代码 `const fee = '$x$'`。",
    );

    expect(segments.every((segment) => segment.kind === "text")).toBe(true);
  });

  test("does not use a delimiter inside code to close surrounding prose", () => {
    const segments = tokenizeMathText(
      "预算是 $5，示例代码为 `const value = '$x$'`，最终是 $10。",
    );

    expect(segments.every((segment) => segment.kind === "text")).toBe(true);
  });

  test("renders valid formulas and rejects malformed input", () => {
    expect(renderMathToHtml("\\frac{a}{b}", false)).toContain("katex");
    expect(renderMathToHtml("\\frac{", false)).toBeNull();
  });

  test("renders a bare square root with a radicand but leaves a lone symbol alone", () => {
    const rooted = tokenizeMathText("分母中的 √d_k 用于缩放");
    const lone = tokenizeMathText("符号 √ 表示平方根");

    expect(rooted.filter((segment) => segment.kind === "math")).toEqual([
      {
        kind: "math",
        value: "\\sqrt{d_k}",
        raw: "√d_k",
        display: false,
      },
    ]);
    expect(lone.every((segment) => segment.kind === "text")).toBe(true);
    expect(renderMathToHtml("√{d_k}", false)).toContain("sqrt");
  });

  test("does not trust link-producing LaTeX commands", () => {
    const html = renderMathToHtml(
      "\\href{javascript:alert(1)}{unsafe}",
      false,
    );

    expect(html).toBeNull();
  });
});

// Formula reference: Attention Is All You Need, section 3.2.1, equation (1).
// https://arxiv.org/html/1706.03762v7#S3.SS2.SSS1
const attention = String.raw`\operatorname{Attention}(Q,K,V)=\operatorname{softmax}\left(\frac{QK^{T}}{\sqrt{d_k}}\right)V`;

test.each([
  [`$$${attention}$$ (1)`, 1],
  [String.raw`缩放因子为 $\frac{1}{\sqrt{d_k}}$。`, 1],
  [`$$${attention}$$ (1)\n\n${String.raw`点积注意力使用缩放因子 $\frac{1}{\sqrt{d_k}}$。`}\n正文和引用 [2] 仍需保留。`, 2],
])("renders paper math with prose and equation labels: %s", (text, count) => {
  const segments = tokenizeMathText(text);
  const formulas = segments.filter((segment) => segment.kind === "math");
  expect(formulas).toHaveLength(count);
  expect(segments.map((s) => s.kind === "math" ? s.raw : s.value).join("")).toBe(text);
  for (const formula of formulas) {
    const html = renderMathToHtml(formula.value, formula.display);
    expect(html).toContain("<mfrac>");
    expect(html).toContain("<msqrt>");
    expect(html).toContain("<msub>");
    if (formula.display) {
      expect(html).toContain("<msup>");
      expect(html).toContain("<mi>V</mi>");
    }
  }
});

test("ignores formulas inside double-backtick code and resumes afterwards", () => {
  const text = "示例 ``literal `$x_i$` and √d_k``，实际使用 $x_j$。";
  expect(tokenizeMathText(text).filter((s) => s.kind === "math")).toMatchObject([
    { value: "x_j", display: false },
  ]);
});
