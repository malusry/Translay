import type { WindowGeometry } from "./overlayMotion";

export type MotionPhase =
  | "idle"
  | "loading"
  | "preparing"
  | "revealing"
  | "settled"
  | "dismissing";

export type DismissReason = "automatic" | "manual";

export type MotionState = {
  requestId: number;
  phase: MotionPhase;
  loadingGeometry: WindowGeometry | null;
  dismissReason: DismissReason | null;
};

export type MotionEvent =
  | { type: "loading"; requestId: number }
  | {
      type: "prepare";
      requestId: number;
      loadingGeometry: WindowGeometry | null;
    }
  | { type: "settle-immediately"; requestId: number }
  | { type: "reveal"; requestId: number }
  | { type: "settle"; requestId: number }
  | {
      type: "dismiss";
      requestId: number;
      reason: DismissReason;
    }
  | { type: "cancel-dismiss"; requestId: number }
  | { type: "hidden"; requestId: number };

export const initialMotionState: MotionState = {
  requestId: 0,
  phase: "idle",
  loadingGeometry: null,
  dismissReason: null,
};

export function reduceMotionState(
  state: MotionState,
  event: MotionEvent,
): MotionState {
  if (event.type === "loading") {
    if (event.requestId < state.requestId) return state;
    return {
      requestId: event.requestId,
      phase: "loading",
      loadingGeometry: null,
      dismissReason: null,
    };
  }

  if (event.requestId !== state.requestId) {
    if (
      event.type === "settle-immediately" &&
      event.requestId > state.requestId
    ) {
      return {
        requestId: event.requestId,
        phase: "settled",
        loadingGeometry: null,
        dismissReason: null,
      };
    }
    return state;
  }

  switch (event.type) {
    case "prepare":
      if (state.phase === "dismissing") return state;
      return {
        ...state,
        phase: "preparing",
        loadingGeometry: event.loadingGeometry,
        dismissReason: null,
      };
    case "settle-immediately":
      return {
        ...state,
        phase: "settled",
        loadingGeometry: null,
        dismissReason: null,
      };
    case "reveal":
      return state.phase === "preparing"
        ? { ...state, phase: "revealing" }
        : state;
    case "settle":
      return state.phase === "revealing"
        ? { ...state, phase: "settled", dismissReason: null }
        : state;
    case "dismiss":
      if (state.phase === "idle" || state.phase === "dismissing") return state;
      return { ...state, phase: "dismissing", dismissReason: event.reason };
    case "cancel-dismiss":
      return state.phase === "dismissing" &&
        state.dismissReason === "automatic"
        ? { ...state, phase: "settled", dismissReason: null }
        : state;
    case "hidden":
      return initialMotionState;
    default:
      return state;
  }
}
