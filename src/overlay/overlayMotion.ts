export const MATERIALIZE_MIN_DURATION_MS = 300;
export const MATERIALIZE_MEDIUM_DURATION_MS = 360;
export const MATERIALIZE_MAX_DURATION_MS = 420;
export const MATERIALIZE_SAFETY_PADDING_MS = 200;
export const DISMISS_DURATION_MS = 630;
export const DISMISS_SAFETY_TIMEOUT_MS = 820;
export const LOADING_OVERLAY_WIDTH = 148;
export const LOADING_OVERLAY_HEIGHT = 58;

export type WindowGeometry = {
  x: number;
  y: number;
  width: number;
  height: number;
};

export type RevealOrigin = {
  left: number;
  top: number;
  width: number;
  height: number;
};

export type MaterializeTiming = {
  durationMs: number;
  contentDelayMs: number;
  controlsDelayMs: number;
};

type WindowGeometrySource = Pick<
  Window,
  "screenX" | "screenY" | "innerWidth" | "innerHeight"
>;

export function readWindowGeometry(
  source: WindowGeometrySource,
): WindowGeometry {
  return {
    x: finiteNumber(source.screenX),
    y: finiteNumber(source.screenY),
    width: positiveNumber(source.innerWidth),
    height: positiveNumber(source.innerHeight),
  };
}

export function calculateRevealOrigin(
  loading: WindowGeometry | null,
  target: WindowGeometry,
): RevealOrigin {
  const requestedWidth = loading?.width ?? LOADING_OVERLAY_WIDTH;
  const requestedHeight = loading?.height ?? LOADING_OVERLAY_HEIGHT;
  const width = Math.min(positiveNumber(requestedWidth), target.width);
  const height = Math.min(positiveNumber(requestedHeight), target.height);
  const requestedLeft = loading ? loading.x - target.x : 0;
  const requestedTop = loading ? loading.y - target.y : 0;

  return {
    left: clamp(requestedLeft, 0, Math.max(0, target.width - width)),
    top: clamp(requestedTop, 0, Math.max(0, target.height - height)),
    width,
    height,
  };
}

export function calculateMaterializeTiming(
  origin: RevealOrigin,
  target: Pick<WindowGeometry, "width" | "height">,
): MaterializeTiming {
  const expansion =
    Math.max(0, target.width - origin.width) +
    Math.max(0, target.height - origin.height);
  const durationMs =
    expansion <= 150
      ? MATERIALIZE_MIN_DURATION_MS
      : expansion <= 380
        ? MATERIALIZE_MEDIUM_DURATION_MS
        : MATERIALIZE_MAX_DURATION_MS;

  return {
    durationMs,
    contentDelayMs: Math.round(durationMs * 0.32),
    controlsDelayMs: Math.round(durationMs * 0.58),
  };
}

function finiteNumber(value: number): number {
  return Number.isFinite(value) ? value : 0;
}

function positiveNumber(value: number): number {
  return Number.isFinite(value) ? Math.max(1, value) : 1;
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, value));
}
