import { readingParagraphs } from "./readingParagraphs";
import { MathText, tokenizeMathText } from "./MathText";

// Preserve authored paragraphs and protected math/code; gently group plain prose.
export function translationParagraphs(text: string): string[] {
  const protectedRanges: [number, number][] = [];
  let offset = 0;
  for (const segment of tokenizeMathText(text)) {
    const raw = segment.kind === "text" ? segment.value : segment.raw;
    if (segment.kind === "math") protectedRanges.push([offset, offset + raw.length]);
    offset += raw.length;
  }
  for (const match of text.matchAll(/(`+|~{3,})[\s\S]*?\1/g)) {
    protectedRanges.push([match.index, match.index + match[0].length]);
  }
  const blocks: string[] = [];
  let start = 0;
  for (const match of text.matchAll(/\r?\n[\t ]*\r?\n(?:[\t ]*\r?\n)*/g)) {
    if (protectedRanges.some(([a, b]) => match.index >= a && match.index < b)) continue;
    blocks.push(text.slice(start, match.index));
    start = match.index + match[0].length;
  }
  blocks.push(text.slice(start));
  const nonempty = blocks.filter(block => block.trim().length > 0);
  if (nonempty.length === 1 && protectedRanges.length === 0) return readingParagraphs(nonempty[0]);
  return nonempty;
}

export function TranslationText({ text, academic }: { text: string; academic: boolean }) {
  return <>{translationParagraphs(text).map((paragraph, index) => (
    <div className="translation-paragraph" key={index}>
      <ReadingBlock text={paragraph} academic={academic} />
    </div>
  ))}</>;
}

function ReadingBlock({ text, academic }: { text: string; academic: boolean }) {
  const render = (value: string) => academic ? <MathText text={value} /> : value;
  // Only a complete, flat numbered list is eligible. Preserve prose, code,
  // multiline formulas and wrapped/nested source lists in their original form.
  const lines = text.split(/\r?\n/);
  const items = lines.map(line => /^(\d{1,3}[.)、])([\t ]+)(\S.*)$/u.exec(line));
  const numbered = items.length >= 2 && items.every((item, index) =>
    item !== null && Number.parseInt(item[1], 10) === index + 1,
  );
  if (!numbered || /[`~]|\$\$|\\\[|\\begin\{/u.test(text) ||
      lines.some(line => tokenizeMathText(line).some(segment =>
        segment.kind === "text" && /\$|\\[()]/u.test(segment.value),
      ))) return render(text);
  return <div className="translation-numbered-list" role="list">
    {items.map((item, index) => <div className="translation-list-item" role="listitem" key={index}>
      <span className="translation-list-marker">{item![1]}{item![2]}</span>
      <span className="translation-list-content">{render(item![3])}</span>
    </div>)}
  </div>;
}
