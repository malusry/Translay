import {
  type CSSProperties,
  useCallback,
  useEffect,
  useLayoutEffect,
  useReducer,
  useRef,
  useState,
} from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import type { CapturePayload } from "../shared/types";
import {
  attachLifecycleSynchronization,
  startBoundedPolling,
  syncLatestCapture as synchronizeLatestCapture,
} from "./overlayDelivery";
import {
  applyPayload,
  shouldBridgeLoadingToResult,
  statusText,
  waitingPayload,
} from "./overlayState";
import {
  DISMISS_SAFETY_TIMEOUT_MS,
  calculateMaterializeTiming,
  calculateRevealOrigin,
  LOADING_OVERLAY_HEIGHT,
  LOADING_OVERLAY_WIDTH,
  MATERIALIZE_SAFETY_PADDING_MS,
  readWindowGeometry,
  type WindowGeometry,
} from "./overlayMotion";
import {
  initialMotionState,
  reduceMotionState,
  type MotionState,
} from "./motionMachine";
import {
  acknowledgeCapture,
  copyTranslation as copyTranslationToClipboard,
  dismissOverlay,
  fitOverlayHeight,
  getLatestCapture,
  notifyOverlayFrontendReady,
  onCaptureResult,
  onOverlayDismissRequested,
  retryCapture,
  setOverlayHovered,
} from "./overlayIpc";
import { developmentPreview } from "./overlayPreview";
import { OverlayView } from "./OverlayView";
import "./overlay.css";

const ENABLE_STAGED_REVEAL = true;

type MotionDebugFrame = {
  elapsedMs: number;
  stage: string;
  surfaceOpacity: string;
  surfaceTransform: string;
  surfaceWidth: string;
  surfaceHeight: string;
  surfaceClipPath: string;
  surfaceAnimation: string;
  contentOpacity: string;
  contentTransform: string;
  contentAnimation: string;
  controlsOpacity: string;
  controlsTransform: string;
  controlsAnimation: string;
};

type MotionDebugWindow = Window & {
  __translayMotionFrames?: MotionDebugFrame[];
};

export function Overlay() {
  const previewRef = useRef(developmentPreview());
  const [payload, setPayload] = useState(previewRef.current ?? waitingPayload);
  const [copiedRequestId, setCopiedRequestId] = useState<number | null>(null);
  const [copyFailedRequestId, setCopyFailedRequestId] = useState<number | null>(
    null,
  );
  const [departingStatus, setDepartingStatus] = useState<{
    requestId: number;
    text: string;
  } | null>(null);
  const initialPreviewMotion: MotionState = previewRef.current
    ? {
        requestId: previewRef.current.requestId,
        phase:
          previewRef.current.phase === "capturing" ||
          previewRef.current.phase === "translating"
            ? "loading"
            : "settled",
        loadingGeometry: null,
        dismissReason: null,
      }
    : initialMotionState;
  const [motionState, dispatchMotion] = useReducer(
    reduceMotionState,
    initialPreviewMotion,
  );
  const payloadRef = useRef(previewRef.current ?? waitingPayload);
  const motionStateRef = useRef(motionState);
  motionStateRef.current = motionState;
  const overlayRef = useRef<HTMLElement | null>(null);
  const loadingGeometryRef = useRef<{
    requestId: number;
    geometry: WindowGeometry;
  } | null>(null);
  const pendingRevealAcknowledgementRef = useRef<number | null>(null);
  const pollingRef = useRef<(() => void) | undefined>(undefined);
  const departingStatusTimerRef = useRef<number | undefined>(undefined);
  const deliveryErrorReportedRef = useRef(false);
  const dismissCompletionRequestRef = useRef<number | null>(null);

  const reportDeliveryError = useCallback(
    (stage: string, error: unknown) => {
      console.error(`[Translay] overlay IPC failed during ${stage}`, error);
      if (deliveryErrorReportedRef.current) return;
      deliveryErrorReportedRef.current = true;
      const failurePayload: CapturePayload = {
        ...waitingPayload,
        errorCode: "OVERLAY_IPC_FAILED",
        errorMessage: "界面通信未连接，请重新启动 Translay",
      };
      payloadRef.current = failurePayload;
      setPayload(failurePayload);
    },
    [],
  );

  const applyLatestPayload = useCallback((next: CapturePayload): boolean => {
    if (next.requestId < payloadRef.current.requestId) return false;
    const current = payloadRef.current;
    if (
      next.requestId === current.requestId &&
      motionStateRef.current.phase === "dismissing" &&
      motionStateRef.current.dismissReason === "manual"
    ) {
      return true;
    }
    if (shouldBridgeLoadingToResult(current, next)) {
      const loadingGeometry =
        loadingGeometryRef.current?.requestId === next.requestId
          ? loadingGeometryRef.current.geometry
          : null;
      dispatchMotion(
        ENABLE_STAGED_REVEAL
          ? {
              type: "prepare",
              requestId: next.requestId,
              loadingGeometry,
            }
          : { type: "settle-immediately", requestId: next.requestId },
      );
      pendingRevealAcknowledgementRef.current = ENABLE_STAGED_REVEAL
        ? next.requestId
        : null;
      window.clearTimeout(departingStatusTimerRef.current);
      const bridge = {
        requestId: current.requestId,
        text: statusText(current),
      };
      setDepartingStatus(bridge);
      departingStatusTimerRef.current = window.setTimeout(() => {
        setDepartingStatus((visible) =>
          visible?.requestId === bridge.requestId ? null : visible,
        );
      }, 200);
    } else if (next.requestId > current.requestId) {
      window.clearTimeout(departingStatusTimerRef.current);
      setDepartingStatus(null);
      pendingRevealAcknowledgementRef.current = null;
      const nextIsLoading =
        next.phase === "capturing" || next.phase === "translating";
      dispatchMotion(
        nextIsLoading
          ? { type: "loading", requestId: next.requestId }
          : { type: "settle-immediately", requestId: next.requestId },
      );
    }
    payloadRef.current = applyPayload(current, next);
    setPayload(payloadRef.current);
    setCopiedRequestId((current) =>
      current === payloadRef.current.requestId ? current : null,
    );
    setCopyFailedRequestId((current) =>
      current === payloadRef.current.requestId ? current : null,
    );
    return true;
  }, []);

  useEffect(() => {
    const motionPreview = new URLSearchParams(window.location.search).get(
      "preview",
    );
    if (
      !import.meta.env.DEV ||
      (motionPreview !== "motion" && motionPreview !== "motion-compact")
    ) {
      return;
    }

    const debugWindow = window as MotionDebugWindow;
    const frames: MotionDebugFrame[] = [];
    debugWindow.__translayMotionFrames = frames;
    document.documentElement.dataset.translayMotionFrames = "[]";
    let frameTimer: number | undefined;
    let stopTimer: number | undefined;

    const transitionTimer = window.setTimeout(() => {
      const startedAt = performance.now();
      const recordFrame = () => {
        const root = overlayRef.current;
        const surface = root?.querySelector<HTMLElement>(".overlay-surface");
        const content = root?.querySelector<HTMLElement>(".translation");
        const controls = root?.querySelector<HTMLElement>(".tools");
        if (root && surface && content && controls) {
          const surfaceStyle = window.getComputedStyle(surface);
          const contentStyle = window.getComputedStyle(content);
          const controlsStyle = window.getComputedStyle(controls);
          frames.push({
            elapsedMs: Math.round(performance.now() - startedAt),
            stage: root.dataset.revealStage ?? "",
            surfaceOpacity: surfaceStyle.opacity,
            surfaceTransform: surfaceStyle.transform,
            surfaceWidth: surfaceStyle.width,
            surfaceHeight: surfaceStyle.height,
            surfaceClipPath: surfaceStyle.clipPath,
            surfaceAnimation: surfaceStyle.animationName,
            contentOpacity: contentStyle.opacity,
            contentTransform: contentStyle.transform,
            contentAnimation: contentStyle.animationName,
            controlsOpacity: controlsStyle.opacity,
            controlsTransform: controlsStyle.transform,
            controlsAnimation: controlsStyle.animationName,
          });
          document.documentElement.dataset.translayMotionFrames =
            JSON.stringify(frames);
        }
      };

      applyLatestPayload({
        ...waitingPayload,
        requestId: 42,
        phase: "translated",
        success: true,
        text:
          motionPreview === "motion-compact"
            ? "简短译文"
            : "翻译应当像系统原本就具备的能力一样自然存在，不打断阅读，也不要求用户切换窗口。真正好的动效会让译文从原位安静地成形。",
        errorMessage: null,
        applicationName: "Microsoft Edge",
        captureMethod: "UI Automation",
        translationMode: "conversational",
      });
      frameTimer = window.setInterval(recordFrame, 16);
      stopTimer = window.setTimeout(() => {
        window.clearInterval(frameTimer);
        recordFrame();
      }, 1_350);
    }, 900);

    return () => {
      window.clearTimeout(transitionTimer);
      window.clearInterval(frameTimer);
      window.clearTimeout(stopTimer);
      delete debugWindow.__translayMotionFrames;
    };
  }, [applyLatestPayload]);

  useEffect(() => {
    if (
      !import.meta.env.DEV ||
      new URLSearchParams(window.location.search).get("preview") !== "dismiss"
    ) {
      return;
    }

    const frames: Array<{
      elapsedMs: number;
      state: string;
      surfaceOpacity: string;
      surfaceTransform: string;
      surfaceAnimation: string;
      surfaceLeft: number;
      surfaceTop: number;
      surfaceWidth: number;
      surfaceHeight: number;
      contentOpacity: string;
      contentTransform: string;
    }> = [];
    document.documentElement.dataset.translayDismissFrames = "[]";
    let frameTimer: number | undefined;
    const startedAt = performance.now();
    const recordFrame = () => {
      const root = overlayRef.current;
      const surface = root?.querySelector<HTMLElement>(".overlay-surface");
      const content = root?.querySelector<HTMLElement>(".translation");
      if (!root || !surface || !content) return;
      const surfaceStyle = window.getComputedStyle(surface);
      const contentStyle = window.getComputedStyle(content);
      const surfaceRect = surface.getBoundingClientRect();
      frames.push({
        elapsedMs: Math.round(performance.now() - startedAt),
        state: root.dataset.motionState ?? "",
        surfaceOpacity: surfaceStyle.opacity,
        surfaceTransform: surfaceStyle.transform,
        surfaceAnimation: surfaceStyle.animationName,
        surfaceLeft: Math.round(surfaceRect.left),
        surfaceTop: Math.round(surfaceRect.top),
        surfaceWidth: Math.round(surfaceRect.width),
        surfaceHeight: Math.round(surfaceRect.height),
        contentOpacity: contentStyle.opacity,
        contentTransform: contentStyle.transform,
      });
      document.documentElement.dataset.translayDismissFrames =
        JSON.stringify(frames);
    };
    frameTimer = window.setInterval(recordFrame, 8);
    const dismissTimer = window.setTimeout(() => {
      dispatchMotion({
        type: "dismiss",
        requestId: payloadRef.current.requestId,
        reason: "manual",
      });
    }, 120);
    const stopTimer = window.setTimeout(() => {
      window.clearInterval(frameTimer);
      recordFrame();
    }, 940);

    return () => {
      window.clearInterval(frameTimer);
      window.clearTimeout(dismissTimer);
      window.clearTimeout(stopTimer);
      delete document.documentElement.dataset.translayDismissFrames;
    };
  }, []);

  useEffect(
    () => () => window.clearTimeout(departingStatusTimerRef.current),
    [],
  );

  const acknowledgeDeliveredCapture = useCallback((requestId: number) => {
    if (pendingRevealAcknowledgementRef.current === requestId) {
      // Delivery is complete, but reading time must begin only after the
      // materialize sequence has settled.
      return Promise.resolve(true);
    }
    return acknowledgeCapture(requestId);
  }, []);

  const syncLatestCapture = useCallback(
    () =>
      synchronizeLatestCapture({
        getLatest: getLatestCapture,
        apply: applyLatestPayload,
        acknowledge: acknowledgeDeliveredCapture,
        onError: reportDeliveryError,
      }),
    [
      acknowledgeDeliveredCapture,
      applyLatestPayload,
      reportDeliveryError,
    ],
  );

  const startPolling = useCallback(() => {
    if (pollingRef.current) return;
    let cancel: (() => void) | undefined;
    cancel = startBoundedPolling(syncLatestCapture, {
      intervalMs: 75,
      durationMs: 1_500,
      onStop: () => {
        if (pollingRef.current === cancel) pollingRef.current = undefined;
      },
    });
    pollingRef.current = cancel;
  }, [syncLatestCapture]);

  useEffect(() => {
    if (previewRef.current) return;
    let disposed = false;
    let unlistenCapture: UnlistenFn | undefined;
    let unlistenDismiss: UnlistenFn | undefined;
    let detachLifecycle: (() => void) | undefined;

    const synchronizeAndPoll = () => {
      if (disposed) return;
      void syncLatestCapture();
      startPolling();
    };
    const initialize = async () => {
      try {
        const disposeListener = await onCaptureResult(synchronizeAndPoll);
        if (disposed) {
          disposeListener();
          return;
        }
        unlistenCapture = disposeListener;
        const disposeDismissListener = await onOverlayDismissRequested(
          (requestId) => {
            if (
              Number.isSafeInteger(requestId) &&
              requestId === payloadRef.current.requestId
            ) {
              dispatchMotion({
                type: "dismiss",
                requestId,
                reason: "automatic",
              });
            }
          },
        );
        if (disposed) {
          disposeDismissListener();
          return;
        }
        unlistenDismiss = disposeDismissListener;
        await notifyOverlayFrontendReady();
        detachLifecycle = attachLifecycleSynchronization(
          document,
          window,
          synchronizeAndPoll,
        );

        // Listener first, getter second. Polling covers the case where this
        // initial getter is null and all later events are dropped while the
        // hidden WebView is suspended.
        synchronizeAndPoll();
      } catch (error) {
        reportDeliveryError("initialize", error);
      }
    };

    void initialize();
    return () => {
      disposed = true;
      unlistenCapture?.();
      unlistenDismiss?.();
      detachLifecycle?.();
      pollingRef.current?.();
      pollingRef.current = undefined;
    };
  }, [reportDeliveryError, startPolling, syncLatestCapture]);

  const dismiss = useCallback(() => {
    const requestId = payloadRef.current.requestId;
    if (requestId > 0) {
      if (pendingRevealAcknowledgementRef.current === requestId) {
        pendingRevealAcknowledgementRef.current = null;
      }
      dispatchMotion({
        type: "dismiss",
        requestId,
        reason: "manual",
      });
    }
  }, []);

  const setHovered = useCallback((hovered: boolean) => {
    const requestId = payloadRef.current.requestId;
    if (requestId > 0) {
      if (
        hovered &&
        motionStateRef.current.requestId === requestId &&
        motionStateRef.current.phase === "dismissing" &&
        motionStateRef.current.dismissReason === "automatic"
      ) {
        dispatchMotion({ type: "cancel-dismiss", requestId });
      }
      void setOverlayHovered(requestId, hovered);
    }
  }, []);

  const completeDismiss = useCallback(async (requestId: number) => {
    const current = motionStateRef.current;
    if (
      current.requestId !== requestId ||
      current.phase !== "dismissing" ||
      dismissCompletionRequestRef.current === requestId
    ) {
      return;
    }
    dismissCompletionRequestRef.current = requestId;
    try {
      const hidden = await dismissOverlay(requestId);
      if (hidden) {
        dispatchMotion({ type: "hidden", requestId });
      } else {
        dismissCompletionRequestRef.current = null;
        dispatchMotion({ type: "settle-immediately", requestId });
      }
    } catch (error) {
      if (previewRef.current) {
        return;
      }
      dismissCompletionRequestRef.current = null;
      console.error("[Translay] native overlay dismissal failed", error);
      dispatchMotion({ type: "settle-immediately", requestId });
    }
  }, []);

  const retry = useCallback(() => {
    setCopiedRequestId(null);
    setCopyFailedRequestId(null);
    void retryCapture();
  }, []);

  const copyTranslation = useCallback(async () => {
    const current = payloadRef.current;
    if (current.phase !== "translated" || !current.text) return;

    try {
      await copyTranslationToClipboard(current.text);
      setCopyFailedRequestId(null);
      setCopiedRequestId(current.requestId);
    } catch {
      setCopiedRequestId(null);
      setCopyFailedRequestId(current.requestId);
    }
  }, []);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") dismiss();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [dismiss]);

  const isFailure =
    payload.phase === "captureFailed" ||
    payload.phase === "translationFailed";
  const isTranslated = payload.phase === "translated";
  const isLoading =
    payload.phase === "capturing" || payload.phase === "translating";
  const isCompact =
    isTranslated &&
    !payload.text.includes("\n") &&
    Array.from(payload.text).length <= 18;
  const copied = copiedRequestId === payload.requestId;
  const copyFailed = copyFailedRequestId === payload.requestId;
  const activeMotionPhase =
    motionState.requestId === payload.requestId
      ? motionState.phase
      : isLoading
        ? "loading"
        : "settled";
  const targetGeometry = readWindowGeometry(window);
  const revealOrigin = calculateRevealOrigin(
    motionState.requestId === payload.requestId
      ? motionState.loadingGeometry
      : null,
    targetGeometry,
  );
  const materializeTiming = calculateMaterializeTiming(
    revealOrigin,
    targetGeometry,
  );
  const motionStyle = {
    "--motion-origin-left": `${revealOrigin.left - 1}px`,
    "--motion-origin-top": `${revealOrigin.top - 1}px`,
    "--motion-origin-width": `${revealOrigin.width + 2}px`,
    "--motion-origin-height": `${revealOrigin.height + 2}px`,
    "--motion-loading-left": `${revealOrigin.left}px`,
    "--motion-loading-top": `${revealOrigin.top}px`,
    "--motion-loading-width": `${revealOrigin.width}px`,
    "--motion-loading-height": `${revealOrigin.height}px`,
    "--motion-materialize": `${materializeTiming.durationMs}ms`,
    "--motion-delay-content": `${materializeTiming.contentDelayMs}ms`,
    "--motion-delay-controls": `${materializeTiming.controlsDelayMs}ms`,
  } as CSSProperties;

  useLayoutEffect(() => {
    if (!isLoading || payload.requestId === 0) return;
    const geometry = readWindowGeometry(window);
    const previewMode = new URLSearchParams(window.location.search).get(
      "preview",
    );
    const simulatesNativeLoadingWindow =
      import.meta.env.DEV &&
      (previewMode === "motion" || previewMode === "motion-compact");
    loadingGeometryRef.current = {
      requestId: payload.requestId,
      geometry: simulatesNativeLoadingWindow
        ? {
            ...geometry,
            width: LOADING_OVERLAY_WIDTH,
            height: LOADING_OVERLAY_HEIGHT,
          }
        : geometry,
    };
  }, [isLoading, payload.requestId]);

  const measureDesiredHeight = useCallback((): number | null => {
    const overlay = overlayRef.current;
    const header = overlay?.querySelector<HTMLElement>(".overlay-header");
    const translation = overlay?.querySelector<HTMLElement>(".translation");
    const status = overlay?.querySelector<HTMLElement>(".status");
    const tools = overlay?.querySelector<HTMLElement>(".tools");
    if (!overlay || !header || !translation || !status || !tools) return null;

    const style = window.getComputedStyle(overlay);
    const verticalChrome =
      Number.parseFloat(style.paddingTop) +
      Number.parseFloat(style.paddingBottom) +
      Number.parseFloat(style.borderTopWidth) +
      Number.parseFloat(style.borderBottomWidth);
    const translationStyle = window.getComputedStyle(translation);

    const measurement = translation.cloneNode(true) as HTMLElement;
    measurement.style.position = "fixed";
    measurement.style.left = "-10000px";
    measurement.style.top = "0";
    measurement.style.width = `${translation.clientWidth}px`;
    measurement.style.height = "auto";
    measurement.style.minHeight = "0";
    measurement.style.flex = "none";
    measurement.style.overflow = "visible";
    measurement.style.visibility = "hidden";
    document.body.appendChild(measurement);
    const contentHeight = measurement.scrollHeight;
    measurement.remove();

    const desiredHeight = isCompact
      ? verticalChrome +
        Math.max(status.offsetHeight, tools.offsetHeight, contentHeight) +
        18
      : verticalChrome +
        header.offsetHeight +
        Number.parseFloat(translationStyle.marginTop) +
        contentHeight +
        18;
    return Math.max(60, Math.min(320, Math.ceil(desiredHeight)));
  }, [isCompact]);

  useLayoutEffect(() => {
    if (
      !ENABLE_STAGED_REVEAL ||
      activeMotionPhase !== "preparing" ||
      payload.requestId === 0
    ) {
      return;
    }

    let measureFrame = 0;
    let firstSettleFrame = 0;
    let secondSettleFrame = 0;
    let cancelled = false;
    const beginReveal = () => {
      dispatchMotion({ type: "reveal", requestId: payload.requestId });
    };

    measureFrame = window.requestAnimationFrame(() => {
      const desiredHeight = measureDesiredHeight();
      const fit =
        desiredHeight !== null &&
        Math.abs(window.innerHeight - desiredHeight) > 2
          ? fitOverlayHeight(payload.requestId, desiredHeight)
          : Promise.resolve(true);

      void fit
        .catch((error) => {
          console.error("[Translay] pre-reveal height fit failed", error);
        })
        .finally(() => {
          if (cancelled) return;
          // The native window has reached its final bounds. Commit two stable
          // transparent frames before materializing the visual surface.
          firstSettleFrame = window.requestAnimationFrame(() => {
            overlayRef.current?.getBoundingClientRect();
            secondSettleFrame = window.requestAnimationFrame(beginReveal);
          });
        });
    });
    const safetyTimer = window.setTimeout(
      beginReveal,
      materializeTiming.durationMs + MATERIALIZE_SAFETY_PADDING_MS,
    );

    return () => {
      cancelled = true;
      window.cancelAnimationFrame(measureFrame);
      window.cancelAnimationFrame(firstSettleFrame);
      window.cancelAnimationFrame(secondSettleFrame);
      window.clearTimeout(safetyTimer);
    };
  }, [
    activeMotionPhase,
    materializeTiming.durationMs,
    measureDesiredHeight,
    payload.requestId,
  ]);

  const finishReveal = useCallback((requestId: number) => {
    dispatchMotion({ type: "settle", requestId });
    if (pendingRevealAcknowledgementRef.current === requestId) {
      pendingRevealAcknowledgementRef.current = null;
      void acknowledgeCapture(requestId);
    }
  }, []);

  useEffect(() => {
    if (activeMotionPhase !== "revealing") return;
    const safetyTimer = window.setTimeout(
      () => finishReveal(payload.requestId),
      materializeTiming.durationMs + MATERIALIZE_SAFETY_PADDING_MS,
    );
    return () => window.clearTimeout(safetyTimer);
  }, [
    activeMotionPhase,
    finishReveal,
    materializeTiming.durationMs,
    payload.requestId,
  ]);

  useEffect(() => {
    if (activeMotionPhase !== "dismissing") return;
    const safetyTimer = window.setTimeout(
      () => void completeDismiss(payload.requestId),
      DISMISS_SAFETY_TIMEOUT_MS,
    );
    return () => window.clearTimeout(safetyTimer);
  }, [activeMotionPhase, completeDismiss, payload.requestId]);

  useLayoutEffect(() => {
    if (
      previewRef.current ||
      payload.requestId === 0 ||
      isLoading ||
      activeMotionPhase !== "settled"
    ) {
      return;
    }

    const fitToContent = () => {
      const desiredHeight = measureDesiredHeight();
      if (
        desiredHeight !== null &&
        Math.abs(window.innerHeight - desiredHeight) > 2
      ) {
        void fitOverlayHeight(payload.requestId, desiredHeight);
      }
    };
    const timers = [
      window.setTimeout(fitToContent, 40),
      window.setTimeout(fitToContent, 220),
    ];

    return () => timers.forEach((timer) => window.clearTimeout(timer));
  }, [
    activeMotionPhase,
    isLoading,
    measureDesiredHeight,
    payload.phase,
    payload.requestId,
    payload.text,
  ]);

  return (
    <OverlayView
      overlayRef={overlayRef}
      payload={payload}
      activeMotionPhase={activeMotionPhase}
      motionStyle={motionStyle}
      isCompact={isCompact}
      isTranslated={isTranslated}
      isFailure={isFailure}
      isLoading={isLoading}
      copied={copied}
      copyFailed={copyFailed}
      departingStatusText={
        departingStatus?.requestId === payload.requestId
          ? departingStatus.text
          : null
      }
      onHoveredChange={setHovered}
      onSurfaceAnimationEnd={(event) => {
        if (event.currentTarget !== event.target) return;
        if (activeMotionPhase === "revealing") {
          finishReveal(payload.requestId);
        } else if (
          activeMotionPhase === "dismissing" &&
          (event.animationName === "surface-air-dissolve" ||
            event.animationName === "content-dismiss-reduced")
        ) {
          void completeDismiss(payload.requestId);
        }
      }}
      onCopy={() => void copyTranslation()}
      onRetry={retry}
      onDismiss={dismiss}
    />
  );
}
