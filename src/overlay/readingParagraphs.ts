// A display-only fallback for Chinese prose when a provider returns one block.
// It inserts boundaries only after complete sentences and never changes text.
export function readingParagraphs(text: string): string[] {
  if (text.includes("\n") || /[`~$\\]|https?:|^\s*(?:\d+[.、)]|[-*#>])/u.test(text)) return [text];
  const length = (value: string) => [...value.replace(/\s/g, "")].length;
  if (length(text) < 65 || !/[\u3400-\u9fff]/u.test(text)) return [text];

  const boundaries: number[] = [];
  const stack: string[] = [];
  const pairs: Record<string, string> = { "“": "”", "‘": "’", "（": "）", "(": ")", "[": "]", "【": "】", "「": "」", "『": "』", '"': '"' };
  for (let i = 0; i < text.length; i++) {
    const char = text[i];
    if (stack.at(-1) === char) stack.pop();
    else if (pairs[char]) stack.push(pairs[char]);
    if (/[。！？]/u.test(char) && stack.length === 0 && !/[。！？]/u.test(text[i + 1] || "")) boundaries.push(i + 1);
  }
  const dependent = /^(?:因此|所以|因而|由此|也就是说|换言之|这意味着|这说明|这表明|例如|比如|其中|尤其|具体来说|具体而言|需要注意|值得注意|需要强调|这一结论|该结论|这并不|仅当|只有|前提是|除非|否则|只要|即使|尽管|不过|但(?:是)?|然而|同时)/u;
  const transition = /^(?:接下来|另一方面|相比之下|至于|此外|最后|随后|我们(?:将|会|计划|也将)|为此)/u;
  const split = (block: string, offsets: number[]): string[] => {
    const total = length(block);
    if (total < 65) return [block];
    const candidates = offsets.filter(at => {
      const left = block.slice(0, at), right = block.slice(at).trimStart();
      if (dependent.test(right)) return false;
      const min = total < 100 ? 24 : 32;
      if (length(left) < min || length(right) < min) return false;
      // Shorter passages only split at an explicit change of topic/action.
      return total >= 100 || transition.test(right);
    });
    if (!candidates.length) return [block];
    const target = total > 240 ? 120 : total / 2;
    const score = (at: number) => Math.abs(length(block.slice(0, at)) - target)
      - (transition.test(block.slice(at).trimStart()) ? 22 : 0);
    const at = candidates.reduce((best, next) => score(next) < score(best) ? next : best);
    // Medium passages get at most two blocks. Longer prose is grouped rather
    // than giving each short sentence its own paragraph.
    const right = block.slice(at);
    return total > 240
      ? [block.slice(0, at), ...split(right, offsets.filter(n => n > at).map(n => n - at))]
      : [block.slice(0, at), right];
  };
  return split(text, boundaries);
}
