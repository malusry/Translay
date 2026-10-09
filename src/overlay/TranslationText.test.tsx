import { expect, test } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { TranslationText, translationParagraphs } from "./TranslationText";

test("paragraphs follow blank lines, not sentences or dictionary lines", () => {
  expect(translationParagraphs("第一句。第二句。\r\n\r\n另一段。"))
    .toEqual(["第一句。第二句。", "另一段。"]);
  expect(translationParagraphs("1. 银行\n2. 河岸")).toEqual(["1. 银行\n2. 河岸"]);
});

test("blank lines inside display formulas and code remain intact", () => {
  const formula = "$$\\begin{aligned}a&=b\\\\\n\nc&=d\\end{aligned}$$";
  const code = "```text\nfirst\n\nsecond\n```";
  expect(translationParagraphs(`正文\n\n${formula}\n\n${code}`))
    .toEqual(["正文", formula, code]);
});

test("academic paragraphs retain math and daily content remains escaped text", () => {
  const academic = renderToStaticMarkup(<TranslationText text={'正文 $x^2$\n\n另一段。'} academic />);
  expect(academic.match(/class="translation-paragraph"/g)).toHaveLength(2);
  expect(academic).toContain('class="katex"');
  const daily = renderToStaticMarkup(<TranslationText text={'<script>bad()</script>\n\n下一段'} academic={false} />);
  expect(daily).not.toContain('<script>');
  expect(daily).toContain('&lt;script&gt;');
});

test("flat consecutive lists indent wrapped text and retain academic inline math", () => {
  const html = renderToStaticMarkup(<TranslationText text={'1. Keep the assumptions with the conclusion.\n2. Preserve $x_i$ in the result.'} academic />);
  expect(html.match(/role="listitem"/g)).toHaveLength(2);
  expect(html).toContain('class="translation-list-marker"');
  expect(html).toContain('class="katex"');
});

test("prose, nested lists, code and display math retain their original layout", () => {
  for (const text of [
    '1. One sentence only.', '1. First\nContinuation\n2. Second',
    '1. Outer\n  2. Nested', '1. First\n3. Discontinuous',
    '1. `code`\n2. Keep code', '1. $$x_i$$\n2. Another formula',
    '1. $x +\n2. y$',
  ]) {
    expect(renderToStaticMarkup(<TranslationText text={text} academic />)).not.toContain('role="listitem"');
  }
});
