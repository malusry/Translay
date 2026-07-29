import { afterEach, describe, expect, test, vi } from "vitest";
import type { CapturePayload } from "../shared/types";
import {
  attachLifecycleSynchronization,
  startBoundedPolling,
  syncLatestCapture,
} from "./overlayDelivery";
import { waitingPayload } from "./overlayState";

function payload(requestId: number): CapturePayload {
  return {
    ...waitingPayload,
    requestId,
    success: true,
    text: `selection-${requestId}`,
    elapsedMs: 76,
    errorMessage: null,
  };
}

afterEach(() => {
  vi.useRealTimers();
});

describe("overlay delivery recovery", () => {
  test("polling recovers a payload written after the initial null getter", async () => {
    vi.useFakeTimers();
    let latest: CapturePayload | null = null;
    const applied: number[] = [];
    const acknowledged: number[] = [];
    const synchronize = () =>
      syncLatestCapture({
        getLatest: async () => latest,
        apply: (next) => {
          applied.push(next.requestId);
          return true;
        },
        acknowledge: async (requestId) => {
          acknowledged.push(requestId);
          return true;
        },
      });

    startBoundedPolling(synchronize, { intervalMs: 75, durationMs: 1_500 });
    await vi.advanceTimersByTimeAsync(1);
    expect(applied).toEqual([]);

    // No capture-result event is delivered. The next getter poll is the only
    // recovery mechanism.
    latest = payload(41);
    await vi.advanceTimersByTimeAsync(75);
    expect(applied).toEqual([41]);
    expect(acknowledged).toEqual([41]);
  });

  test("the applied request id is the id sent in the acknowledgement", async () => {
    const acknowledged: number[] = [];
    const delivered = await syncLatestCapture({
      getLatest: async () => payload(42),
      apply: () => true,
      acknowledge: async (requestId) => {
        acknowledged.push(requestId);
        return true;
      },
    });

    expect(delivered).toBe(true);
    expect(acknowledged).toEqual([42]);
  });

  test("an older payload rejected by the guard is not acknowledged", async () => {
    const acknowledge = vi.fn(async () => true);
    const delivered = await syncLatestCapture({
      getLatest: async () => payload(43),
      apply: () => false,
      acknowledge,
    });

    expect(delivered).toBe(false);
    expect(acknowledge).not.toHaveBeenCalled();
  });

  test("a getter IPC failure is reported instead of being silently swallowed", async () => {
    const onError = vi.fn();
    const error = new Error("IPC denied");
    const delivered = await syncLatestCapture({
      getLatest: async () => {
        throw error;
      },
      apply: () => true,
      acknowledge: async () => true,
      onError,
    });

    expect(delivered).toBe(false);
    expect(onError).toHaveBeenCalledWith("get-latest", error);
  });

  test("an acknowledgement IPC failure is reported", async () => {
    const onError = vi.fn();
    const error = new Error("IPC denied");
    const delivered = await syncLatestCapture({
      getLatest: async () => payload(44),
      apply: () => true,
      acknowledge: async () => {
        throw error;
      },
      onError,
    });

    expect(delivered).toBe(false);
    expect(onError).toHaveBeenCalledWith("acknowledge", error);
  });

  test("polling is bounded when the WebView never receives a payload", async () => {
    vi.useFakeTimers();
    const synchronize = vi.fn(async () => false);
    startBoundedPolling(synchronize, { intervalMs: 75, durationMs: 1_500 });

    await vi.advanceTimersByTimeAsync(2_000);
    const countAfterDeadline = synchronize.mock.calls.length;
    await vi.advanceTimersByTimeAsync(2_000);

    expect(countAfterDeadline).toBeGreaterThan(1);
    expect(synchronize).toHaveBeenCalledTimes(countAfterDeadline);
  });

  test("visible and pageshow lifecycle signals restart synchronization", () => {
    const documentTarget = new EventTarget() as EventTarget & {
      visibilityState: DocumentVisibilityState;
    };
    documentTarget.visibilityState = "hidden";
    const windowTarget = new EventTarget();
    const synchronize = vi.fn();
    const detach = attachLifecycleSynchronization(
      documentTarget,
      windowTarget,
      synchronize,
    );

    documentTarget.dispatchEvent(new Event("visibilitychange"));
    expect(synchronize).not.toHaveBeenCalled();
    documentTarget.visibilityState = "visible";
    documentTarget.dispatchEvent(new Event("visibilitychange"));
    windowTarget.dispatchEvent(new Event("pageshow"));
    expect(synchronize).toHaveBeenCalledTimes(2);

    detach();
    windowTarget.dispatchEvent(new Event("pageshow"));
    expect(synchronize).toHaveBeenCalledTimes(2);
  });
});
