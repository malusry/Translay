import { describe, expect, it } from "vitest";
import {
  acceptSnapshot,
  previewAccessibility,
  previewSnapshot,
} from "./ipc";

describe("Mac prototype snapshot delivery", () => {
  it("keeps a menu update when an earlier initial read arrives late", () => {
    const newer = { ...previewSnapshot, mode: "academic" as const, revision: 2 };
    expect(acceptSnapshot(newer, previewSnapshot)).toEqual(newer);
  });

  it("does not resurrect a running state after shutdown", () => {
    const stopped = { ...previewSnapshot, exiting: true, revision: 5 };
    expect(acceptSnapshot(stopped, { ...previewSnapshot, revision: 4 })).toEqual(stopped);
  });

  it("never reports browser preview as native accessibility authorization", () => {
    expect(previewAccessibility).toEqual({ supported: false, trusted: null });
  });
});
