import { Fragment, useMemo } from "react";
import katex from "katex";
import "katex/dist/katex.min.css";

export type MathTextSegment =
  | { kind: "text"; value: string }
  | {
      kind: "math";
      value: string;
      raw: string;
      display: boolean;
    };

type MathDelimiter = {
  open: string;
  close: string;
  display: boolean;
  needsHeuristic: boolean;
};

const delimiters: MathDelimiter[] = [
  { open: "$$", close: "$$", display: true, needsHeuristic: false },
  { open: "\\[", close: "\\]", display: true, needsHeuristic: false },
  { open: "\\(", close: "\\)", display: false, needsHeuristic: false },
  { open: "$", close: "$", display: false, needsHeuristic: true },
];

export function tokenizeMathText(text: string): MathTextSegment[] {
  const segments: MathTextSegment[] = [];
  let plainStart = 0;
  let cursor = 0;

  while (cursor < text.length) {
    if (text[cursor] === "`") {
      cursor = skipCodeSpan(text, cursor);
      continue;
    }

    const bareRoot = readBareSquareRoot(text, cursor);
    if (bareRoot) {
      pushTextSegment(segments, text.slice(plainStart, cursor));
      segments.push({
        kind: "math",
        value: bareRoot.value,
        raw: text.slice(cursor, bareRoot.end),
        display: false,
      });
      cursor = bareRoot.end;
      plainStart = bareRoot.end;
      continue;
    }

    const delimiter = delimiters.find(
      (candidate) =>
        text.startsWith(candidate.open, cursor) &&
        !isEscaped(text, cursor) &&
        !isSingleDollarInsideDouble(text, cursor, candidate.open),
    );
    if (!delimiter) {
      cursor += 1;
      continue;
    }

    const expressionStart = cursor + delimiter.open.length;
    const closeIndex = findClosingDelimiter(
      text,
      expressionStart,
      delimiter.close,
    );
    if (closeIndex === -1) {
      cursor += delimiter.open.length;
      continue;
    }

    const expression = text.slice(expressionStart, closeIndex).trim();
    const inlineContainsLineBreak =
      !delimiter.display && expression.includes("\n");
    if (
      !expression ||
      inlineContainsLineBreak ||
      (delimiter.needsHeuristic && !looksLikeDollarMath(expression))
    ) {
      cursor += delimiter.open.length;
      continue;
    }

    pushTextSegment(segments, text.slice(plainStart, cursor));
    const end = closeIndex + delimiter.close.length;
    segments.push({
      kind: "math",
      value: expression,
      raw: text.slice(cursor, end),
      display: delimiter.display,
    });
    cursor = end;
    plainStart = end;
  }

  pushTextSegment(segments, text.slice(plainStart));
  return segments;
}

export function renderMathToHtml(
  expression: string,
  displayMode: boolean,
): string | null {
  const normalizedExpression = normalizeUnicodeSquareRoots(expression);
  if (containsUntrustedCommand(normalizedExpression)) return null;

  try {
    return katex.renderToString(normalizedExpression, {
      displayMode,
      throwOnError: true,
      strict: "warn",
      trust: false,
      output: "htmlAndMathml",
      maxExpand: 500,
      maxSize: 20,
    });
  } catch {
    return null;
  }
}

export function MathText({ text }: { text: string }) {
  const segments = useMemo(() => tokenizeMathText(text), [text]);
  return (
    <>
      {segments.map((segment, index) => {
        if (segment.kind === "text") {
          return <Fragment key={`text-${index}`}>{segment.value}</Fragment>;
        }
        const html = renderMathToHtml(segment.value, segment.display);
        if (html === null) {
          return (
            <span className="math-fallback" key={`fallback-${index}`}>
              {segment.raw}
            </span>
          );
        }
        return (
          <span
            className={segment.display ? "math-display" : "math-inline"}
            key={`math-${index}`}
            data-math-source={segment.raw}
            dangerouslySetInnerHTML={{ __html: html }}
          />
        );
      })}
    </>
  );
}

function findClosingDelimiter(
  text: string,
  start: number,
  close: string,
): number {
  let cursor = start;
  while (cursor < text.length) {
    if (text[cursor] === "`") {
      cursor = skipCodeSpan(text, cursor);
      continue;
    }
    if (
      text.startsWith(close, cursor) &&
      !isEscaped(text, cursor) &&
      !isSingleDollarInsideDouble(text, cursor, close)
    ) {
      return cursor;
    }
    cursor += 1;
  }
  return -1;
}

function isSingleDollarInsideDouble(
  text: string,
  index: number,
  delimiter: string,
): boolean {
  return (
    delimiter === "$" &&
    (text[index - 1] === "$" || text[index + 1] === "$")
  );
}

function isEscaped(text: string, index: number): boolean {
  let backslashes = 0;
  for (let cursor = index - 1; cursor >= 0 && text[cursor] === "\\"; cursor--) {
    backslashes += 1;
  }
  return backslashes % 2 === 1;
}

function skipCodeSpan(text: string, start: number): number {
  let ticks = 1;
  while (text[start + ticks] === "`") ticks += 1;
  const marker = "`".repeat(ticks);
  const close = text.indexOf(marker, start + ticks);
  return close === -1 ? text.length : close + ticks;
}

function looksLikeDollarMath(expression: string): boolean {
  if (expression.includes("`")) return false;
  if (/^[+-]?\d+(?:[.,]\d+)?$/.test(expression)) return false;
  return (
    /\\[A-Za-z]+/.test(expression) ||
    /[_^=+\-*/<>≤≥≈≠∑∏∫√∞]/u.test(expression) ||
    /^[A-Za-z]$/.test(expression) ||
    /^[\p{Script=Greek}]+$/u.test(expression) ||
    /[A-Za-z]\s*\d|\d\s*[A-Za-z]/.test(expression) ||
    /[A-Za-z]\s*\([^)]*\)/.test(expression)
  );
}

type BareRootMatch = {
  end: number;
  value: string;
};

function readBareSquareRoot(text: string, start: number): BareRootMatch | null {
  if (text[start] !== "√") return null;
  let cursor = start + 1;
  while (cursor < text.length && /[ \t]/.test(text[cursor])) cursor += 1;
  if (cursor >= text.length || text[cursor] === "\n") return null;

  if (text[cursor] === "{" || text[cursor] === "(") {
    const opening = text[cursor];
    const closing = opening === "{" ? "}" : ")";
    const groupEnd = findBalancedGroupEnd(text, cursor, opening, closing);
    if (groupEnd === -1) return null;
    const radicand = text.slice(cursor + 1, groupEnd).trim();
    if (!radicand) return null;
    return { end: groupEnd + 1, value: `\\sqrt{${radicand}}` };
  }

  if (!isMathAtomCharacter(text[cursor])) return null;
  const atomStart = cursor;
  cursor += 1;
  while (cursor < text.length && isMathAtomCharacter(text[cursor])) cursor += 1;
  while (cursor < text.length && (text[cursor] === "_" || text[cursor] === "^")) {
    cursor += 1;
    if (text[cursor] === "{") {
      const groupEnd = findBalancedGroupEnd(text, cursor, "{", "}");
      if (groupEnd === -1) return null;
      cursor = groupEnd + 1;
    } else if (cursor < text.length && isMathAtomCharacter(text[cursor])) {
      cursor += 1;
    } else {
      return null;
    }
  }
  return {
    end: cursor,
    value: `\\sqrt{${text.slice(atomStart, cursor)}}`,
  };
}

function normalizeUnicodeSquareRoots(expression: string): string {
  let result = "";
  let cursor = 0;
  while (cursor < expression.length) {
    const match = readBareSquareRoot(expression, cursor);
    if (!match) {
      result += expression[cursor];
      cursor += 1;
      continue;
    }
    result += match.value;
    cursor = match.end;
  }
  return result;
}

function findBalancedGroupEnd(
  text: string,
  start: number,
  opening: string,
  closing: string,
): number {
  let depth = 0;
  for (let cursor = start; cursor < text.length; cursor += 1) {
    if (text[cursor] === opening) depth += 1;
    if (text[cursor] !== closing) continue;
    depth -= 1;
    if (depth === 0) return cursor;
  }
  return -1;
}

function isMathAtomCharacter(character: string): boolean {
  return /[A-Za-z0-9\p{Script=Greek}]/u.test(character);
}

function containsUntrustedCommand(expression: string): boolean {
  return /\\(?:href|url|includegraphics|htmlClass|htmlId|htmlStyle|htmlData)\b/i.test(
    expression,
  );
}

function pushTextSegment(segments: MathTextSegment[], value: string) {
  if (!value) return;
  const previous = segments.at(-1);
  if (previous?.kind === "text") {
    previous.value += value;
  } else {
    segments.push({ kind: "text", value });
  }
}
