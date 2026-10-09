import { expect, test } from "vitest";
import { readingParagraphs } from "./readingParagraphs";

test("user's medium passage separates the situation from the response", () => {
  const first = "老练且持续存在的威胁行为者不断试探我们的防护措施，并试图绕过我们用来检测和防止滥用的技术手段。";
  const second = "我们将继续改进防护措施，并与合作伙伴协调，以提高我们检测、阻止和防止未来滥用的能力。";
  expect(readingParagraphs(first + second)).toEqual([first, second]);
});

test("ordinary short replies and connected explanations stay together", () => {
  expect(readingParagraphs("好的，会议三点开始。请提前到场。")).toEqual(["好的，会议三点开始。请提前到场。"]);
  const text = "这一方法可以帮助读者找到文章中的主要信息，并且能够减少在不重要细节上花费的时间。" +
    "因此，我们建议在遇到陌生领域的材料时先阅读完整段落，再结合文章上下文逐步理解术语与表达的含义。";
  expect(readingParagraphs(text)).toEqual([text]);
});

test("multiple short sentences form groups, never one sentence per line", () => {
  const sentences = Array.from({length: 6}, (_, i) => `第${i + 1}项工作已经完成核对，相关资料也已经归档，后续安排保持原定计划。`);
  const blocks = readingParagraphs(sentences.join(""));
  expect(blocks).toHaveLength(2);
  expect(blocks.every(b => (b.match(/。/g) || []).length >= 2)).toBe(true);
  expect(blocks.join("")).toBe(sentences.join(""));
});

test("long prose preserves every character while adding bounded groups", () => {
  const text = Array.from({length: 12}, (_, i) => `事项${i}已经由相关负责人核对完毕，文件记录能够清楚说明这次工作的具体安排。`).join("");
  const blocks = readingParagraphs(text);
  expect(blocks.length).toBeGreaterThan(2);
  expect(blocks.length).toBeLessThan(8);
  expect(blocks.join("")).toBe(text);
});

test("a single long sentence is never cut at commas", () => {
  const text = "相关负责人需要核实全部材料、检查具体安排、确认每一项工作细节，".repeat(5) + "然后再提交。";
  expect(readingParagraphs(text)).toEqual([text]);
});

test("quotes, lists, formulas, code and explicit lines remain protected", () => {
  const quote = "“" + "这是引用材料中需要整体保留的句子。".repeat(8) + "”";
  for (const text of [quote, '1. ' + "词典说明。".repeat(30), '正文 $x^2$。' + "解释。".repeat(40), '```' + "代码。".repeat(40) + '```', "已有换行\n" + "正文。".repeat(40)]) {
    expect(readingParagraphs(text)).toEqual([text]);
  }
});
