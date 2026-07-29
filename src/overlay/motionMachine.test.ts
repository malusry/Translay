import { describe, expect, test } from "vitest";
import {
  initialMotionState,
  reduceMotionState,
  type MotionState,
} from "./motionMachine";

function loading(requestId = 7): MotionState {
  return reduceMotionState(initialMotionState, {
    type: "loading",
    requestId,
  });
}

describe("overlay motion state machine", () => {
  test("moves one request through loading, preparation, reveal and settle", () => {
    const preparing = reduceMotionState(loading(), {
      type: "prepare",
      requestId: 7,
      loadingGeometry: { x: 10, y: 20, width: 148, height: 58 },
    });
    const revealing = reduceMotionState(preparing, {
      type: "reveal",
      requestId: 7,
    });
    const settled = reduceMotionState(revealing, {
      type: "settle",
      requestId: 7,
    });

    expect(preparing.phase).toBe("preparing");
    expect(revealing.phase).toBe("revealing");
    expect(settled.phase).toBe("settled");
  });

  test("ignores stale events from an older request", () => {
    const current = loading(9);
    expect(
      reduceMotionState(current, { type: "settle", requestId: 8 }),
    ).toBe(current);
  });

  test("a newer request supersedes an in-progress dismissal", () => {
    const dismissing = reduceMotionState(
      reduceMotionState(loading(), {
        type: "dismiss",
        requestId: 7,
        reason: "manual",
      }),
      { type: "loading", requestId: 8 },
    );

    expect(dismissing).toMatchObject({
      requestId: 8,
      phase: "loading",
      dismissReason: null,
    });
  });

  test("automatic dismissal is cancellable by renewed reading intent", () => {
    const settled = reduceMotionState(loading(), {
      type: "settle-immediately",
      requestId: 7,
    });
    const dismissing = reduceMotionState(settled, {
      type: "dismiss",
      requestId: 7,
      reason: "automatic",
    });
    const restored = reduceMotionState(dismissing, {
      type: "cancel-dismiss",
      requestId: 7,
    });

    expect(restored.phase).toBe("settled");
    expect(restored.dismissReason).toBeNull();
  });

  test("manual dismissal cannot be cancelled by hover", () => {
    const dismissing = reduceMotionState(loading(), {
      type: "dismiss",
      requestId: 7,
      reason: "manual",
    });

    expect(
      reduceMotionState(dismissing, {
        type: "cancel-dismiss",
        requestId: 7,
      }),
    ).toBe(dismissing);
  });

  test("only the matching hidden event resets the machine", () => {
    const current = loading(7);
    expect(
      reduceMotionState(current, { type: "hidden", requestId: 6 }),
    ).toBe(current);
    expect(
      reduceMotionState(current, { type: "hidden", requestId: 7 }),
    ).toEqual(initialMotionState);
  });
});
