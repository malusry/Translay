import { readFileSync } from "node:fs";
import { describe, expect, test } from "vitest";

const styles = readFileSync(new URL("./overlay.css", import.meta.url), "utf8");

function declarationBlock(selector) {
  const escapedSelector = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = styles.match(new RegExp(`${escapedSelector}\\s*\\{([^}]+)\\}`));
  if (!match) throw new Error(`Missing CSS rule for ${selector}`);
  return match[1];
}

describe("overlay motion isolation", () => {
  test("the interactive overlay root remains free of visual animation", () => {
    const overlay = declarationBlock(".overlay");
    expect(overlay).not.toContain("animation:");
    expect(overlay).not.toContain("transform:");
    expect(overlay).not.toContain("transition:");
  });

  test("the animated surface never participates in layout or pointer input", () => {
    const surface = declarationBlock(".overlay-surface");
    expect(surface).toContain("position: absolute");
    expect(surface).toContain("pointer-events: none");
  });

  test("materialize motion resizes only the noninteractive visual surface", () => {
    const overlay = declarationBlock(".overlay");
    expect(overlay).not.toContain("clip-path:");
    expect(styles).toContain("--motion-materialize: 420ms");
    expect(styles).toContain("--motion-delay-content: 135ms");
    expect(styles).toContain("--motion-delay-controls: 245ms");
    expect(styles).toContain("top: var(--motion-origin-top)");
    expect(styles).toContain("left: var(--motion-origin-left)");
    expect(styles).toContain("width: var(--motion-origin-width)");
    expect(styles).toContain("height: var(--motion-origin-height)");
    expect(styles).toContain("width: calc(100% + 2px)");
    expect(styles).toContain("height: calc(100% + 2px)");
    expect(styles).not.toContain("clip-path:");
    expect(styles).toContain(".departing-loading-shell");
    expect(styles).toContain(".motion-preparing .overlay-surface");
    expect(styles).toContain(".motion-revealing .translation");
  });

  test("the final surface uses the warm neutral Translay palette", () => {
    const surface = declarationBlock(".overlay-surface");
    expect(surface).toContain("border: 1px solid #dddbd4");
    expect(surface).toContain("background: rgba(247, 246, 242, 0.988)");
    expect(surface).not.toContain("rgba(250, 250, 248");
  });

  test("loading uses a restrained dot and dismissal returns to its anchor", () => {
    expect(styles).toContain(".progress-dot");
    expect(styles).toContain("@keyframes progress-pulse");
    expect(styles).toContain("900ms ease-in-out infinite alternate");
    expect(styles).toContain(".motion-dismissing .overlay-surface");
    expect(styles).toContain("@keyframes surface-anchor-return");
    expect(styles).toContain("@keyframes surface-air-dissolve");
    expect(styles).toContain(
      "content-return 270ms cubic-bezier(0.2, 0, 0.3, 1)",
    );
    expect(styles).toContain(
      "controls-return 210ms cubic-bezier(0.2, 0, 0.3, 1)",
    );
    expect(styles).toContain("--motion-dismiss: 630ms");
    expect(styles).toContain("--motion-dismiss-visible: 554ms");
    expect(styles).toContain("top: var(--motion-origin-top)");
    expect(styles).toContain("left: var(--motion-origin-left)");
    expect(styles).toContain("width: var(--motion-origin-width)");
    expect(styles).toContain("height: var(--motion-origin-height)");
    expect(styles).toContain("opacity: 0.84");
    expect(styles).toContain("opacity: 0.62");
    expect(styles).toContain("opacity: 0.36");
    expect(styles).toContain("opacity: 0.16");
    expect(styles).toContain("opacity: 0.06");
    expect(styles).toContain("88% {\n    opacity: 0");
    expect(styles).toContain("transform: scale(0.93)");
  });
});
