import { describe, expect, test } from "vitest";
import {
  calculateMaterializeTiming,
  calculateRevealOrigin,
  DISMISS_DURATION_MS,
  DISMISS_SAFETY_TIMEOUT_MS,
  LOADING_OVERLAY_HEIGHT,
  LOADING_OVERLAY_WIDTH,
  MATERIALIZE_MAX_DURATION_MS,
  MATERIALIZE_MIN_DURATION_MS,
  MATERIALIZE_SAFETY_PADDING_MS,
  readWindowGeometry,
  shouldAnimateLoadingResult,
} from "./overlayMotion";

describe("overlay materialize geometry", () => {
  test("fast results do not replay an unseen loading shell", () => {
    expect(shouldAnimateLoadingResult(0)).toBe(false);
    expect(shouldAnimateLoadingResult(109)).toBe(false);
    expect(shouldAnimateLoadingResult(110)).toBe(true);
    expect(shouldAnimateLoadingResult(8000)).toBe(true);
  });
  test("preserves a loading surface anchored at the target top-left", () => {
    const origin = calculateRevealOrigin(
      { x: 320, y: 180, width: 148, height: 58 },
      { x: 320, y: 180, width: 520, height: 220 },
    );

    expect(origin).toEqual({ left: 0, top: 0, width: 148, height: 58 });
  });

  test("preserves a loading surface aligned to the target bottom-right", () => {
    const origin = calculateRevealOrigin(
      { x: 472, y: 342, width: 148, height: 58 },
      { x: 120, y: 100, width: 500, height: 300 },
    );

    expect(origin).toEqual({
      left: 352,
      top: 242,
      width: 148,
      height: 58,
    });
  });

  test("clamps unexpected native geometry inside the transparent stage", () => {
    const origin = calculateRevealOrigin(
      { x: 20, y: 20, width: 800, height: 400 },
      { x: 100, y: 100, width: 320, height: 160 },
    );

    expect(origin).toEqual({ left: 0, top: 0, width: 320, height: 160 });
  });

  test("uses the loading dimensions when no prior geometry was captured", () => {
    const origin = calculateRevealOrigin(null, {
      x: 0,
      y: 0,
      width: 480,
      height: 220,
    });

    expect(origin.width).toBe(LOADING_OVERLAY_WIDTH);
    expect(origin.height).toBe(LOADING_OVERLAY_HEIGHT);
  });

  test("adapts materialize duration to the actual expansion distance", () => {
    const short = calculateMaterializeTiming(
      { left: 0, top: 0, width: 148, height: 58 },
      { width: 220, height: 64 },
    );
    const medium = calculateMaterializeTiming(
      { left: 0, top: 0, width: 148, height: 58 },
      { width: 400, height: 140 },
    );
    const long = calculateMaterializeTiming(
      { left: 0, top: 0, width: 148, height: 58 },
      { width: 560, height: 300 },
    );

    expect(short.durationMs).toBe(MATERIALIZE_MIN_DURATION_MS);
    expect(medium.durationMs).toBeGreaterThan(short.durationMs);
    expect(long.durationMs).toBe(MATERIALIZE_MAX_DURATION_MS);
    expect(long.contentDelayMs).toBeLessThan(long.controlsDelayMs);
  });

  test("safety windows exceed their visible motion", () => {
    expect(MATERIALIZE_SAFETY_PADDING_MS).toBeGreaterThan(0);
    expect(DISMISS_SAFETY_TIMEOUT_MS).toBeGreaterThan(DISMISS_DURATION_MS);
  });

  test("normalizes invalid browser geometry", () => {
    expect(
      readWindowGeometry({
        screenX: Number.NaN,
        screenY: 12,
        innerWidth: 0,
        innerHeight: Number.NaN,
      } as Window),
    ).toEqual({ x: 0, y: 12, width: 1, height: 1 });
  });
});
